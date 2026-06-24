use wmi::{COMLibrary, WMIConnection};
use serde::Deserialize;
use std::io::Write;
use std::time::Duration;

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

fn connect() -> WMIConnection {
    let com = COMLibrary::new().expect("COM init failed");
    WMIConnection::with_namespace_path("ROOT\\WMI", com)
        .expect("WMI ROOT\\WMI connection failed (try running as Administrator)")
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
    let wmi = connect();
    let zones = get_thermals(&wmi);
    println!("{}", format_line(&zones));
}

pub fn run_watch(interval_ms: u64) {
    let wmi = connect();
    println!("Polling every {}ms — press Ctrl+C to stop\n", interval_ms);
    loop {
        let zones = get_thermals(&wmi);
        print!("\r{}", format_line(&zones));
        std::io::stdout().flush().ok();
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
}