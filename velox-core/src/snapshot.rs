//! velox-core::snapshot — 시스템 상태 스냅샷(순수 데이터).
//!
//! 두 종류의 데이터를 담는다:
//! - **순간값**(plan/cpu_usage/max_temp_c): 그 시점의 값 — 두 스냅샷 비교엔 무의미(노이즈).
//! - **구조값**(system/gpus/drivers): 세션 사이 안정 — `velox compare`의 핵심.
//!
//! 원칙: 엔진은 데이터를 반환하고, 표시(사람용/JSON)는 호출자(CLI/HTTP/GUI)가 한다.

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

#[derive(Deserialize)]
#[serde(rename = "Win32_PnPSignedDriver")]
struct SignedDriver {
    #[serde(rename = "DeviceName")]
    device_name: Option<String>,
    #[serde(rename = "DriverVersion")]
    driver_version: Option<String>,
    #[serde(rename = "DriverDate")]
    driver_date: Option<String>,
    #[serde(rename = "DeviceClass")]
    device_class: Option<String>,
}

/// 하드웨어/OS 정보 (구조값 — 세션 사이 안정).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SystemInfo {
    pub cpu_model: String,
    pub logical_cores: usize,
    pub ram_total_mb: u64,
    pub os: String,
    pub kernel: String,
}

/// 디스플레이(GPU) 드라이버 정보.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub driver_version: String,
    pub driver_date: String,
}

/// 장치 드라이버 (이름, 버전).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DriverInfo {
    pub device: String,
    pub version: String,
}

/// 시스템 상태 스냅샷. 순수 데이터 — 포맷/표시는 호출자 몫.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snapshot {
    // --- 순간값 (비교엔 무의미, 참고용) ---
    pub plan_guid: String,
    pub plan_label: String,
    pub cpu_usage: f32,
    /// 최고 온도(°C). 센서 읽기 실패/권한 없으면 `None`(JSON에선 null).
    pub max_temp_c: Option<f32>,
    // --- 구조값 (compare 핵심) ---
    pub system: SystemInfo,
    pub gpus: Vec<GpuInfo>,
    pub drivers: Vec<DriverInfo>,
}

