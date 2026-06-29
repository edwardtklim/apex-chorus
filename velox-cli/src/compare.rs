// APEX Velox — compare (CLI 표시 계층). 엔진은 velox_core::snapshot::compare.
//
// 저장된 두 스냅샷(JSON)을 비교해 **구조 변화**만 보여준다 (순간값은 무시).

use velox_core::snapshot::{compare, Snapshot};

fn load(path: &str) -> Result<Snapshot, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("{} 읽기 실패: {}", path, e))?;
    serde_json::from_str(&text).map_err(|e| format!("{} JSON 파싱 실패: {}", path, e))
}

pub fn run(old_path: String, new_path: String) {
    println!("=== APEX Velox — compare ===");
    println!("이전: {}\n이후: {}\n", old_path, new_path);

    let (old, new) = match (load(&old_path), load(&new_path)) {
        (Ok(o), Ok(n)) => (o, n),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("✗ {}", e);
            return;
        }
    };

    let d = compare(&old, &new);
    if d.is_empty() {
        println!("✓ 구조적 변화 없음 (순간값 차이는 무시됨)");
        return;
    }

    if !d.changed.is_empty() {
        println!("[변경] {}개", d.changed.len());
        for c in &d.changed {
            println!("  {} : {} → {}", c.item, c.old, c.new);
        }
    }
    if !d.added.is_empty() {
        println!("\n[추가] {}개", d.added.len());
        for a in &d.added {
            println!("  + {}", a);
        }
    }
    if !d.removed.is_empty() {
        println!("\n[삭제] {}개", d.removed.len());
        for r in &d.removed {
            println!("  - {}", r);
        }
    }
}
