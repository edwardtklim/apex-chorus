// APEX Velox — snapshot (CLI 표시 계층). 엔진은 velox_core::snapshot.
//
// 시스템 상태를 한 번에 읽어 사람용 표 또는 --json(엔진 데이터 그대로 직렬화)으로 출력한다.

use velox_core::snapshot::Snapshot;

pub fn run(json: bool) {
    let snap = Snapshot::collect();

    // --json: 순수 JSON만 출력 (jq 등으로 파이프 가능하게 헤더 없음).
    if json {
        match serde_json::to_string_pretty(&snap) {
            Ok(s) => println!("{}", s),
            Err(e) => eprintln!("JSON 직렬화 실패: {}", e),
        }
        return;
    }

    println!("=== APEX Velox — snapshot ===\n");
    println!("전원 모드 : {} ({})", snap.plan_label, snap.plan_guid);
    println!("CPU 사용률: {:.0}%", snap.cpu_usage);
    match snap.max_temp_c {
        Some(t) => println!("최고 온도 : {:.1}°C", t),
        None => println!("최고 온도 : N/A (센서 읽기 실패 — 관리자 권한 필요)"),
    }
    println!("\n(기계용 출력은 `velox snapshot --json`)");
}
