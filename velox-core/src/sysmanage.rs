//! velox-core::sysmanage — 읽기 전용 시스템 관리 조회 (프로그램 5).
//!
//! 마스터 플랜 프로그램 5는 "등록한 PC·서버의 상태를 보고 허용된 관리 작업을 수행"이다.
//! 이 모듈은 그 중 **보는 쪽만** 담당한다. 서비스를 시작·중지하지 않고, 설정을
//! 바꾸지 않는다. 제어는 별도 권한·승인 설계가 끝난 뒤의 작업이다.
//!
//! 설계에서 양보하지 않는 것:
//!
//! 1. **못 읽은 것은 못 읽었다고 말한다.** 권한이 없거나 WMI 가 실패하면 빈 목록이
//!    아니라 [`Readout::Unavailable`] 로 이유를 남긴다. 0 과 "모름"은 다르다.
//! 2. **수집 시각을 함께 준다.** 오래된 값을 현재 상태처럼 보여주지 않기 위해
//!    모든 조회에 [`SystemView::collected_at`] 이 붙는다.
//! 3. **표시는 호출자가 한다.** 이 모듈은 데이터만 돌려준다.

use serde::{Deserialize, Serialize};
use wmi::{COMLibrary, WMIConnection};

/// 조회 결과 — 값이 있거나, 왜 없는지가 있다. 둘 중 하나다.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "state", content = "value")]
pub enum Readout<T> {
    Ok(T),
    /// 읽지 못했다 — 사람이 읽을 이유. **실패가 아니라 "모른다"다.**
    Unavailable(String),
}