impl Snapshot {
    /// 현재 시스템 상태를 읽어 스냅샷을 만든다. (powercfg + sysinfo + WMI)
    pub fn collect() -> Self {
        let (plan_guid, plan_label) = active_power_plan();
        let signed = signed_drivers();
        Snapshot {
            plan_guid,
            plan_label,
            cpu_usage: cpu_usage(),
            max_temp_c: max_temp_c(),
            system: system_info(),
            gpus: gpus_from(&signed),
            drivers: drivers_from(&signed),
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

/// 하드웨어/OS 정보 (CPU 모델·코어·RAM·OS).
pub fn system_info() -> SystemInfo {
    let mut sys = System::new();
    sys.refresh_cpu();
    sys.refresh_memory();
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    SystemInfo {
        cpu_model,
        logical_cores: sys.cpus().len(),
        ram_total_mb: sys.total_memory() / 1024 / 1024,
        os: System::long_os_version().unwrap_or_else(|| "Unknown".into()),
        kernel: System::kernel_version().unwrap_or_else(|| "Unknown".into()),
    }
}

/// 서명된 드라이버 전체를 WMI로 한 번 읽는다(GPU·드라이버 목록의 공통 소스).
fn signed_drivers() -> Vec<SignedDriver> {
    let Ok(com) = COMLibrary::new() else {
        return vec![];
    };
    let Ok(wmi) = WMIConnection::new(com) else {
        return vec![];
    };
    wmi.raw_query(
        "SELECT DeviceName, DriverVersion, DriverDate, DeviceClass FROM Win32_PnPSignedDriver",
    )
    .unwrap_or_default()
}

fn gpus_from(signed: &[SignedDriver]) -> Vec<GpuInfo> {
    signed
        .iter()
        .filter(|d| d.device_class.as_deref() == Some("DISPLAY"))
        .filter_map(|d| {
            let name = d.device_name.clone()?;
            Some(GpuInfo {
                name,
                driver_version: d.driver_version.clone().unwrap_or_else(|| "?".into()),
                driver_date: d
                    .driver_date
                    .clone()
                    .map(|s| s.chars().take(8).collect())
                    .unwrap_or_else(|| "?".into()),
            })
        })
        .collect()
}

fn drivers_from(signed: &[SignedDriver]) -> Vec<DriverInfo> {
    let mut out: Vec<DriverInfo> = signed
        .iter()
        .filter_map(|d| {
            let device = d.device_name.clone()?;
            let version = d.driver_version.clone().filter(|v| !v.is_empty())?;
            Some(DriverInfo { device, version })
        })
        .collect();
    // 비교가 안정적이도록 이름순 정렬 + 중복 제거.
    out.sort_by(|a, b| a.device.cmp(&b.device));
    out.dedup_by(|a, b| a.device == b.device && a.version == b.version);
    out
}

// ---------------- compare: 두 스냅샷의 구조 변화 ----------------

/// 한 항목의 변화 (이전 → 이후).
#[derive(Serialize, Clone, Debug)]
pub struct Change {
    pub item: String,
    pub old: String,
    pub new: String,
}

/// 두 스냅샷의 차이. **구조값만** 본다 — 순간값(cpu_usage/온도)은 노이즈라 무시.
#[derive(Serialize, Default, Debug)]
pub struct SnapshotDiff {
    pub changed: Vec<Change>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl SnapshotDiff {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }
}

fn push_if_changed(d: &mut SnapshotDiff, item: &str, old: &str, new: &str) {
    if old != new {
        d.changed.push(Change {
            item: item.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        });
    }
}

/// 이름→버전 목록을 비교해 변경/추가/삭제를 누적.
fn diff_versioned(
    label: &str,
    old: &[(String, String)],
    new: &[(String, String)],
    d: &mut SnapshotDiff,
) {
    use std::collections::BTreeMap;
    let om: BTreeMap<&str, &str> = old.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let nm: BTreeMap<&str, &str> = new.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    for (k, nv) in &nm {
        match om.get(k) {
            Some(ov) if ov != nv => d.changed.push(Change {
                item: format!("{} · {}", label, k),
                old: ov.to_string(),
                new: nv.to_string(),
            }),
            None => d.added.push(format!("{} · {} (v{})", label, k, nv)),
            _ => {}
        }
    }
    for k in om.keys() {
        if !nm.contains_key(k) {
            d.removed.push(format!("{} · {}", label, k));
        }
    }
}

/// 두 스냅샷의 **구조적** 차이를 계산한다(순간값 무시).
pub fn compare(old: &Snapshot, new: &Snapshot) -> SnapshotDiff {
    let mut d = SnapshotDiff::default();

    push_if_changed(&mut d, "CPU", &old.system.cpu_model, &new.system.cpu_model);
    push_if_changed(
        &mut d,
        "RAM(MB)",
        &old.system.ram_total_mb.to_string(),
        &new.system.ram_total_mb.to_string(),
    );
    push_if_changed(&mut d, "OS", &old.system.os, &new.system.os);
    push_if_changed(&mut d, "커널", &old.system.kernel, &new.system.kernel);
    push_if_changed(&mut d, "전원 모드", &old.plan_label, &new.plan_label);

    let og: Vec<_> = old.gpus.iter().map(|g| (g.name.clone(), g.driver_version.clone())).collect();
    let ng: Vec<_> = new.gpus.iter().map(|g| (g.name.clone(), g.driver_version.clone())).collect();
    diff_versioned("GPU", &og, &ng, &mut d);

    let od: Vec<_> = old.drivers.iter().map(|x| (x.device.clone(), x.version.clone())).collect();
    let nd: Vec<_> = new.drivers.iter().map(|x| (x.device.clone(), x.version.clone())).collect();
    diff_versioned("드라이버", &od, &nd, &mut d);

    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Snapshot {
        Snapshot {
            plan_guid: "381b4222-f694-41f0-9685-ff5bb260df2e".to_string(),
            plan_label: "Balanced".to_string(),
            cpu_usage: 12.5,
            max_temp_c: Some(48.0),
            system: SystemInfo {
                cpu_model: "Test CPU".into(),
                logical_cores: 8,
                ram_total_mb: 16384,
                os: "Windows 11".into(),
                kernel: "10.0.26200".into(),
            },
            gpus: vec![GpuInfo {
                name: "Test GPU".into(),
                driver_version: "31.0.15.5152".into(),
                driver_date: "20240101".into(),
            }],
            drivers: vec![DriverInfo {
                device: "Test Device".into(),
                version: "1.2.3".into(),
            }],
        }
    }

    #[test]
    fn serializes_to_json_with_expected_keys() {
        let json = serde_json::to_string(&sample()).unwrap();
        for key in ["plan_guid", "cpu_usage", "max_temp_c", "system", "gpus", "drivers"] {
            assert!(json.contains(&format!("\"{key}\"")), "missing {key} in {json}");
        }
    }

    #[test]
    fn missing_temp_serializes_as_null() {
        let mut snap = sample();
        snap.max_temp_c = None;
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"max_temp_c\":null"), "got: {json}");
    }

    #[test]
    fn drivers_are_sorted_and_deduped() {
        let signed = vec![
            SignedDriver {
                device_name: Some("Zeta".into()),
                driver_version: Some("2.0".into()),
                driver_date: None,
                device_class: None,
            },
            SignedDriver {
                device_name: Some("Alpha".into()),
                driver_version: Some("1.0".into()),
                driver_date: None,
                device_class: None,
            },
            SignedDriver {
                device_name: Some("Alpha".into()),
                driver_version: Some("1.0".into()),
                driver_date: None,
                device_class: None,
            },
            SignedDriver {
                device_name: Some("NoVersion".into()),
                driver_version: Some("".into()),
                driver_date: None,
                device_class: None,
            },
        ];
        let drivers = drivers_from(&signed);
        // 빈 버전 제외, 중복 제거, 이름순
        assert_eq!(drivers.len(), 2);
        assert_eq!(drivers[0].device, "Alpha");
        assert_eq!(drivers[1].device, "Zeta");
    }

    #[test]
    fn compare_ignores_noise_catches_structural() {
        let a = sample();
        let mut b = sample();

        // 순간값만 다르면 → 변화 없음 (노이즈 무시)
        b.cpu_usage = 99.0;
        b.max_temp_c = Some(90.0);
        assert!(compare(&a, &b).is_empty(), "순간값 차이는 무시해야 함");

        // GPU 드라이버 버전 변경 → 감지
        b.gpus[0].driver_version = "31.0.99.9999".into();
        let d = compare(&a, &b);
        assert_eq!(d.changed.len(), 1);
        assert!(d.changed[0].item.contains("GPU"));

        // 드라이버 추가/삭제
        let mut c = sample();
        c.drivers.clear();
        let d2 = compare(&c, &a);
        assert!(!d2.added.is_empty(), "a에만 있는 드라이버 = 추가로 감지");
    }
}
