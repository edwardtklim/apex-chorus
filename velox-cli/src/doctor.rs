// APEX Doctor — "내 컴퓨터 왜 느려?" 한 명령 종합 진단
//
// 모든 서브시스템(CPU/RAM/Disk/GPU/온도/배터리/시작프로그램/이벤트로그)을 읽어서(읽기 전용)
// **typed EvidenceBundle**로 만들고, 그 Evidence만 AI에게 전달한다(정책 게이트 경유).
// 원칙(불변조건 2.3): AI payload는 EvidenceBundle에서만 생성 · 전체 사용자 경로/원문 로그는
// Evidence로 만들지 않는다(이벤트 로그는 원문 대신 유무/개수 Finding으로).

use serde::Deserialize;
use std::process::Command;
use sysinfo::{Disks, System};
use velox_core::evidence::{
    EvidenceBundle, EvidenceData, EvidenceError, EvidenceId, EvidenceItem, EvidenceSource,
};
use velox_core::privacy::ContextScope;
use wmi::{COMLibrary, WMIConnection};

/// sensitivity가 scope 이내일 때만 Evidence 항목을 추가(데이터 최소화). source=Health.
fn push(
    out: &mut Vec<EvidenceItem>,
    scope: ContextScope,
    id: &str,
    sensitivity: ContextScope,
    data: EvidenceData,
) {
    if sensitivity <= scope {
        out.push(EvidenceItem {
            id: EvidenceId(id.to_string()),
            source: EvidenceSource::Health,
            sensitivity,
            data,
        });
    }
}

fn metric(name: &str, value: f64, unit: &str) -> EvidenceData {
    EvidenceData::Metric {
        name: name.into(),
        value,
        unit: unit.into(),
    }
}

fn cpu_ram_disk(scope: ContextScope, out: &mut Vec<EvidenceItem>) {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_name = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let cores = sys.cpus().len();
    let cpu_usage = sys.global_cpu_info().cpu_usage();
    let total = sys.total_memory() / 1024 / 1024;
    let used = sys.used_memory() / 1024 / 1024;

    push(
        out,
        scope,
        "doctor.cpu_model",
        ContextScope::System,
        EvidenceData::Fact {
            name: "CPU".into(),
            value: cpu_name,
        },
    );
    push(
        out,
        scope,
        "doctor.cpu_cores",
        ContextScope::System,
        metric("논리 코어", cores as f64, ""),
    );
    push(
        out,
        scope,
        "doctor.cpu_usage",
        ContextScope::Minimal,
        metric("CPU 사용률", cpu_usage as f64, "%"),
    );
    push(
        out,
        scope,
        "doctor.ram_used_mb",
        ContextScope::System,
        metric("RAM 사용", used as f64, "MB"),
    );
    push(
        out,
        scope,
        "doctor.ram_total_mb",
        ContextScope::System,
        metric("RAM 총량", total as f64, "MB"),
    );

    let disks = Disks::new_with_refreshed_list();
    for (i, d) in disks.iter().enumerate() {
        let total_gb = d.total_space() / 1_000_000_000;
        let free_gb = d.available_space() / 1_000_000_000;
        if total_gb > 0 {
            // 마운트 지점(예: "C:\\")만 — 전체 사용자 경로는 포함하지 않는다.
            push(
                out,
                scope,
                &format!("doctor.disk.{i}"),
                ContextScope::System,
                EvidenceData::Fact {
                    name: format!("디스크 {}", d.mount_point().to_string_lossy()),
                    value: format!("{free_gb}GB free / {total_gb}GB"),
                },
            );
        }
    }
}

