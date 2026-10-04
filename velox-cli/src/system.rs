//! `velox system` — 읽기 전용 시스템 관리 조회 (프로그램 5).
//!
//! 판단은 전부 [`velox_core::sysmanage`] 가 한다. 여기서는 표시만 한다.
//! 이 명령은 **아무것도 바꾸지 않는다** — 서비스를 시작·중지하지 않는다.

use velox_core::sysmanage::{self, Readout, ServiceState, SystemView};

/// 요약 화면 — 수집 시각, 확인할 항목, 디스크·네트워크, 서비스 개수.
pub fn status(json: bool) {
    let started = std::time::Instant::now();
    let v = sysmanage::collect();

    if json {
        match serde_json::to_string_pretty(&v) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        record(&v, started);
        return;
    }

    println!("=== APEX 시스템 상태 (읽기 전용) ===");
    println!("PC: {}", if v.host.is_empty() { "-" } else { &v.host });
    // 수집 시각을 제일 먼저 보여준다 — 오래된 값을 현재로 오해하지 않게.
    println!("수집 시각: {}\n", v.collected_at);

    let attention = v.attention();
    if attention.is_empty() {
        println!("확인할 항목: 없음\n");
    } else {
        println!("확인할 항목 {}개:", attention.len());
        for a in &attention {
            println!("  · {a}");
        }
        println!();
    }

    println!("--- 디스크 (고정 디스크만) ---");
    match &v.disks {
        Readout::Ok(disks) if disks.is_empty() => println!("  표시할 디스크 없음"),
        Readout::Ok(disks) => {
            for d in disks {
                println!(
                    "  {} {:<12} {:>7.1}GB 중 {:>7.1}GB 사용 가능  ({:.0}% 사용)",
                    d.drive, d.label, d.total_gb, d.free_gb, d.used_pct
                );
            }
        }
        Readout::Unavailable(why) => println!("  측정 불가 — {why}"),
    }

    println!("\n--- 네트워크 ---");
    match &v.network {
        Readout::Ok(nets) if nets.is_empty() => println!("  어댑터 없음"),
        Readout::Ok(nets) => {
            for n in nets {
                println!(
                    "  [{}] {} ({})",
                    if n.connected { "연결" } else { "  -  " },
                    n.name,
                    n.status
                );
            }
        }
        Readout::Unavailable(why) => println!("  측정 불가 — {why}"),
    }

    println!("\n--- 서비스 ---");
    match &v.services {
        Readout::Ok(svcs) => {
            let running = svcs
                .iter()
                .filter(|s| s.state == ServiceState::Running)
                .count();
            let auto_stopped = svcs.iter().filter(|s| s.auto_but_stopped).count();
            println!(
                "  전체 {} · 실행 중 {} · 자동인데 멈춤 {}",
                svcs.len(),
                running,
                auto_stopped
            );
            println!("  목록: velox system services");
        }
        Readout::Unavailable(why) => println!("  측정 불가 — {why}"),
    }

    println!("\n--- 시작 프로그램 ---");
    match &v.startup {
        Readout::Ok(items) => println!("  {}개 (목록: velox system startup)", items.len()),
        Readout::Unavailable(why) => println!("  측정 불가 — {why}"),
    }

    println!("\n이 명령은 읽기만 합니다. 서비스 시작·중지는 하지 않습니다.");
    record(&v, started);
}

/// 서비스 목록. `--stopped` 면 자동 시작인데 멈춘 것만.
pub fn services(only_attention: bool, json: bool) {
    let started = std::time::Instant::now();
    let v = sysmanage::collect();
    let svcs = match &v.services {
        Readout::Ok(s) => s.clone(),
        Readout::Unavailable(why) => {
            println!("✗ 서비스를 읽지 못했습니다 — {why}");
            record(&v, started);
            return;
        }
    };
    let shown: Vec<_> = svcs
        .into_iter()
        .filter(|s| !only_attention || s.auto_but_stopped)
        .collect();

    if json {
        match serde_json::to_string_pretty(&shown) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        record(&v, started);
        return;
    }

    println!("수집 시각: {}", v.collected_at);
    println!(
        "{} {}개\n",
        if only_attention {
            "자동 시작인데 멈춘 서비스"
        } else {
            "서비스"
        },
        shown.len()
    );
    for s in &shown {
        println!(
            "  {:<9} {:<8} {}",
            s.state.label(),
            s.start_mode,
            s.display_name
        );
    }
    if shown.is_empty() {
        println!("  (없음)");
    }
    record(&v, started);
}

/// 시작 프로그램 목록 — 사용자 홈 경로는 `~` 로 축약돼 있다.
pub fn startup(json: bool) {
    let started = std::time::Instant::now();
    let v = sysmanage::collect();
    match &v.startup {
        Readout::Ok(items) => {
            if json {
                match serde_json::to_string_pretty(items) {
                    Ok(t) => println!("{t}"),
                    Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
                }
            } else {
                println!("수집 시각: {}", v.collected_at);
                println!("시작 프로그램 {}개\n", items.len());
                for i in items {
                    println!("  {} [{}]", i.name, i.location);
                    println!("      {}", i.command);
                }
                if items.is_empty() {
                    println!("  (없음)");
                }
            }
        }
        Readout::Unavailable(why) => println!("✗ 시작 프로그램을 읽지 못했습니다 — {why}"),
    }
    record(&v, started);
}

/// 알파 지표 기록 — 못 읽은 항목이 있으면 완료가 아니라 실패로 센다.
fn record(v: &SystemView, started: std::time::Instant) {
    let all_ok = v.services.reason().is_none()
        && v.disks.reason().is_none()
        && v.network.reason().is_none()
        && v.startup.reason().is_none();
    velox_core::metrics::record_operation(
        "system_status",
        if all_ok {
            velox_core::metrics::OperationOutcome::Completed
        } else {
            velox_core::metrics::OperationOutcome::Failed
        },
        started.elapsed().as_millis() as u64,
    );
}
