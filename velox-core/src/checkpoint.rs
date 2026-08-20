//! velox-core::checkpoint — 정상 상태 저장/복원 엔진.
//!
//! 위험 동작 전 자동 save, 블루스크린/AI 오판 후 마지막 정상으로 복원.
//! **데이터/결과를 반환만 한다 — 표시는 호출자가.**

use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "velox_checkpoints.txt";

/// 현재 활성 전원 구성표 (guid, label).
pub fn active_plan() -> (String, String) {
    if let Ok(out) = Command::new("powercfg").arg("/getactivescheme").output() {
        let s = crate::util::decode_console(&out.stdout);
        let guid = s
            .split("GUID:")
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 현재 상태를 체크포인트로 저장. 위험 동작 직전 자동 호출용.
pub fn save_silent() -> bool {
    let (guid, label) = active_plan();
    if guid.is_empty() {
        return false;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(crate::paths::resolve(FILE))
    {
        let _ = writeln!(f, "{}|power_plan|{}|{}", now_secs(), guid, label);
        return true;
    }
    false
}

/// 저장된 체크포인트 줄 목록.
pub fn entries() -> Vec<String> {
    std::fs::read_to_string(crate::paths::resolve(FILE))
        .map(|s| {
            s.lines()
                .map(|l| l.to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 복원 결과.
pub enum Restore {
    Empty,
    BadFormat,
    Done { label: String, ok: bool },
}

/// 가장 최근 체크포인트로 복원 (powercfg 적용 + 결과 반환).
pub fn restore_latest() -> Restore {
    let lines = entries();
    let last = match lines.last() {
        Some(l) => l,
        None => return Restore::Empty,
    };
    let parts: Vec<&str> = last.split('|').collect();
    if parts.len() < 4 || parts[1] != "power_plan" {
        return Restore::BadFormat;
    }
    let (guid, label) = (parts[2].to_string(), parts[3].to_string());
    let ok = Command::new("powercfg")
        .args(["/setactive", &guid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    Restore::Done { label, ok }
}