fn wmi_bits(scope: ContextScope, out: &mut Vec<EvidenceItem>) {
    let com = match COMLibrary::new() {
        Ok(c) => c,
        Err(_) => return,
    };

    if let Ok(con) = WMIConnection::new(com) {
        #[derive(Deserialize)]
        #[serde(rename = "Win32_VideoController")]
        struct G {
            #[serde(rename = "Name")]
            name: String,
        }
        for (i, g) in con.query::<G>().unwrap_or_default().into_iter().enumerate() {
            push(
                out,
                scope,
                &format!("doctor.gpu.{i}"),
                ContextScope::System,
                EvidenceData::Fact {
                    name: "GPU".into(),
                    value: g.name,
                },
            );
        }

        #[derive(Deserialize)]
        #[serde(rename = "Win32_Battery")]
        struct B {
            #[serde(rename = "EstimatedChargeRemaining")]
            c: u32,
        }
        if let Some(b) = con.query::<B>().unwrap_or_default().first() {
            push(
                out,
                scope,
                "doctor.battery",
                ContextScope::System,
                metric("배터리", b.c as f64, "%"),
            );
        }

        #[derive(Deserialize)]
        #[serde(rename = "Win32_StartupCommand")]
        struct S {
            #[serde(rename = "Name")]
            _name: Option<String>,
        }
        let startups = con.query::<S>().unwrap_or_default();
        push(
            out,
            scope,
            "doctor.startup_count",
            ContextScope::System,
            metric("시작 프로그램", startups.len() as f64, "개"),
        );
    }

    if let Ok(con2) = WMIConnection::with_namespace_path("ROOT\\WMI", com) {
        #[derive(Deserialize)]
        #[serde(rename = "MSAcpi_ThermalZoneTemperature")]
        struct T {
            #[serde(rename = "CurrentTemperature")]
            t: u32,
        }
        let temps = con2.query::<T>().unwrap_or_default();
        let maxc = temps
            .iter()
            .map(|x| (x.t as f32 / 10.0) - 273.15)
            .fold(f32::MIN, f32::max);
        if maxc > f32::MIN {
            push(
                out,
                scope,
                "doctor.max_temp_c",
                ContextScope::Minimal,
                metric("최고 온도", maxc as f64, "°C"),
            );
        }
    }
}

/// 이벤트 로그: **원문 대신 유무/개수만** Evidence로(전체 사용자 경로·앱 상세 유출 방지).
fn event_errors(scope: ContextScope, out: &mut Vec<EvidenceItem>) {
    let output = Command::new("wevtutil")
        .args([
            "qe",
            "System",
            "/q:*[System[(Level=1 or Level=2)]]",
            "/c:3",
            "/rd:true",
            "/f:text",
        ])
        .output();
    let (code, message) = match output {
        Ok(o) if o.status.success() => {
            let body = velox_core::util::decode_console(&o.stdout);
            if body.trim().is_empty() {
                (
                    "eventlog.clean",
                    "최근 시스템 심각/오류 이벤트 없음".to_string(),
                )
            } else {
                let n = body.matches("Event[").count().max(1);
                (
                    "eventlog.errors",
                    format!("최근 시스템 심각/오류 이벤트 발견 ({n}건 확인, 최대 3)"),
                )
            }
        }
        _ => ("eventlog.unavailable", "이벤트 로그 조회 실패".to_string()),
    };
    push(
        out,
        scope,
        "doctor.eventlog",
        ContextScope::System,
        EvidenceData::Finding {
            code: code.into(),
            message,
        },
    );
}

/// 시스템 전체를 읽어 승인 범위 안의 typed Evidence로 만든다.
fn build_evidence(scope: ContextScope) -> Result<EvidenceBundle, EvidenceError> {
    let mut items = Vec::new();
    cpu_ram_disk(scope, &mut items);
    wmi_bits(scope, &mut items);
    event_errors(scope, &mut items);
    EvidenceBundle::new(scope, items)
}

pub async fn run() {
    println!("=== APEX Doctor — 종합 진단 ===\n");
    println!("[1] 시스템 전체 스캔 (읽기 전용)...\n");

    // doctor는 CPU/GPU 이름 등 System 범위 데이터를 본다.
    let bundle = match build_evidence(ContextScope::System) {
        Ok(b) => b,
        Err(e) => {
            println!("Evidence 생성 실패: {e}");
            return;
        }
    };
    // [1] 표시 = AI에게 전송될 바로 그 Evidence (무엇을 보내는지 투명하게).
    let evidence = bundle.to_prompt();
    println!("{evidence}");

    println!("\n[2] AI 종합 진단...\n");
    let prompt = format!(
        "너는 PC 종합 진단 의사다. 아래 Evidence만 보고 한국어로 간결하게 답하라:\n\
         1) 전반 상태 한 줄 요약\n\
         2) 병목/의심되는 문제\n\
         3) 개선·업그레이드 추천\n\
         설명 위주로, 위험한 조치는 권하지 말 것. Evidence 밖 정보는 지어내지 말 것.\n\n\
         [PC 상태]\n{evidence}"
    );
    match crate::chorus::gated_text(
        "claude",
        velox_core::policy::AgentPurpose::Diagnose,
        ContextScope::System,
        prompt,
    )
    .await
    {
        Some(t) => println!("{}", t.trim()),
        None => {
            println!("(AI 종합 진단 생략 — 위 이유 참고. 위 [1] 스캔은 키/동의 없이도 동작합니다.)")
        }
    }
}
