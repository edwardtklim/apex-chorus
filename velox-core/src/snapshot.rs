//! velox-core::snapshot — 시스템 상태 스냅샷(순수 데이터).
//!
//! 전원 모드·CPU 사용률·최고 온도를 한 번에 읽어 **직렬화 가능한 데이터**로 반환한다.
//! 원칙: 엔진은 데이터를 반환하고, 표시(사람용/JSON)는 호출자(CLI/GUI)가 한다.

use serde::{Deserialize, Serialize};
use std::process::Command;
use sysinfo::System;
use wmi::{COMLibrary, WMIConnection};

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
struct ThermalZone {
    #[serde(rename = "CurrentTemperature")]
    current_temperature: u32,
}

/// 시스템 상태 스냅샷. 순수 데이터 — 포맷/표시는 호출자 몫.
#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    pub plan_guid: String,
    pub plan_label: String,
    pub cpu_usage: f32,
    /// 최고 온도(°C). 센서 읽기 실패/권한 없으면 `None`(JSON에선 null).
    pub max_temp_c: Option<f32>,
}

impl Snapshot {
    /// 현재 시스템 상태를 읽어 스냅샷을 만든다. (powercfg + sysinfo + WMI)
    pub fn collect() -> Self {
        let (plan_guid, plan_label) = active_power_plan();
        Snapshot {
            plan_guid,
            plan_label,
            cpu_usage: cpu_usage(),
            max_temp_c: max_temp_c(),
        }
    }
}

/// 활성 전원 구성표 (GUID, 라벨). powercfg 출력 파싱(시스템 코드페이지 디코딩).
pub fn active_power_plan() -> (String, String) {
    if let Ok(out) = Command::new("powercfg").arg("/getactivescheme").output() {
        let s = crate::util::decode_console(&out.stdout);
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

/// 전역 CPU 사용률(%). 정확도 위해 두 번 샘플링한다.
pub fn cpu_usage() -> f32 {
    let mut sys = System::new();
    sys.refresh_cpu();
    std::thread::sleep(std::time::Duration::from_millis(200));
    sys.refresh_cpu();
    sys.global_cpu_info().cpu_usage()
}

/// 최고 온도(°C). WMI `MSAcpi_ThermalZoneTemperature` — 관리자 권한이 필요할 수 있다.
pub fn max_temp_c() -> Option<f32> {
    let com = COMLibrary::new().ok()?;
    let wmi = WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()?;
    let temps: Vec<ThermalZone> = wmi.query().unwrap_or_default();
    temps
        .iter()
        .map(|t| (t.current_temperature as f32 / 10.0) - 273.15)
        .fold(None, |acc, c| Some(acc.map_or(c, |m: f32| m.max(c))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_to_json_with_expected_keys() {
        let snap = Snapshot {
            plan_guid: "381b4222-f694-41f0-9685-ff5bb260df2e".to_string(),
            plan_label: "Balanced".to_string(),
            cpu_usage: 12.5,
            max_temp_c: Some(48.0),
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"plan_guid\""));
        assert!(json.contains("\"plan_label\""));
        assert!(json.contains("\"cpu_usage\""));
        assert!(json.contains("\"max_temp_c\""));
    }

    #[test]
    fn missing_temp_serializes_as_null() {
        let snap = Snapshot {
            plan_guid: String::new(),
            plan_label: "Unknown".to_string(),
            cpu_usage: 0.0,
            max_temp_c: None,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"max_temp_c\":null"), "got: {json}");
    }
}
