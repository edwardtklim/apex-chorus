// APEX Velox — checkpoint
//
// 정상 상태(전원 모드 등)를 저장해두고, 블루스크린이나 AI 오판 후
// "마지막 정상"으로 되돌릴 수 있게 한다. 모든 위험 동작 전에 자동 save 하는 게 목표.
//
// (스캐폴드: 현재 전원 구성표만 저장. 추후 드라이버 버전/설정 등으로 확장.)

use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "velox_checkpoints.txt";

fn active_plan() -> (String, String) {
    if let Ok(out) = Command::new("powercfg").arg("/getactivescheme").output() {
        let s = velox_core::util::decode_console(&out.stdout);
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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 현재 상태를 체크포인트로 저장. (internal 호출용 — 위험 동작 직전 자동 save)
pub fn save_silent() -> bool {
    let (guid, label) = active_plan();
    if guid.is_empty() {
        return false;
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(FILE) {
        let _ = writeln!(f, "{}|power_plan|{}|{}", now_secs(), guid, label);
        return true;
    }
    false
}

pub fn save() {
    println!("=== APEX Velox — checkpoint save ===\n");
    let (_, label) = active_plan();
    if save_silent() {
        println!("✓ 현재 정상 상태 저장됨: 전원 모드 = {}", label);
        println!("  파일: {}", FILE);
    } else {
        println!("✗ 저장 실패 (상태 읽기 불가).");
    }
}

fn read_lines() -> Vec<String> {
    std::fs::read_to_string(FILE)
        .map(|s| s.lines().map(|l| l.to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

pub fn list() {
    println!("=== APEX Velox — checkpoints ===\n");
    let lines = read_lines();
    if lines.is_empty() {
        println!("(저장된 체크포인트 없음 — `velox checkpoint save`)");
        return;
    }
    for (i, l) in lines.iter().enumerate() {
        let parts: Vec<&str> = l.split('|').collect();
        if parts.len() >= 4 {
            println!("#{:<2} [{}] {} = {}", i + 1, parts[0], parts[1], parts[3]);
        }
    }
}

/// 가장 최근 체크포인트로 복원.
pub fn restore_latest() {
    println!("=== APEX Velox — checkpoint restore ===\n");
    let lines = read_lines();
    let last = match lines.last() {
        Some(l) => l,
        None => {
            println!("✗ 복원할 체크포인트 없음.");
            return;
        }
    };
    let parts: Vec<&str> = last.split('|').collect();
    if parts.len() < 4 || parts[1] != "power_plan" {
        println!("✗ 알 수 없는 체크포인트 형식.");
        return;
    }
    let (guid, label) = (parts[2], parts[3]);
    println!("마지막 정상 상태로 복원: 전원 모드 → {}", label);
    let ok = Command::new("powercfg")
        .args(["/setactive", guid])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    println!("{}", if ok { "✓ 복원 완료" } else { "✗ 복원 실패 (권한 문제일 수 있음)" });
}
