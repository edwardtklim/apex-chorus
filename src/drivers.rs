// APEX Velox — drivers
//
// 드라이버/장치 상태를 읽는다. (읽기 전용 — 위험 사다리의 첫 칸, 안전)
// "문제 장치"(장치 관리자의 노란 느낌표 = ConfigManagerErrorCode != 0)를 골라낸다.
// diagnose 의 3단계 AI 가 이 정보를 보고 더 쓸모있는 진단을 하도록 컨텍스트로도 제공한다.

use serde::Deserialize;
use wmi::{COMLibrary, WMIConnection};

#[derive(Deserialize)]
#[serde(rename = "Win32_PnPEntity")]
struct PnpDevice {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "ConfigManagerErrorCode")]
    error_code: Option<u32>,
}

/// 흔한 Device Manager 오류 코드 설명.
fn code_meaning(code: u32) -> &'static str {
    match code {
        1 => "올바르게 구성되지 않음",
        3 => "드라이버 손상 또는 메모리 부족",
        10 => "장치를 시작할 수 없음",
        12 => "리소스 부족",
        14 => "재시작 필요",
        18 => "드라이버 재설치 필요",
        19 => "레지스트리 손상",
        22 => "비활성화됨",
        28 => "드라이버가 설치되지 않음",
        31 => "정상 작동하지 않음",
        37 => "드라이버 초기화 실패",
        39 => "드라이버 손상/누락",
        43 => "Windows가 문제로 인해 중지시킴",
        45 => "장치가 연결되어 있지 않음",
        _ => "기타 오류",
    }
}

struct DeviceScan {
    total: usize,
    problems: Vec<(String, u32)>, // (이름, 오류코드)
}

fn scan() -> Option<DeviceScan> {
    let com = COMLibrary::new().ok()?;
    let wmi = WMIConnection::new(com).ok()?;
    let devices: Vec<PnpDevice> = wmi.query().ok()?;

    let total = devices.len();
    let mut problems = Vec::new();
    for d in devices {
        if let Some(code) = d.error_code {
            if code != 0 {
                let name = d.name.unwrap_or_else(|| "(이름 없음)".to_string());
                problems.push((name, code));
            }
        }
    }
    Some(DeviceScan { total, problems })
}

/// diagnose 스냅샷에 끼워 넣을 한 줄 요약. (AI 컨텍스트용)
pub fn problem_summary() -> String {
    match scan() {
        None => "- 드라이버: 조회 실패".to_string(),
        Some(s) if s.problems.is_empty() => {
            format!("- 드라이버: 문제 장치 없음 (총 {}개 정상)", s.total)
        }
        Some(s) => {
            let names: Vec<String> = s
                .problems
                .iter()
                .map(|(n, c)| format!("{} (code {} {})", n, c, code_meaning(*c)))
                .collect();
            format!("- 드라이버 문제 장치 {}개: {}", s.problems.len(), names.join(", "))
        }
    }
}

/// CLI: velox drivers
pub fn run() {
    println!("=== APEX Velox — drivers (읽기 전용) ===\n");
    match scan() {
        None => println!("✗ 장치 조회 실패."),
        Some(s) => {
            println!("총 장치: {}개", s.total);
            if s.problems.is_empty() {
                println!("문제 장치: 없음 ✓");
            } else {
                println!("문제 장치: {}개 ⚠\n", s.problems.len());
                for (name, code) in &s.problems {
                    println!("  • {}\n    └ code {} — {}", name, code, code_meaning(*code));
                }
            }
        }
    }
}
