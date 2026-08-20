//! `velox report` — 수리 전/후 리포트 (v0.20 Closed Alpha).
//!
//! 실제 수리 세션의 흐름:
//!
//! ```text
//! velox report capture --label before --bench --out before.json   (수리 전)
//! ... 재조립 / 드라이버 갱신 ...
//! velox report capture --label after --bench --out after.json     (수리 후)
//! velox report repair --before before.json --after after.json --out repair.html
//! ```
//!
//! 판정은 전부 `velox_core::report` 의 결정론적 규칙이 한다. AI 는 관여하지 않는다.

use std::path::Path;

use velox_core::report::{ReportMeta, SessionMeasurement};

/// 현재 상태를 측정해 파일로 저장한다.
pub fn capture(label: &str, out: &str, run_bench: bool) {
    println!("측정 중... ({label})");

    let snap = velox_core::snapshot::Snapshot::collect();
    let (single, multi) = if run_bench {
        println!("  CPU 벤치 실행 중 (몇 초 걸립니다)");
        let (s, m) = crate::bench::quick_score();
        (Some(s), Some(m))
    } else {
        (None, None)
    };

    let temp = snap.max_temp_c;
    if temp.is_none() {
        println!(
            "{}",
            velox_core::guidance::Problem::SensorUnsupported {
                what: "CPU 온도".into()
            }
            .guidance()
            .render_plain()
        );
    }

    let m = SessionMeasurement {
        label: label.to_string(),
        captured_at: velox_core::report::ReportMeta::new("", "").generated_at,
        snapshot: snap,
        cpu_single: single,
        cpu_multi: multi,
        // 지속 부하 유지율은 `bench thermal` 로 따로 측정한다 — 빠른 캡처에서는 재지 않는다.
        sustain_ratio: None,
        max_temp_c: temp,
    };

    match serde_json::to_string_pretty(&m) {
        Ok(json) => match std::fs::write(out, json) {
            Ok(_) => {
                println!("✓ 저장: {out}");
                if !run_bench {
                    println!("  (--bench 를 주면 CPU 점수도 함께 측정합니다)");
                }
            }
            Err(e) => eprintln!("✗ 저장 실패 ({out}): {e}"),
        },
        Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
    }
}

fn load(path: &str) -> Option<SessionMeasurement> {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("✗ {path} 를 읽을 수 없습니다: {e}");
                eprintln!("  다음 행동: velox report capture 로 만든 파일인지 확인하세요.");
                None
            }
        },
        Err(e) => {
            eprintln!("✗ {path} 를 열 수 없습니다: {e}");
            eprintln!("  다음 행동: 경로가 맞는지 확인하세요.");
            None
        }
    }
}

/// 두 측정을 비교해 리포트를 만든다.
pub fn repair(before: &str, after: &str, out: Option<&str>, machine: &str, note: &str, json: bool) {
    let (Some(b), Some(a)) = (load(before), load(after)) else {
        return;
    };

    let report = velox_core::report::build(ReportMeta::new(machine, note), b, a);

    if json {
        match report.to_json() {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("✗ JSON 직렬화 실패: {e}"),
        }
        return;
    }

    // 터미널 요약 — 파일로 내보내지 않아도 결과를 바로 볼 수 있게.
    let (pass, fail, unknown) = report.tally();
    println!("\n=== APEX 수리 리포트 ===");
    if !report.meta.machine.is_empty() {
        println!("PC: {}", report.meta.machine);
    }
    if !report.meta.work_note.is_empty() {
        println!("작업: {}", report.meta.work_note);
    }
    println!("개선 {pass} · 악화 {fail} · 측정 불가 {unknown}\n");

    for c in &report.caveats {
        println!("⚠ {c}");
    }
    if !report.caveats.is_empty() {
        println!();
    }

    for v in &report.verdicts {
        println!("[{}] {}", v.outcome.label(), v.name);
        println!("    측정: {}", v.measured);
        println!("    기준: {}", v.criterion);
    }

    if !report.diff.is_empty() {
        println!(
            "\n시스템 변화: 변경 {} · 추가 {} · 제거 {}",
            report.diff.changed.len(),
            report.diff.added.len(),
            report.diff.removed.len()
        );
    }

    println!(
        "\n(benchmark version {} — 같은 버전끼리만 비교할 수 있습니다)",
        report.meta.benchmark_version
    );

    if let Some(path) = out {
        let html = report.to_html();
        match std::fs::write(path, html) {
            Ok(_) => {
                println!("\n✓ HTML 리포트 저장: {path}");
                if Path::new(path).extension().and_then(|e| e.to_str()) != Some("html") {
                    println!("  (확장자를 .html 로 하면 브라우저에서 바로 열립니다)");
                }
            }
            Err(e) => eprintln!("✗ 저장 실패 ({path}): {e}"),
        }
    } else {
        println!("\n(--out repair.html 로 저장하면 공유할 수 있는 리포트가 만들어집니다)");
    }
}
