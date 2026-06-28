// APEX Velox — checkpoint (CLI 표시 계층). 엔진은 velox_core::checkpoint.

use velox_core::checkpoint::{self, Restore};

pub fn save() {
    println!("=== APEX Velox — checkpoint save ===\n");
    let (_, label) = checkpoint::active_plan();
    if checkpoint::save_silent() {
        println!("✓ 현재 정상 상태 저장됨: 전원 모드 = {}", label);
    } else {
        println!("✗ 저장 실패 (상태 읽기 불가).");
    }
}

pub fn list() {
    println!("=== APEX Velox — checkpoints ===\n");
    let lines = checkpoint::entries();
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

pub fn restore_latest() {
    println!("=== APEX Velox — checkpoint restore ===\n");
    match checkpoint::restore_latest() {
        Restore::Empty => println!("✗ 복원할 체크포인트 없음."),
        Restore::BadFormat => println!("✗ 알 수 없는 체크포인트 형식."),
        Restore::Done { label, ok } => {
            println!("마지막 정상 상태로 복원: 전원 모드 → {}", label);
            println!("{}", if ok { "✓ 복원 완료" } else { "✗ 복원 실패 (권한 문제일 수 있음)" });
        }
    }
}
