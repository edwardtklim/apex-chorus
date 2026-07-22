use serde::Deserialize;
use std::io::Write;
use std::time::Duration;
use wmi::{COMLibrary, WMIConnection};

const WARNING_CELSIUS: f32 = 85.0;

#[derive(Deserialize, Debug)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
pub struct ThermalZone {
    #[serde(rename = "CurrentTemperature")]
    pub current_temperature: u32,
    #[serde(rename = "InstanceName")]
    pub instance_name: String,
}

pub fn get_thermals(wmi: &WMIConnection) -> Vec<ThermalZone> {
    wmi.query().unwrap_or_default()
}

pub fn to_celsius(raw: u32) -> f32 {
    (raw as f32 / 10.0) - 273.15
}

fn connect() -> Option<WMIConnection> {
    let com = COMLibrary::new().ok()?;
    WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()
}

fn format_line(zones: &[ThermalZone]) -> String {
    if zones.is_empty() {
        return "No thermal sensors found — try running as Administrator".to_string();
    }
    zones
        .iter()
        .map(|z| {
            let c = to_celsius(z.current_temperature);
            let mark = if c >= WARNING_CELSIUS { " [HIGH]" } else { "" };
            format!("{}: {:.1}°C{}", z.instance_name, c, mark)
        })
        .collect::<Vec<_>>()
        .join("  |  ")
}

pub fn run_once() {
    let Some(wmi) = connect() else {
        println!("온도 센서 접근 불가 — 관리자 권한으로 실행해 보세요.");
        return;
    };
    let zones = get_thermals(&wmi);
    println!("{}", format_line(&zones));
}

pub fn run_watch(interval_ms: u64) {
    let Some(wmi) = connect() else {
        println!("온도 센서 접근 불가 — 관리자 권한으로 실행해 보세요.");
        return;
    };
    println!("Polling every {}ms — press Ctrl+C to stop\n", interval_ms);
    loop {
        let zones = get_thermals(&wmi);
        print!("\r{}", format_line(&zones));
        std::io::stdout().flush().ok();
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
}
