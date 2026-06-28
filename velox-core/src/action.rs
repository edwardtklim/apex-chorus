//! velox-core::action — 안전·가역 시스템 동작 엔진.
//!
//! **화이트리스트된 동작만** 노출한다. 실제 실행은 여기서 하되, "언제/승인/검증/롤백"
//! 오케스트레이션은 호출자(CLI/GUI)가 한다.

use std::process::Command;

/// 전원 구성표 화이트리스트. AI는 이 key 중에서만 고를 수 있다. (GUID 하드코딩)
pub fn plan_by_key(key: &str) -> Option<(&'static str, &'static str)> {
    match key {
        "balanced" => Some(("Balanced", "381b4222-f694-41f0-9685-ff5bb260df2e")),
        "high_performance" => Some(("High performance", "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c")),
        "power_saver" => Some(("Power saver", "a1841308-3541-4fab-bc81-f71556f20b4a")),
        _ => None,
    }
}

/// 전원 구성표 적용 (가역). 성공 여부 반환.
pub fn apply_power_plan(guid: &str) -> bool {
    Command::new("powercfg")
        .args(["/setactive", guid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
