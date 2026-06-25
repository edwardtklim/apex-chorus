// APEX Velox — diagnose
//
// 이것이 APEX Core(AI+기기 제어 연동)의 첫 세포다.
// Action Loop: 읽기 → 추론(AI 구조화 제안) → 화이트리스트 검증 → [승인] → 실행 → 검증 → 롤백
//
// 안전 원칙(Security by Default):
//   - AI는 "명령을 생성"하지 않는다. 미리 정의된 화이트리스트에서 "선택"만 한다.
//   - 실제 명령/GUID는 Rust에 하드코딩 → AI가 헛소리/인젝션을 당해도 임의 실행 불가.
//   - 모든 동작은 되돌릴 수 있어야 함(rollback).
//   - 사람이 명시적으로 승인(y)해야만 실행 (--fix).
//   - 실행 후 반드시 검증 + 로그 기록.

use serde::Deserialize;
use std::io::{self, Write};
use std::process::Command;
use wmi::{COMLibrary, WMIConnection};

/// 이 온도(℃) 이상이면 규칙 기반 대체 제안이 전원 모드 하향을 제안.
const TEMP_WARN_C: f32 = 85.0;
/// 실행 기록 로그 파일.
const LOG_FILE: &str = "velox_actions.log";

/// 전원 구성표 화이트리스트. AI는 이 key 중에서만 고를 수 있다.
/// (key, 표시이름, GUID) — GUID는 Windows 기본값, 하드코딩.
fn plan_by_key(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        "balanced" => Some(("Balanced", "381b4222-f694-41f0-9685-ff5bb260df2e")),
        "high_performance" => Some(("High performance", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")),
        "power_saver" => Some(("Power saver", "a1841308-3541-4fab-bc81-f71556f20b4a")),
        _ => None,
    }
}

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
struct ThermalZone {
    #[serde(rename = "CurrentTemperature")]
    current_temperature: u32,
}

struct Snapshot {
    max_temp_c: Option<f32>,
    plan_guid: String,
    summary: String,
}

/// 화이트리스트된 안전·가역 동작만 존재한다. (스캐폴드: 현재 1종)
enum Action {
    SetPowerPlan {
        label: &'static str,
        guid: &'static str,
        rollback_guid: String,
    },
    None,
}

/// AI가 돌려주는 구조화 제안. 자유 텍스트가 아니라 이 스키마로만 받는다.
#[derive(Deserialize, Default)]
struct AiProposal {
    #[serde(default)]
    diagnosis: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    reason: String,
}

// ---------------- [1] 읽기 (Velox) ----------------

fn read_active_plan() -> (String, String) {
    if let Ok(out) = Command::new("powercfg").arg("/getactivescheme").output() {
        let s = String::from_utf8_lossy(&out.stdout);
        let guid = s
            .split("GUID:")
            .nth(1)
            .and_then(|t| t.trim().split_whitespace().next())
            .unwrap_or("")
            .to_string();
        let label = s
            .split('(')
            .nth(1)
            .and_then(|t| t.split(')').next())
            .unwrap_or("Unknown")
            .trim()
            .to_string();
        return (guid, label);
    }
    (String::new(), "Unknown".to_string())
}

fn read_max_temp() -> Option<f32> {
    let com = COMLibrary::new().ok()?;
    let wmi = WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()?;
    let temps: Vec<ThermalZone> = wmi.query().unwrap_or_default();
    temps
        .iter()
        .map(|t| (t.current_temperature as f32 / 10.0) - 273.15)
        .fold(None, |acc, c| Some(acc.map_or(c, |m: f32| m.max(c))))
}

fn collect() -> Snapshot {
    let (plan_guid, plan_label) = read_active_plan();
    let max_temp_c = read_max_temp();
    let temp_str = match max_temp_c {
        Some(c) => format!("{:.1}°C", c),
        None => "N/A (센서 읽기 실패 — 관리자 권한 필요)".to_string(),
    };
    let summary = format!(
        "- 현재 전원 모드: {} ({})\n- 최고 온도: {}",
        plan_label, plan_guid, temp_str
    );
    Snapshot {
        max_temp_c,
        plan_guid,
        summary,
    }
}

// ---------------- [2] 추론: AI 구조화 제안 ----------------

/// AI에게 시스템 상태를 주고, 화이트리스트 중 하나를 JSON으로 "선택"하게 한다.
/// 반환: (사람이 읽을 진단 텍스트, 검증 통과한 Action)
async fn ai_propose(snap: &Snapshot) -> Option<(String, Action)> {
    let prompt = format!(
        "너는 APEX Velox의 시스템 진단 엔진이다. 아래 시스템 상태를 보고 판단해라.\n\n\
         [엄격한 규칙]\n\
         - 반드시 아래 JSON 한 개로만 답한다. 그 외 설명/마크다운/코드펜스 금지.\n\
         - action 은 정확히 \"set_power_plan\" 또는 \"none\" 중 하나.\n\
         - action 이 \"set_power_plan\" 이면 target 은 정확히 \"balanced\", \"high_performance\", \"power_saver\" 중 하나.\n\
         - 안전하고 되돌릴 수 있는 경우에만 조치를 제안하고, 애매하면 action=\"none\".\n\n\
         [JSON 스키마]\n\
         {{\"diagnosis\":\"2~3줄 한국어 진단\",\"action\":\"set_power_plan|none\",\"target\":\"balanced|high_performance|power_saver|null\",\"reason\":\"왜 이 조치인지 한 줄\"}}\n\n\
         [시스템 상태]\n{}",
        snap.summary
    );

    let raw = crate::chorus::query_text(&prompt).await?;
    let json_str = extract_json(&raw)?;
    let p: AiProposal = serde_json::from_str(json_str).ok()?;

    let diagnosis = if p.reason.trim().is_empty() {
        p.diagnosis.clone()
    } else {
        format!("{}\n→ 이유: {}", p.diagnosis.trim(), p.reason.trim())
    };

    // ★ 화이트리스트 검증: AI의 선택을 하드코딩된 메뉴와 대조. 통과한 것만 Action이 됨.
    let action = match p.action.as_str() {
        "set_power_plan" => {
            let key = p.target.as_deref().unwrap_or("");
            match plan_by_key(key) {
                Some((label, guid)) if guid.to_lowercase() != snap.plan_guid.to_lowercase() => {
                    Action::SetPowerPlan {
                        label,
                        guid,
                        rollback_guid: snap.plan_guid.clone(),
                    }
                }
                _ => Action::None, // 알 수 없는 target 이거나 이미 그 모드 → 무시
            }
        }
        _ => Action::None,
    };

    Some((diagnosis, action))
}

/// AI 응답에서 첫 '{' ~ 마지막 '}' 사이만 추출 (코드펜스/잡텍스트 방어).
fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

// ---------------- [3] 대체 제안 (AI 불가 시 규칙 기반) ----------------

fn propose_rule_based(snap: &Snapshot) -> Action {
    if let Some(t) = snap.max_temp_c {
        if t >= TEMP_WARN_C {
            if let Some((label, guid)) = plan_by_key("balanced") {
                if guid.to_lowercase() != snap.plan_guid.to_lowercase() {
                    return Action::SetPowerPlan {
                        label,
                        guid,
                        rollback_guid: snap.plan_guid.clone(),
                    };
                }
            }
        }
    }
    Action::None
}

// ---------------- [5] 실행 ----------------

fn apply_power_plan(guid: &str) -> bool {
    Command::new("powercfg")
        .args(["/setactive", guid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn log_action(label: &str, guid: &str, rollback: &str, verified: bool) {
    use std::fs::OpenOptions;
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = writeln!(
            f,
            "[diagnose] set_power_plan label={} guid={} verified={} rollback={}",
            label, guid, verified, rollback
        );
    }
}

// ---------------- 메인 루프 ----------------

pub async fn run(fix: bool) {
    println!("=== APEX Velox — diagnose ===\n");

    // [1] 읽기
    let snap = collect();
    println!("[1] 시스템 상태 (읽기)\n{}\n", snap.summary);

    // [2] 추론: AI가 구조화된 조치를 선택 (실패 시 규칙 기반 대체)
    println!("[2] AI 진단 + 조치 선택 (추론)...");
    let action = match ai_propose(&snap).await {
        Some((diagnosis, act)) => {
            println!("{}\n", diagnosis.trim());
            act
        }
        None => {
            println!("(AI 사용 불가/응답 파싱 실패 — 규칙 기반으로 대체)\n");
            propose_rule_based(&snap)
        }
    };

    // [3] 화이트리스트 검증 결과
    match &action {
        Action::None => {
            println!("[3] 제안: 적용할 안전한 조치 없음 (양호하거나 수동 확인 필요).");
            return;
        }
        Action::SetPowerPlan { label, .. } => {
            println!(
                "[3] AI가 선택한 조치: 전원 모드 → '{}'  (화이트리스트 검증 통과 ✓, 되돌릴 수 있음)",
                label
            );
        }
    }

    if !fix {
        println!("\n실제로 적용하려면:  velox diagnose --fix");
        return;
    }

    if let Action::SetPowerPlan {
        label,
        guid,
        rollback_guid,
    } = action
    {
        // [4] 승인 (사람이 최종 결정)
        print!("\n[4] 이 조치를 적용할까요? (y/N): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin().read_line(&mut input).ok();
        if input.trim().to_lowercase() != "y" {
            println!("취소됨 — 아무 변경 없음.");
            return;
        }

        // [5] 실행
        println!("[5] 실행: 전원 모드 → {}", label);
        if !apply_power_plan(guid) {
            println!("✗ 실행 실패 (권한 문제이거나 해당 전원 모드가 없을 수 있음).");
            return;
        }

        // [6] 검증
        let (new_guid, new_label) = read_active_plan();
        let verified = new_guid.to_lowercase() == guid.to_lowercase();
        println!(
            "[6] 검증: 현재 전원 모드 = {} {}",
            new_label,
            if verified { "✓" } else { "✗ (불일치)" }
        );

        // [7] 롤백 안내 + 로그
        println!("[7] 되돌리려면:  powercfg /setactive {}", rollback_guid);
        log_action(label, guid, &rollback_guid, verified);
        println!("\n기록됨 → {}", LOG_FILE);
    }
}
