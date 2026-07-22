// APEX Velox — dashboard  (실시간 터미널 대시보드 · TUI)
//
// 1초마다 화면을 갱신하며 CPU/RAM/GPU/온도/디스크/네트워크/전원/드라이버를 한 화면에 보여준다.
// 풀 GUI(Pulse) 이전의 "보이는" 단계. 이미 만든 watch/drivers/util 재사용.

use serde::Deserialize;
use std::io::Write;
use std::process::Command;
use std::time::Duration;
use wmi::{COMLibrary, WMIConnection};

fn bar(pct: f64) -> String {
    let seg = ((pct / 100.0) * 10.0).round() as usize;
    let seg = seg.min(10);
    format!("{}{}", "█".repeat(seg), "░".repeat(10 - seg))
}

fn power_plan() -> String {
    Command::new("powercfg")
        .arg("/getactivescheme")
        .output()
        .ok()
        .map(|o| {
            let s = velox_core::util::decode_console(&o.stdout);
            s.split('(')
                .nth(1)
                .and_then(|t| t.split(')').next())
                .unwrap_or("?")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "?".to_string())
}

fn temp() -> Option<f32> {
    #[derive(Deserialize)]
    #[serde(rename = "MSAcpi_ThermalZoneTemperature")]
    struct Tz {
        #[serde(rename = "CurrentTemperature")]
        t: u32,
    }
    let com = COMLibrary::new().ok()?;
    let wmi = WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()?;
    let temps: Vec<Tz> = wmi.query().unwrap_or_default();
    temps
        .iter()
        .map(|x| (x.t as f32 / 10.0) - 273.15)
        .fold(None, |a, c| Some(a.map_or(c, |m: f32| m.max(c))))
}

struct GpuShot {
    name: String,
    util: String,
    mem: String,
    temp: String,
}

fn gpu() -> Option<GpuShot> {
    let o = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&o.stdout);
    let p: Vec<String> = s
        .lines()
        .next()?
        .split(',')
        .map(|x| x.trim().to_string())
        .collect();
    if p.len() != 5 {
        return None;
    }
    Some(GpuShot {
        name: p[0].clone(),
        util: format!("{}%", p[1]),
        mem: format!("{}/{}MB", p[2], p[3]),
        temp: format!("{}°C", p[4]),
    })
}

pub async fn run() {
    let mut watcher = velox_core::watch::Watcher::new();
    let drivers = crate::drivers::problem_summary(); // 시작 시 1회 (변동 드묾)

    loop {
        let m = watcher.read(1);
        let tp = temp();
        let pw = power_plan();
        let g = gpu();

        print!("\x1b[2J\x1b[H"); // clear + cursor home
        println!("╭─ APEX Velox — Dashboard ──────────────────────────╮");
        println!("  CPU   {}  {:>3.0}%", bar(m.cpu as f64), m.cpu);
        println!("  RAM   {}  {:>3.0}%", bar(m.ram_pct), m.ram_pct);
        match &g {
            Some(g) => println!("  GPU   {} · {} · {} · {}", g.name, g.util, g.mem, g.temp),
            None => println!("  GPU   (nvidia-smi 없음)"),
        }
        match tp {
            Some(t) => println!("  Temp  {:.1}°C", t),
            None => println!("  Temp  -- (관리자 권한 필요)"),
        }
        print!("  Disk  ");
        for (mp, free) in &m.disks {
            print!("{} {:.0}%여유   ", mp, free);
        }
        println!();
        println!("  Net   ↓{:.1}  ↑{:.1} MB/s", m.rx_mbps, m.tx_mbps);
        println!("  Power {}", pw);
        println!(
            "  Drv   {}",
            drivers
                .trim_start_matches("- ")
                .trim_start_matches("드라이버: ")
        );
        println!("╰─────────────────── 1초 갱신 · Ctrl+C 종료 ────────╯");

        std::io::stdout().flush().ok();

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = tokio::signal::ctrl_c() => {
                print!("\x1b[2J\x1b[H");
                println!("대시보드 종료.");
                break;
            }
        }
    }
}
