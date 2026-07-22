// APEX Doctor — "내 컴퓨터 왜 느려?" 한 명령 종합 진단
//
// 모든 서브시스템(CPU/RAM/Disk/GPU/온도/드라이버/배터리/시작프로그램/이벤트로그)을
// 읽어서(읽기 전용 = 안전) 하나의 스냅샷으로 묶고, AI가 종합 진단한다.
// 버킷 A(#1,2,4,5,11,14...)를 흡수하는 우산 명령.

use serde::Deserialize;
use std::process::Command;
use sysinfo::{Disks, System};
use wmi::{COMLibrary, WMIConnection};

fn truncate_chars(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{}…", t)
    } else {
        t
    }
}

fn cpu_ram_disk() -> String {
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
    let mem_pct = if total > 0 {
        used as f32 / total as f32 * 100.0
    } else {
        0.0
    };

    let mut out = format!(
        "- CPU: {} ({} 논리코어), 사용률 {:.1}%\n- RAM: {}MB / {}MB ({:.1}%)\n",
        cpu_name, cores, cpu_usage, used, total, mem_pct
    );

    let disks = Disks::new_with_refreshed_list();
    for d in &disks {
        let total_gb = d.total_space() / 1_000_000_000;
        let free_gb = d.available_space() / 1_000_000_000;
        if total_gb > 0 {
            out.push_str(&format!(
                "- Disk {}: {}GB 중 {}GB 여유\n",
                d.mount_point().to_string_lossy(),
                total_gb,
                free_gb
            ));
        }
    }
    out
}

fn wmi_bits() -> String {
    let mut out = String::new();
    let com = match COMLibrary::new() {
        Ok(c) => c,
        Err(_) => return "- (WMI 초기화 실패)\n".to_string(),
    };

    if let Ok(con) = WMIConnection::new(com) {
        #[derive(Deserialize)]
        #[serde(rename = "Win32_VideoController")]
        struct G {
            #[serde(rename = "Name")]
            name: String,
        }
        for g in con.query::<G>().unwrap_or_default() {
            out.push_str(&format!("- GPU: {}\n", g.name));
        }

        #[derive(Deserialize)]
        #[serde(rename = "Win32_Battery")]
        struct B {
            #[serde(rename = "EstimatedChargeRemaining")]
            c: u32,
        }
        if let Some(b) = con.query::<B>().unwrap_or_default().first() {
            out.push_str(&format!("- Battery: {}%\n", b.c));
        }

        #[derive(Deserialize)]
        #[serde(rename = "Win32_StartupCommand")]
        struct S {
            #[serde(rename = "Name")]
            _name: Option<String>,
        }
        let startups = con.query::<S>().unwrap_or_default();
        out.push_str(&format!("- 시작 프로그램: {}개\n", startups.len()));
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
            out.push_str(&format!("- 최고 온도: {:.1}°C\n", maxc));
        } else {
            out.push_str("- 최고 온도: N/A (관리자 권한 필요)\n");
        }
    }

    out
}

fn event_log() -> String {
    let out = Command::new("wevtutil")
        .args([
            "qe",
            "System",
            "/q:*[System[(Level=1 or Level=2)]]",
            "/c:3",
            "/rd:true",
            "/f:text",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = velox_core::util::decode_console(&o.stdout);
            let body = s.trim();
            if body.is_empty() {
                "- 최근 시스템 심각/오류 이벤트: 없음\n".to_string()
            } else {
                format!(
                    "- 최근 시스템 심각/오류 이벤트(최대 3건):\n{}\n",
                    truncate_chars(body, 700)
                )
            }
        }
        _ => "- 이벤트 로그: 조회 실패\n".to_string(),
    }
}

fn build_snapshot() -> String {
    let mut s = String::new();
    s.push_str(&cpu_ram_disk());
    s.push_str(&wmi_bits());
    s.push_str(&crate::drivers::problem_summary());
    s.push('\n');
    s.push_str(&event_log());
    s
}

pub async fn run() {
    println!("=== APEX Doctor — 종합 진단 ===\n");
    println!("[1] 시스템 전체 스캔 (읽기 전용)...\n");
    let snap = build_snapshot();
    println!("{}", snap);

    println!("[2] AI 종합 진단...\n");
    let prompt = format!(
        "너는 PC 종합 진단 의사다. 아래 시스템 상태 전체를 보고 한국어로 간결하게 답하라:\n\
         1) 전반 상태 한 줄 요약\n\
         2) 병목/의심되는 문제\n\
         3) 개선·업그레이드 추천\n\
         설명 위주로, 위험한 조치는 권하지 말 것.\n\n\
         [PC 상태]\n{}",
        snap
    );
    match velox_core::ai::query_text_with("claude", &prompt).await {
        Some(t) => println!("{}", t.trim()),
        None => println!(
            "(AI 진단 건너뜀 — API 키 없음/호출 실패. 'velox chorus set claude <key>'로 키를 넣으면 종합 진단을 받습니다. 위 [1] 시스템 스캔은 키 없이도 동작합니다.)"
        ),
    }
}
