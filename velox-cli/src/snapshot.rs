// APEX Velox — snapshot (CLI 표시 계층). 엔진은 velox_core::snapshot.
//
// 시스템 상태를 한 번에 읽어 사람용 요약 / --json(엔진 데이터) / --out(파일 저장)으로 낸다.

use velox_core::snapshot::Snapshot;

pub fn run(json: bool, out: Option<String>) {
    let snap = Snapshot::collect();

    // --out <파일>: 전체 스냅샷을 JSON으로 저장 (나중에 `velox compare`로 비교).
    if let Some(path) = out {
        match serde_json::to_string_pretty(&snap) {
            Ok(s) => match std::fs::write(&path, s) {
                Ok(_) => println!(
                    "✓ 스냅샷 저장됨: {} (드라이버 {}개 · GPU {}개)",
                    path,
                    snap.drivers.len(),
                    snap.gpus.len()
                ),
                Err(e) => eprintln!("✗ 저장 실패 ({}): {}", path, e),
            },
            Err(e) => eprintln!("✗ JSON 직렬화 실패: {}", e),
        }
        return;
    }

    // --json: 순수 JSON만 (jq 등으로 파이프 가능하게 헤더 없음).
    if json {
        match serde_json::to_string_pretty(&snap) {
            Ok(s) => println!("{}", s),
            Err(e) => eprintln!("JSON 직렬화 실패: {}", e),
        }
        return;
    }

    // 사람용 요약 — 구조값(시스템/GPU) 위주, 전체 드라이버 목록은 --json/--out.
    println!("=== APEX Velox — snapshot ===\n");

    println!("[시스템]");
    println!("  CPU : {} ({}코어)", snap.system.cpu_model, snap.system.logical_cores);
    println!("  RAM : {} MB", snap.system.ram_total_mb);
    println!("  OS  : {} (kernel {})", snap.system.os, snap.system.kernel);

    println!("\n[상태] (순간값 — 비교엔 무의미)");
    println!("  전원 모드 : {} ({})", snap.plan_label, snap.plan_guid);
    println!("  CPU 사용률: {:.0}%", snap.cpu_usage);
    match snap.max_temp_c {
        Some(t) => println!("  최고 온도 : {:.1}°C", t),
        None => println!("  최고 온도 : N/A (센서 읽기 실패 — 관리자 권한 필요)"),
    }

    println!("\n[GPU 드라이버]");
    if snap.gpus.is_empty() {
        println!("  (없음)");
    } else {
        for g in &snap.gpus {
            println!("  {} : v{} ({})", g.name, g.driver_version, g.driver_date);
        }
    }

    println!("\n[드라이버] {}개 (전체 목록·버전은 --json / --out)", snap.drivers.len());
    println!("\n(저장: `velox snapshot --out before.json` · 기계용: `--json`)");
}
