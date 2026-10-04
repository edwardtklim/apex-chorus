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

/// `velox system save` — 지금 상태를 기준점으로 저장한다. 이전 기준점은 교체된다.
pub fn save_baseline() {
    let v = sysmanage::collect();
    let unread: Vec<&str> = [
        ("서비스", v.services.reason()),
        ("시작 프로그램", v.startup.reason()),
        ("디스크", v.disks.reason()),
        ("네트워크", v.network.reason()),
    ]
    .into_iter()
    .filter_map(|(what, r)| r.map(|_| what))
    .collect();

    match sysmanage::save_baseline(&v) {
        Ok(()) => {
            println!("✓ 기준점을 저장했습니다 ({})", v.collected_at);
            if !unread.is_empty() {
                // 못 읽은 항목은 나중에 비교할 수 없다 — 지금 알려준다.
                println!(
                    "  ! 읽지 못한 항목은 나중에 비교되지 않습니다: {}",
                    unread.join(", ")
                );
            }
            println!("  나중에 `velox system changes` 로 무엇이 달라졌는지 볼 수 있습니다.");
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox system changes` — 기준점 이후 달라진 것. 좋고 나쁨은 판정하지 않는다.
pub fn changes(json: bool) {
    let old = match sysmanage::load_baseline() {
        Ok(v) => v,
        Err(e) => {
            println!("✗ {e}");
            return;
        }
    };
    let new = sysmanage::collect();
    let c = sysmanage::diff(&old, &new);

    if json {
        match serde_json::to_string_pretty(&c) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        return;
    }

    println!("=== 기준점 이후 달라진 것 (읽기 전용) ===");
    println!("기준점: {}", c.baseline_at);
    println!("지금  : {}\n", c.current_at);
    if c.host_mismatch {
        println!(
            "⚠ 기준점이 다른 PC 에서 만들어졌습니다({} → {}). 비교가 의미 없을 수 있습니다.\n",
            old.host, new.host
        );
    }

    if c.is_empty() {
        println!("달라진 것이 없습니다.");
        return;
    }

    if !c.startup_added.is_empty() {
        println!(
            "--- 새로 생긴 시작 프로그램 {}개 ---",
            c.startup_added.len()
        );
        for s in &c.startup_added {
            println!("  + {}", s.name);
            println!("      {}", s.command);
        }
        println!();
    }
    if !c.startup_removed.is_empty() {
        println!("--- 없어진 시작 프로그램 {}개 ---", c.startup_removed.len());
        for s in &c.startup_removed {
            println!("  - {}", s.name);
        }
        println!();
    }
    if !c.services_changed.is_empty() {
        println!("--- 상태가 바뀐 서비스 {}개 ---", c.services_changed.len());
        for s in &c.services_changed {
            println!(
                "  {} {} → {}  {}",
                if s.auto_now_stopped { "!" } else { " " },
                s.before.label(),
                s.after.label(),
                s.display_name
            );
        }
        if c.services_changed.iter().any(|s| s.auto_now_stopped) {
            println!("  (! = 자동 시작 서비스인데 지금 멈춰 있음)");
        }
        println!();
    }
    if !c.services_added.is_empty() {
        println!("--- 새로 생긴 서비스 {}개 ---", c.services_added.len());
        for s in &c.services_added {
            println!("  + {s}");
        }
        println!();
    }
    if !c.services_removed.is_empty() {
        println!("--- 없어진 서비스 {}개 ---", c.services_removed.len());
        for s in &c.services_removed {
            println!("  - {s}");
        }
        println!();
    }
    if !c.disks.is_empty() {
        println!("--- 디스크 여유 공간 ---");
        for d in &c.disks {
            println!(
                "  {} {:.1}GB → {:.1}GB ({:+.1}GB)",
                d.drive, d.free_before_gb, d.free_after_gb, d.delta_gb
            );
        }
        println!();
    }
    if !c.network.is_empty() {
        println!("--- 네트워크 ---");
        for n in &c.network {
            let label = |on: bool| if on { "연결" } else { "끊김" };
            println!(
                "  {} : {} → {}",
                n.name,
                label(n.was_connected),
                label(n.now_connected)
            );
        }
        println!();
    }
    if !c.not_compared.is_empty() {
        println!("--- 비교하지 못한 항목 (변화 없음이 아닙니다) ---");
        for n in &c.not_compared {
            println!("  ? {n}");
        }
        println!();
    }
    println!("바뀐 것만 보여줍니다. 좋고 나쁨은 판정하지 않습니다.");
}