impl<T> Readout<T> {
    pub fn ok(&self) -> Option<&T> {
        match self {
            Readout::Ok(v) => Some(v),
            Readout::Unavailable(_) => None,
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Readout::Ok(_) => None,
            Readout::Unavailable(r) => Some(r),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Running,
    Stopped,
    Other,
}

impl ServiceState {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" => ServiceState::Running,
            "stopped" => ServiceState::Stopped,
            _ => ServiceState::Other,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            ServiceState::Running => "실행 중",
            ServiceState::Stopped => "중지",
            ServiceState::Other => "기타",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub display_name: String,
    pub state: ServiceState,
    /// "Auto" / "Manual" / "Disabled" 등 Windows 가 보고한 시작 모드.
    pub start_mode: String,
    /// 자동 시작인데 멈춰 있다 — 사람이 볼 만한 신호. 판정은 하지 않는다.
    pub auto_but_stopped: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct StartupItem {
    pub name: String,
    /// 실행 경로/명령. 사용자 홈 경로는 그대로 두지 않고 축약한다.
    pub command: String,
    pub location: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DiskVolume {
    pub drive: String,
    pub label: String,
    pub total_gb: f64,
    pub free_gb: f64,
    pub used_pct: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetAdapter {
    pub name: String,
    pub connected: bool,
    /// Windows 가 보고한 연결 상태 문자열(없으면 빈 문자열).
    pub status: String,
}

/// 한 번의 조회 결과 묶음.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SystemView {
    /// 이 값들을 **언제** 읽었는지. 화면은 이 시각을 반드시 함께 보여준다.
    pub collected_at: String,
    pub host: String,
    /// 관리자 권한으로 읽었는지 — 아니면 일부 항목이 비어 보일 수 있다.
    pub elevated: Readout<bool>,
    pub services: Readout<Vec<Service>>,
    pub startup: Readout<Vec<StartupItem>>,
    pub disks: Readout<Vec<DiskVolume>>,
    pub network: Readout<Vec<NetAdapter>>,
}

impl SystemView {
    /// 사람이 먼저 볼 요약 — "자동인데 멈춘 서비스", "공간 부족 디스크", "끊긴 어댑터".
    /// 좋고 나쁨을 단정하지 않고, 확인해 볼 항목만 모아 준다.
    pub fn attention(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(svcs) = self.services.ok() {
            let stopped: Vec<&Service> = svcs.iter().filter(|s| s.auto_but_stopped).collect();
            if !stopped.is_empty() {
                out.push(format!(
                    "자동 시작인데 멈춘 서비스 {}개: {}",
                    stopped.len(),
                    stopped
                        .iter()
                        .take(5)
                        .map(|s| s.display_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        if let Some(disks) = self.disks.ok() {
            for d in disks.iter().filter(|d| d.used_pct >= 90.0) {
                out.push(format!(
                    "{} 디스크 사용률 {:.0}% (여유 {:.1}GB)",
                    d.drive, d.used_pct, d.free_gb
                ));
            }
        }
        if let Some(net) = self.network.ok().filter(|n| !n.is_empty())
            && !net.iter().any(|a| a.connected)
        {
            out.push("연결된 네트워크 어댑터가 없습니다".to_string());
        }
        for (what, r) in [
            ("서비스", self.services.reason()),
            ("시작 프로그램", self.startup.reason()),
            ("디스크", self.disks.reason()),
            ("네트워크", self.network.reason()),
        ] {
            if let Some(reason) = r {
                out.push(format!("{what}: 측정 불가 — {reason}"));
            }
        }
        out
    }
}

// --- 수집 ---------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename = "Win32_Service")]
#[serde(rename_all = "PascalCase")]
struct WmiService {
    name: Option<String>,
    display_name: Option<String>,
    state: Option<String>,
    start_mode: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_StartupCommand")]
#[serde(rename_all = "PascalCase")]
struct WmiStartup {
    name: Option<String>,
    command: Option<String>,
    location: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_LogicalDisk")]
#[serde(rename_all = "PascalCase")]
struct WmiDisk {
    device_id: Option<String>,
    volume_name: Option<String>,
    size: Option<u64>,
    free_space: Option<u64>,
    drive_type: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename = "Win32_NetworkAdapter")]
#[serde(rename_all = "PascalCase")]
struct WmiAdapter {
    net_connection_id: Option<String>,
    net_connection_status: Option<u16>,
    net_enabled: Option<bool>,
}

/// 현재 PC 를 읽는다. 어느 항목도 실패로 전체를 무너뜨리지 않는다.
pub fn collect() -> SystemView {
    let collected_at = crate::util::now_rfc3339();
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();

    let Ok(com) = COMLibrary::new() else {
        let why = "COM 초기화 실패 — 이 환경에서는 시스템 조회를 할 수 없습니다";
        return SystemView {
            collected_at,
            host,
            elevated: Readout::Unavailable(why.into()),
            services: Readout::Unavailable(why.into()),
            startup: Readout::Unavailable(why.into()),
            disks: Readout::Unavailable(why.into()),
            network: Readout::Unavailable(why.into()),
        };
    };
    let wmi = match WMIConnection::new(com) {
        Ok(w) => w,
        Err(e) => {
            let why = format!("WMI 연결 실패: {e}");
            return SystemView {
                collected_at,
                host,
                elevated: Readout::Unavailable(why.clone()),
                services: Readout::Unavailable(why.clone()),
                startup: Readout::Unavailable(why.clone()),
                disks: Readout::Unavailable(why.clone()),
                network: Readout::Unavailable(why),
            };
        }
    };

    SystemView {
        collected_at,
        host,
        // 권한 자체를 묻는 API 는 쓰지 않는다 — 관리자 전용 쿼리의 성공 여부로 추정한다.
        elevated: Readout::Unavailable(
            "권한 상태는 직접 판정하지 않습니다 — 항목별 측정 불가 이유를 보세요".into(),
        ),
        services: collect_services(&wmi),
        startup: collect_startup(&wmi),
        disks: collect_disks(&wmi),
        network: collect_network(&wmi),
    }
}

fn collect_services(wmi: &WMIConnection) -> Readout<Vec<Service>> {
    match wmi
        .raw_query::<WmiService>("SELECT Name, DisplayName, State, StartMode FROM Win32_Service")
    {
        Ok(rows) => {
            let mut out: Vec<Service> = rows
                .into_iter()
                .map(|r| {
                    let state = ServiceState::parse(r.state.as_deref().unwrap_or(""));
                    let start_mode = r.start_mode.unwrap_or_default();
                    let auto = start_mode.eq_ignore_ascii_case("auto");
                    Service {
                        display_name: r
                            .display_name
                            .clone()
                            .or_else(|| r.name.clone())
                            .unwrap_or_default(),
                        name: r.name.unwrap_or_default(),
                        auto_but_stopped: auto && state == ServiceState::Stopped,
                        state,
                        start_mode,
                    }
                })
                .filter(|s| !s.name.is_empty())
                .collect();
            out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
            Readout::Ok(out)
        }
        Err(e) => Readout::Unavailable(format!("서비스 목록을 읽지 못했습니다: {e}")),
    }
}

fn collect_startup(wmi: &WMIConnection) -> Readout<Vec<StartupItem>> {
    match wmi.raw_query::<WmiStartup>("SELECT Name, Command, Location FROM Win32_StartupCommand") {
        Ok(rows) => Readout::Ok(
            rows.into_iter()
                .map(|r| StartupItem {
                    name: r.name.unwrap_or_default(),
                    // 사용자 경로가 그대로 Evidence·화면에 흐르지 않게 축약한다.
                    command: crate::project::redact_secrets(&shorten_home(
                        &r.command.unwrap_or_default(),
                    )),
                    location: r.location.unwrap_or_default(),
                })
                .filter(|s| !s.name.is_empty() || !s.command.is_empty())
                .collect(),
        ),
        Err(e) => Readout::Unavailable(format!("시작 프로그램을 읽지 못했습니다: {e}")),
    }
}

fn collect_disks(wmi: &WMIConnection) -> Readout<Vec<DiskVolume>> {
    match wmi.raw_query::<WmiDisk>(
        "SELECT DeviceID, VolumeName, Size, FreeSpace, DriveType FROM Win32_LogicalDisk",
    ) {
        Ok(rows) => Readout::Ok(
            rows.into_iter()
                // DriveType 3 = 고정 디스크. 네트워크·이동식은 상태 화면에서 제외한다.
                .filter(|r| r.drive_type == Some(3))
                .filter_map(|r| {
                    let total = r.size.unwrap_or(0);
                    if total == 0 {
                        return None;
                    }
                    let free = r.free_space.unwrap_or(0);
                    let gb = |b: u64| b as f64 / 1_073_741_824.0;
                    Some(DiskVolume {
                        drive: r.device_id.unwrap_or_default(),
                        label: r.volume_name.unwrap_or_default(),
                        total_gb: (gb(total) * 10.0).round() / 10.0,
                        free_gb: (gb(free) * 10.0).round() / 10.0,
                        used_pct: ((total - free.min(total)) as f64 / total as f64 * 1000.0)
                            .round()
                            / 10.0,
                    })
                })
                .collect(),
        ),
        Err(e) => Readout::Unavailable(format!("디스크 정보를 읽지 못했습니다: {e}")),
    }
}

fn collect_network(wmi: &WMIConnection) -> Readout<Vec<NetAdapter>> {
    match wmi.raw_query::<WmiAdapter>(
        "SELECT NetConnectionID, NetConnectionStatus, NetEnabled FROM Win32_NetworkAdapter",
    ) {
        Ok(rows) => Readout::Ok(
            rows.into_iter()
                .filter(|r| r.net_connection_id.is_some())
                .map(|r| NetAdapter {
                    name: r.net_connection_id.unwrap_or_default(),
                    connected: r.net_connection_status == Some(2) && r.net_enabled != Some(false),
                    status: connection_status_label(r.net_connection_status).to_string(),
                })
                .collect(),
        ),
        Err(e) => Readout::Unavailable(format!("네트워크 어댑터를 읽지 못했습니다: {e}")),
    }
}

/// Win32_NetworkAdapter.NetConnectionStatus 코드 → 사람이 읽는 말.
fn connection_status_label(code: Option<u16>) -> &'static str {
    match code {
        Some(0) => "연결 안 됨",
        Some(1) => "연결 중",
        Some(2) => "연결됨",
        Some(3) => "연결 끊는 중",
        Some(4) => "하드웨어 없음",
        Some(5) => "하드웨어 비활성",
        Some(6) => "하드웨어 오류",
        Some(7) => "매체 없음",
        Some(8) => "인증 중",
        Some(9) => "인증 성공",
        Some(10) => "인증 실패",
        Some(11) => "주소 잘못됨",
        Some(12) => "자격 증명 필요",
        Some(_) => "기타",
        None => "",
    }
}

/// `C:\Users\<이름>\...` → `~\...`. 사용자 이름이 화면·Evidence 로 새지 않게.
fn shorten_home(path: &str) -> String {
    let Ok(home) = std::env::var("USERPROFILE") else {
        return path.to_string();
    };
    if home.is_empty() {
        return path.to_string();
    }
    let lower = path.to_ascii_lowercase();
    let home_lower = home.to_ascii_lowercase();
    match lower.find(&home_lower) {
        Some(i) => {
            let mut out = path[..i].to_string();
            out.push('~');
            out.push_str(&path[i + home.len()..]);
            out
        }
        None => path.to_string(),
    }
}

// --- 변화 추적 ------------------------------------------------------------------
//
// "이 PC 왜 느려졌지?"에 답하려면 지금 상태만으로는 부족하다. 기준점을 저장해 두고
// 그때와 무엇이 달라졌는지를 본다. 좋고 나쁨은 판정하지 않는다 — 바뀐 것만 보여준다.

pub const BASELINE_FILE: &str = "velox_system_baseline.json";
/// 이보다 작은 여유 공간 변화는 일상적인 흔들림으로 보고 보고하지 않는다.
pub const DISK_NOISE_GB: f64 = 1.0;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ServiceChange {
    pub display_name: String,
    pub before: ServiceState,
    pub after: ServiceState,
    /// 자동 시작 서비스가 멈춘 경우 — 사람이 먼저 볼 만한 변화.
    pub auto_now_stopped: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct DiskChange {
    pub drive: String,
    pub free_before_gb: f64,
    pub free_after_gb: f64,
    /// 양수 = 여유 공간이 늘었다, 음수 = 줄었다.
    pub delta_gb: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetChange {
    pub name: String,
    pub was_connected: bool,
    pub now_connected: bool,
}

/// 기준점과 지금의 차이.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct SystemChanges {
    pub baseline_at: String,
    pub current_at: String,
    /// 기준점을 다른 PC 에서 만들었다 — 비교 자체가 의미 없을 수 있다.
    pub host_mismatch: bool,
    pub startup_added: Vec<StartupItem>,
    pub startup_removed: Vec<StartupItem>,
    pub services_added: Vec<String>,
    pub services_removed: Vec<String>,
    pub services_changed: Vec<ServiceChange>,
    pub disks: Vec<DiskChange>,
    pub network: Vec<NetChange>,
    /// 한쪽이라도 못 읽어서 **비교하지 못한** 항목과 이유. "변화 없음"과 다르다.
    pub not_compared: Vec<String>,
}

impl SystemChanges {
    /// 보고할 변화가 하나도 없는가. 비교하지 못한 항목이 있으면 "없다"고 말하지 않는다.
    pub fn is_empty(&self) -> bool {
        self.startup_added.is_empty()
            && self.startup_removed.is_empty()
            && self.services_added.is_empty()
            && self.services_removed.is_empty()
            && self.services_changed.is_empty()
            && self.disks.is_empty()
            && self.network.is_empty()
            && self.not_compared.is_empty()
    }
}

/// 두 값이 모두 읽혔을 때만 비교한다. 아니면 이유를 `not_compared` 에 남긴다.
fn both<'a, T>(
    what: &str,
    old: &'a Readout<T>,
    new: &'a Readout<T>,
    not_compared: &mut Vec<String>,
) -> Option<(&'a T, &'a T)> {
    match (old, new) {
        (Readout::Ok(a), Readout::Ok(b)) => Some((a, b)),
        (Readout::Unavailable(r), _) => {
            not_compared.push(format!("{what}: 기준점에서 읽지 못했음 — {r}"));
            None
        }
        (_, Readout::Unavailable(r)) => {
            not_compared.push(format!("{what}: 지금 읽지 못함 — {r}"));
            None
        }
    }
}

/// 기준점(`old`)과 지금(`new`)을 비교한다. 순수 함수다 — 디스크·WMI 를 건드리지 않는다.
pub fn diff(old: &SystemView, new: &SystemView) -> SystemChanges {
    let mut c = SystemChanges {
        baseline_at: old.collected_at.clone(),
        current_at: new.collected_at.clone(),
        host_mismatch: !old.host.is_empty()
            && !new.host.is_empty()
            && !old.host.eq_ignore_ascii_case(&new.host),
        ..Default::default()
    };

    if let Some((a, b)) = both(
        "시작 프로그램",
        &old.startup,
        &new.startup,
        &mut c.not_compared,
    ) {
        // 이름+명령이 같으면 같은 항목이다. 위치만 달라진 것은 변화로 보지 않는다.
        let key = |s: &StartupItem| (s.name.to_lowercase(), s.command.to_lowercase());
        c.startup_added = b
            .iter()
            .filter(|x| !a.iter().any(|y| key(y) == key(x)))
            .cloned()
            .collect();
        c.startup_removed = a
            .iter()
            .filter(|x| !b.iter().any(|y| key(y) == key(x)))
            .cloned()
            .collect();
    }

    if let Some((a, b)) = both("서비스", &old.services, &new.services, &mut c.not_compared) {
        for s in b {
            match a.iter().find(|x| x.name.eq_ignore_ascii_case(&s.name)) {
                None => c.services_added.push(s.display_name.clone()),
                Some(prev) if prev.state != s.state => c.services_changed.push(ServiceChange {
                    display_name: s.display_name.clone(),
                    before: prev.state,
                    after: s.state,
                    auto_now_stopped: s.auto_but_stopped,
                }),
                Some(_) => {}
            }
        }
        c.services_removed = a
            .iter()
            .filter(|x| !b.iter().any(|y| y.name.eq_ignore_ascii_case(&x.name)))
            .map(|x| x.display_name.clone())
            .collect();
        // 자동인데 멈춘 것을 맨 위로.
        c.services_changed
            .sort_by_key(|s| std::cmp::Reverse(s.auto_now_stopped));
    }

    if let Some((a, b)) = both("디스크", &old.disks, &new.disks, &mut c.not_compared) {
        for d in b {
            if let Some(prev) = a.iter().find(|x| x.drive.eq_ignore_ascii_case(&d.drive)) {
                let delta = d.free_gb - prev.free_gb;
                if delta.abs() >= DISK_NOISE_GB {
                    c.disks.push(DiskChange {
                        drive: d.drive.clone(),
                        free_before_gb: prev.free_gb,
                        free_after_gb: d.free_gb,
                        delta_gb: (delta * 10.0).round() / 10.0,
                    });
                }
            }
        }
    }

    if let Some((a, b)) = both("네트워크", &old.network, &new.network, &mut c.not_compared) {
        for n in b {
            if let Some(prev) = a.iter().find(|x| x.name == n.name)
                && prev.connected != n.connected
            {
                c.network.push(NetChange {
                    name: n.name.clone(),
                    was_connected: prev.connected,
                    now_connected: n.connected,
                });
            }
        }
    }
    c
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BaselineError {
    /// 저장된 기준점이 없다.
    Missing,
    /// 파일은 있는데 읽을 수 없다.
    Corrupt(String),
    Io(String),
}

impl std::fmt::Display for BaselineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BaselineError::Missing => write!(
                f,
                "저장된 기준점이 없습니다.\n  다음 행동: 상태가 정상일 때 `velox system save` 로 기준점을 먼저 만드세요."
            ),
            BaselineError::Corrupt(e) => write!(
                f,
                "기준점 파일을 읽을 수 없습니다: {e}\n  다음 행동: `velox system save` 로 새 기준점을 만드세요."
            ),
            BaselineError::Io(e) => write!(
                f,
                "기준점을 저장하지 못했습니다: {e}\n  다음 행동: 디스크 공간과 권한을 확인하세요."
            ),
        }
    }
}

fn baseline_path() -> std::path::PathBuf {
    crate::paths::resolve(BASELINE_FILE)
}

/// 지금 상태를 기준점으로 저장한다(원자적). 이전 기준점은 교체된다.
pub fn save_baseline(view: &SystemView) -> Result<(), BaselineError> {
    save_baseline_to(&baseline_path(), view)
}

fn save_baseline_to(p: &std::path::Path, view: &SystemView) -> Result<(), BaselineError> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| BaselineError::Io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(view).map_err(|e| BaselineError::Io(e.to_string()))?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| BaselineError::Io(e.to_string()))?;
    std::fs::rename(&tmp, p).map_err(|e| BaselineError::Io(e.to_string()))
}

pub fn load_baseline() -> Result<SystemView, BaselineError> {
    load_baseline_from(&baseline_path())
}

fn load_baseline_from(p: &std::path::Path) -> Result<SystemView, BaselineError> {
    match std::fs::read_to_string(p) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| BaselineError::Corrupt(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(BaselineError::Missing),
        Err(e) => Err(BaselineError::Io(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn startup(name: &str, cmd: &str) -> StartupItem {
        StartupItem {
            name: name.into(),
            command: cmd.into(),
            location: "HKCU".into(),
        }
    }

    fn disk(drive: &str, free: f64) -> DiskVolume {
        DiskVolume {
            drive: drive.into(),
            label: String::new(),
            total_gb: 500.0,
            free_gb: free,
            used_pct: (500.0 - free) / 500.0 * 100.0,
        }
    }

    fn net(name: &str, connected: bool) -> NetAdapter {
        NetAdapter {
            name: name.into(),
            connected,
            status: String::new(),
        }
    }

    #[test]
    fn identical_views_have_no_changes() {
        let v = view(
            Readout::Ok(vec![svc("A", ServiceState::Running, "Auto")]),
            Readout::Ok(vec![disk("C:", 100.0)]),
            Readout::Ok(vec![net("이더넷", true)]),
        );
        let c = diff(&v, &v);
        assert!(c.is_empty(), "{c:?}");
        assert!(!c.host_mismatch);
    }

    #[test]
    fn new_and_removed_startup_items_are_reported() {
        let mut old = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        old.startup = Readout::Ok(vec![startup("Keep", "a.exe"), startup("Gone", "g.exe")]);
        let mut new = old.clone();
        new.startup = Readout::Ok(vec![startup("Keep", "A.EXE"), startup("New", "n.exe")]);

        let c = diff(&old, &new);
        assert_eq!(c.startup_added.len(), 1);
        assert_eq!(c.startup_added[0].name, "New");
        assert_eq!(c.startup_removed.len(), 1);
        assert_eq!(c.startup_removed[0].name, "Gone");
    }

    #[test]
    fn service_state_changes_put_stopped_auto_services_first() {
        let old = view(
            Readout::Ok(vec![
                svc("Manual", ServiceState::Stopped, "Manual"),
                svc("Auto", ServiceState::Running, "Auto"),
                svc("Same", ServiceState::Running, "Auto"),
                svc("Removed", ServiceState::Running, "Auto"),
            ]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        let new = view(
            Readout::Ok(vec![
                svc("Manual", ServiceState::Running, "Manual"),
                svc("Auto", ServiceState::Stopped, "Auto"),
                svc("Same", ServiceState::Running, "Auto"),
                svc("Added", ServiceState::Running, "Auto"),
            ]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        let c = diff(&old, &new);
        assert_eq!(c.services_changed.len(), 2);
        assert_eq!(c.services_changed[0].display_name, "Auto");
        assert!(c.services_changed[0].auto_now_stopped);
        assert_eq!(c.services_added, ["Added"]);
        assert_eq!(c.services_removed, ["Removed"]);
    }

    #[test]
    fn small_disk_wobble_is_ignored_but_real_change_is_reported() {
        let old = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![disk("C:", 100.0), disk("D:", 300.0)]),
            Readout::Ok(vec![]),
        );
        let new = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![disk("C:", 99.6), disk("D:", 250.0)]),
            Readout::Ok(vec![]),
        );
        let c = diff(&old, &new);
        assert_eq!(c.disks.len(), 1, "0.4GB 는 흔들림이다");
        assert_eq!(c.disks[0].drive, "D:");
        assert_eq!(c.disks[0].delta_gb, -50.0);
    }

    #[test]
    fn network_connection_flips_are_reported() {
        let old = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![net("이더넷", true), net("Wi-Fi", false)]),
        );
        let new = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![net("이더넷", false), net("Wi-Fi", false)]),
        );
        let c = diff(&old, &new);
        assert_eq!(c.network.len(), 1);
        assert!(c.network[0].was_connected && !c.network[0].now_connected);
    }

    /// **못 읽은 것을 "변화 없음"으로 보고하면 안 된다.**
    #[test]
    fn unreadable_sections_are_not_compared_and_not_called_unchanged() {
        let old = view(
            Readout::Ok(vec![svc("A", ServiceState::Running, "Auto")]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        let new = view(
            Readout::Unavailable("권한 없음".into()),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        let c = diff(&old, &new);
        assert!(c.services_changed.is_empty());
        assert_eq!(c.not_compared.len(), 1);
        assert!(c.not_compared[0].contains("서비스"));
        assert!(c.not_compared[0].contains("권한 없음"));
        assert!(
            !c.is_empty(),
            "비교 못 한 항목이 있으면 '변화 없음'이 아니다"
        );
    }

    #[test]
    fn baseline_from_another_pc_is_flagged() {
        let old = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        let mut new = old.clone();
        new.host = "OTHER-PC".into();
        assert!(diff(&old, &new).host_mismatch);
    }

    #[test]
    fn baseline_roundtrip_and_error_states() {
        let dir = std::env::temp_dir().join(format!("velox-baseline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join(BASELINE_FILE);

        assert_eq!(load_baseline_from(&p), Err(BaselineError::Missing));

        let v = view(
            Readout::Ok(vec![svc("A", ServiceState::Running, "Auto")]),
            Readout::Unavailable("이유".into()),
            Readout::Ok(vec![net("이더넷", true)]),
        );
        save_baseline_to(&p, &v).unwrap();
        assert_eq!(
            load_baseline_from(&p).unwrap(),
            v,
            "못 읽은 이유까지 보존된다"
        );

        std::fs::write(&p, "{ broken").unwrap();
        assert!(matches!(
            load_baseline_from(&p),
            Err(BaselineError::Corrupt(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn view(
        services: Readout<Vec<Service>>,
        disks: Readout<Vec<DiskVolume>>,
        network: Readout<Vec<NetAdapter>>,
    ) -> SystemView {
        SystemView {
            collected_at: "2026-10-05T00:00:00Z".into(),
            host: "test".into(),
            elevated: Readout::Ok(false),
            services,
            startup: Readout::Ok(vec![]),
            disks,
            network,
        }
    }

    fn svc(name: &str, state: ServiceState, mode: &str) -> Service {
        Service {
            name: name.into(),
            display_name: name.into(),
            state,
            start_mode: mode.into(),
            auto_but_stopped: mode.eq_ignore_ascii_case("auto") && state == ServiceState::Stopped,
        }
    }

    #[test]
    fn service_state_parsing_is_case_insensitive() {
        assert_eq!(ServiceState::parse("Running"), ServiceState::Running);
        assert_eq!(ServiceState::parse("stopped"), ServiceState::Stopped);
        assert_eq!(ServiceState::parse("Start Pending"), ServiceState::Other);
        assert_eq!(ServiceState::parse(""), ServiceState::Other);
    }

    #[test]
    fn auto_services_that_are_stopped_are_flagged() {
        let v = view(
            Readout::Ok(vec![
                svc("A", ServiceState::Stopped, "Auto"),
                svc("B", ServiceState::Stopped, "Manual"),
                svc("C", ServiceState::Running, "Auto"),
            ]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        let a = v.attention();
        assert_eq!(a.len(), 1, "수동 중지와 정상 실행은 신호가 아니다: {a:?}");
        assert!(a[0].contains("1개"));
        assert!(a[0].contains('A'));
    }

    /// **못 읽은 것을 0 으로 보여주면 안 된다.** 이유가 화면에 올라가야 한다.
    #[test]
    fn unreadable_sections_report_why_instead_of_zero() {
        let v = view(
            Readout::Unavailable("권한 없음".into()),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        assert!(v.services.ok().is_none());
        let a = v.attention();
        assert!(
            a.iter()
                .any(|s| s.contains("서비스: 측정 불가 — 권한 없음")),
            "{a:?}"
        );
    }

    #[test]
    fn full_disk_is_surfaced_and_healthy_disk_is_not() {
        let d = |drive: &str, total: f64, free: f64| DiskVolume {
            drive: drive.into(),
            label: String::new(),
            total_gb: total,
            free_gb: free,
            used_pct: (total - free) / total * 100.0,
        };
        let v = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![d("C:", 100.0, 3.0), d("D:", 100.0, 50.0)]),
            Readout::Ok(vec![]),
        );
        let a = v.attention();
        assert_eq!(a.len(), 1);
        assert!(a[0].starts_with("C:"), "{a:?}");
    }

    #[test]
    fn disconnected_network_is_surfaced_only_when_adapters_exist() {
        let down = NetAdapter {
            name: "이더넷".into(),
            connected: false,
            status: "연결 안 됨".into(),
        };
        let v = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![down.clone()]),
        );
        assert_eq!(v.attention().len(), 1);

        // 어댑터가 하나도 없으면 "연결 없음"이라고 단정하지 않는다.
        let empty = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
        );
        assert!(empty.attention().is_empty());

        let up = NetAdapter {
            connected: true,
            ..down
        };
        let ok = view(
            Readout::Ok(vec![]),
            Readout::Ok(vec![]),
            Readout::Ok(vec![up]),
        );
        assert!(ok.attention().is_empty());
    }

    #[test]
    fn home_paths_are_shortened() {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        let p = format!("{home}\\AppData\\Local\\app.exe");
        let short = shorten_home(&p);
        assert!(short.starts_with('~'), "{short}");
        assert!(!short.contains(&home));
        assert_eq!(shorten_home("C:\\Windows\\x.exe"), "C:\\Windows\\x.exe");
    }

    #[test]
    fn connection_status_labels_cover_codes() {
        assert_eq!(connection_status_label(Some(2)), "연결됨");
        assert_eq!(connection_status_label(Some(7)), "매체 없음");
        assert_eq!(connection_status_label(Some(99)), "기타");
        assert_eq!(connection_status_label(None), "");
    }

    /// 조회는 어떤 환경에서도 패닉하지 않고 수집 시각을 남긴다.
    #[test]
    fn collect_never_panics_and_timestamps_the_readout() {
        let v = collect();
        assert!(v.collected_at.ends_with('Z'));
        // 못 읽었으면 이유가 있어야 한다 — 조용한 빈 목록은 금지.
        for reason in [v.services.reason(), v.disks.reason(), v.network.reason()]
            .into_iter()
            .flatten()
        {
            assert!(!reason.is_empty());
        }
    }
}
