// APEX Velox — timeline  (성능 추세 기록·비교)
//
// "기준 점수"는 외부가 아니라 **너 자신의 과거 최고 기록**이다.
// bench 점수를 시간순으로 저장 → 개인 최고(=100%) 대비 지금이 몇 %인지 비교 →
// "언제부터 느려졌나 / 발열·노후화로 성능이 떨어졌나"를 데이터로 본다. (읽기 전용)

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

const FILE: &str = "velox_timeline.csv";

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fmt_ago(ts: u64) -> String {
    let d = now().saturating_sub(ts);
    if d < 60 {
        format!("{}s ago", d)
    } else if d < 3600 {
        format!("{}m ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86400)
    }
}

struct Entry {
    ts: u64,
    single: f64,
    multi: f64,
}

fn parse_line(l: &str) -> Option<Entry> {
    let p: Vec<&str> = l.split(',').collect();
    if p.len() < 3 {
        return None;
    }
    Some(Entry {
        ts: p[0].parse().ok()?,
        single: p[1].parse().ok()?,
        multi: p[2].parse().ok()?,
    })
}

fn load() -> Vec<Entry> {
    std::fs::read_to_string(velox_core::paths::resolve(FILE))
        .map(|s| s.lines().filter_map(parse_line).collect())
        .unwrap_or_default()
}

pub fn record() {
    println!("=== APEX Velox — timeline record ===\n");
    println!("빠른 벤치 측정 중...");
    let (single, multi) = crate::bench::quick_score();
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(velox_core::paths::resolve(FILE))
    {
        let _ = writeln!(f, "{},{:.2},{:.2}", now(), single, multi);
    }
    println!(
        "✓ 기록됨: single {:.1} GFLOPS · multi {:.1} GFLOPS",
        single, multi
    );
    println!("→ `velox timeline show` 로 추세 확인");
}

pub fn show() {
    println!("=== APEX Velox — timeline ===\n");
    let entries = load();
    if entries.is_empty() {
        println!("(기록 없음 — 먼저 `velox timeline record`)");
        return;
    }

    // 기준 점수 = 개인 최고 멀티 (= 100%)
    let best = entries.iter().map(|e| e.multi).fold(f64::MIN, f64::max);
    println!("기준 점수(개인 최고 멀티) = {:.1} GFLOPS = 100%\n", best);

    println!(
        "{:<10} {:>10} {:>10} {:>8}",
        "시점", "single", "multi", "vs최고"
    );
    println!("{}", "-".repeat(42));
    for e in &entries {
        let pct = e.multi / best * 100.0;
        println!(
            "{:<10} {:>10.1} {:>10.1} {:>7.0}%",
            fmt_ago(e.ts),
            e.single,
            e.multi,
            pct
        );
    }

    if let Some(last) = entries.last() {
        let pct = last.multi / best * 100.0;
        println!();
        if pct < 85.0 {
            println!(
                "⚠ 최신 성능이 개인 최고 대비 {:.0}% — 성능 하락 감지 (발열/백그라운드/노후화 의심)",
                pct
            );
        } else {
            println!("✓ 최신 성능 {:.0}% — 양호", pct);
        }
    }
}
