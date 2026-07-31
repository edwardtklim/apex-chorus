// APEX Velox — usage (v0.17)
//
// APEX가 호출한 AI 사용량을 로컬 원장에서 읽어 보여준다.
// **정직성:** 여기 나오는 비용은 APEX가 기록한 사용량 기준 *추정치*이며,
// 소비자 구독(Claude Pro / ChatGPT Plus / Gemini Advanced)의 잔액·청구서가 아니다.
// 단가를 모르면 숫자를 지어내지 않고 unknown으로 표시한다.

use velox_core::ledger::{self, Period, SessionRecord, UsageGroup};
use velox_core::pricing;

/// 모든 usage 화면 하단에 붙는 고정 고지 (표현 규칙).
fn disclaimer() {
    println!(
        "\n※ Estimated API cost · APEX-recorded usage only\n\
         ※ Not subscription billing or provider balance (Claude Pro / ChatGPT Plus / Gemini Advanced 아님)"
    );
}

fn pricing_note(est: &pricing::CostEstimate) {
    if est.pricing_unconfigured {
        println!(
            "※ 단가표 미설정 → 비용 unknown. `velox usage pricing set <model> --input <USD/1M> --output <USD/1M> --date YYYY-MM-DD`"
        );
        return;
    }
    println!(
        "※ Pricing v{} · updated: {}",
        est.pricing_version,
        if est.pricing_effective_date.is_empty() {
            "(미기재)"
        } else {
            &est.pricing_effective_date
        }
    );
    if est.pricing_stale {
        println!("※ 단가표가 오래됐습니다 — 콘솔에서 최신 단가를 확인해 갱신하세요.");
    }
    if est.calls_missing_price > 0 {
        println!(
            "※ 단가 없는 호출 {}건 (모델: {}) → 비용에서 제외됨",
            est.calls_missing_price,
            est.models_missing_price.join(", ")
        );
    }
    if est.calls_missing_usage > 0 {
        println!(
            "※ provider가 토큰 수를 주지 않은 호출 {}건 → usage unavailable, 비용에서 제외됨",
            est.calls_missing_usage
        );
    }
}

fn parse_period(s: &str) -> Period {
    Period::parse(s).unwrap_or(Period::Month)
}

fn print_group_table(title: &str, groups: &[UsageGroup]) {
    println!("\n[{title}]");
    if groups.is_empty() {
        println!("  (기록 없음)");
        return;
    }
    println!(
        "{:<22}{:>7}{:>8}{:>7}{:>7}{:>12}{:>12}",
        "", "호출", "성공", "실패", "거부", "입력토큰", "출력토큰"
    );
    for g in groups {
        println!(
            "{:<22}{:>7}{:>8}{:>7}{:>7}{:>12}{:>12}",
            g.key, g.calls, g.success, g.failed, g.policy_denied, g.input_tokens, g.output_tokens
        );
    }
}

/// `velox usage summary [--period day|week|month|all]`
pub fn summary(period: &str) {
    let period = parse_period(period);
    let ledger = ledger::load();
    let now = ledger::now_unix();
    let records = ledger::in_period(&ledger.records, period, now);
    let totals = ledger::totals(&records);
    let table = pricing::load();
    let est = pricing::estimate(&records, &table, now);

    println!("=== APEX Usage — {} ===\n", period.label());
    if !ledger.settings.enabled {
        println!("⚠ 기록이 꺼져 있습니다 (`velox usage recording on`으로 켜기)\n");
    }
    println!("Estimated API cost : {}", est.display());
    println!(
        "호출               : {} (성공 {} · 실패 {} · 취소 {} · 정책거부 {})",
        totals.calls, totals.success, totals.failed, totals.cancelled, totals.policy_denied
    );
    println!(
        "토큰               : 입력 {} · 출력 {}{}",
        totals.input_tokens,
        totals.output_tokens,
        if totals.cache_read_tokens > 0 {
            format!(" · 캐시읽기 {}", totals.cache_read_tokens)
        } else {
            String::new()
        }
    );
    if totals.calls_without_usage > 0 {
        println!(
            "                     ({}건은 usage unavailable)",
            totals.calls_without_usage
        );
    }
    print_group_table("Provider별", &ledger::by_provider(&records));
    print_group_table("기능별", &ledger::by_feature(&records));
    pricing_note(&est);
    disclaimer();
}

/// `velox usage providers [--period ...]`
pub fn providers(period: &str) {
    let period = parse_period(period);
    let ledger = ledger::load();
    let now = ledger::now_unix();
    let records = ledger::in_period(&ledger.records, period, now);
    println!("=== APEX Usage — Provider/모델 ({}) ===", period.label());
    print_group_table("Provider별", &ledger::by_provider(&records));
    print_group_table("모델별", &ledger::by_model(&records));
    let est = pricing::estimate(&records, &pricing::load(), now);
    pricing_note(&est);
    disclaimer();
}

/// `velox usage sessions [--limit N]`
pub fn sessions(limit: usize) {
    let ledger = ledger::load();
    let n = ledger.records.len();
    println!("=== APEX Usage — 최근 세션 (총 {n}건) ===\n");
    if n == 0 {
        println!("(기록 없음)");
        return;
    }
    println!(
        "{:<12}{:<18}{:<10}{:<22}{:>9}{:>10}",
        "날짜", "기능", "provider", "모델", "상태", "ms"
    );
    for r in ledger.records.iter().rev().take(limit) {
        println!(
            "{:<12}{:<18}{:<10}{:<22}{:>9}{:>10}",
            ledger::date_string(r.unix_ts),
            r.feature,
            r.provider,
            r.model,
            r.status.label(),
            r.duration_ms
        );
    }
    println!("\n※ 프롬프트·응답·Evidence·API 키는 저장되지 않습니다 (메타데이터만).");
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn to_csv(records: &[SessionRecord]) -> String {
    let mut out = String::from(
        "id,date,unix_ts,feature,provider,model,scope,status,duration_ms,input_tokens,output_tokens,cache_read_tokens\n",
    );
    for r in records {
        let (i, o, c) = r
            .usage
            .map(|u| {
                (
                    u.input.map(|v| v.to_string()).unwrap_or_default(),
                    u.output.map(|v| v.to_string()).unwrap_or_default(),
                    u.cache_read.map(|v| v.to_string()).unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{},{:?},{},{},{},{},{}\n",
            csv_escape(&r.id),
            ledger::date_string(r.unix_ts),
            r.unix_ts,
            csv_escape(&r.feature),
            csv_escape(&r.provider),
            csv_escape(&r.model),
            r.scope,
            r.status.label(),
            r.duration_ms,
            i,
            o,
            c
        ));
    }
    out
}

/// `velox usage export --format json|csv [--out FILE]`
pub fn export(format: &str, out: Option<&str>) {
    let ledger = ledger::load();
    let body = match format.trim().to_lowercase().as_str() {
        "csv" => to_csv(&ledger.records),
        "json" => serde_json::to_string_pretty(&ledger.records).unwrap_or_else(|_| "[]".into()),
        other => {
            println!("✗ 알 수 없는 형식: {other} (json / csv)");
            return;
        }
    };
    match out {
        Some(path) => match std::fs::write(path, &body) {
            Ok(()) => println!(
                "✓ {}건을 {}로 내보냈습니다 ({})",
                ledger.records.len(),
                path,
                format
            ),
            Err(e) => println!("✗ 파일 쓰기 실패: {e}"),
        },
        None => print!("{body}"),
    }
}

/// `velox usage clear`
pub fn clear() {
    let n = ledger::clear();
    println!("✓ 세션 기록 {n}건 삭제됨 (설정은 유지)");
}

/// `velox usage recording on|off`
pub fn recording(on: bool) {
    if ledger::set_enabled(on) {
        println!(
            "✓ 세션 기록 {}",
            if on {
                "켜짐"
            } else {
                "꺼짐 (새 호출은 기록되지 않습니다)"
            }
        );
    } else {
        println!("✗ 설정 저장 실패");
    }
}

/// `velox usage retention <days>`
pub fn retention(days: u32) {
    if ledger::set_retention_days(days) {
        if days == 0 {
            println!("✓ 보존 기간: 무기한 (최대 {}건 유지)", ledger::MAX_RECORDS);
        } else {
            println!("✓ 보존 기간: {days}일 (지난 기록은 정리됨)");
        }
    } else {
        println!("✗ 설정 저장 실패");
    }
}

/// `velox usage pricing show`
pub fn pricing_show() {
    let t = pricing::load();
    println!("=== APEX 단가표 ===\n");
    println!("version        : {}", t.version);
    println!(
        "updated        : {}",
        if t.effective_date.is_empty() {
            "(미기재)"
        } else {
            &t.effective_date
        }
    );
    println!("currency       : {}", t.currency);
    println!("source         : {}", t.source);
    if t.is_unconfigured() {
        println!("\n(단가 미설정 — 비용은 unknown으로 표시됩니다)");
        println!(
            "설정: velox usage pricing set <model> --input <USD/1M> --output <USD/1M> --date YYYY-MM-DD"
        );
        println!("      단가는 각 provider 콘솔의 공개 가격표에서 확인해 입력하세요.");
    } else {
        println!(
            "\n{:<28}{:>14}{:>14}{:>14}",
            "모델", "입력/1M", "출력/1M", "캐시/1M"
        );
        for (m, p) in &t.models {
            println!(
                "{:<28}{:>14.4}{:>14.4}{:>14}",
                m,
                p.input_per_mtok,
                p.output_per_mtok,
                p.cache_read_per_mtok
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "-".into())
            );
        }
        if t.is_stale(ledger::now_unix()) {
            println!("\n⚠ 단가표가 오래됐습니다 — 콘솔에서 최신 단가를 확인하세요.");
        }
    }
    disclaimer();
}

/// `velox usage pricing set <model> --input X --output Y [--cache Z] --date YYYY-MM-DD [--source URL]`
pub fn pricing_set(
    model: &str,
    input: f64,
    output: f64,
    cache: Option<f64>,
    date: &str,
    source: Option<&str>,
) {
    match pricing::set_price(model, input, output, cache, date, source) {
        Ok(()) => {
            println!("✓ {model} 단가 저장됨 (입력 {input}/1M · 출력 {output}/1M · 확인일 {date})");
            println!(
                "  ※ 이 값은 사용자가 입력한 공개 단가입니다 — APEX가 추정치를 만들지 않습니다."
            );
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox usage pricing remove <model>`
pub fn pricing_remove(model: &str) {
    if pricing::remove_price(model) {
        println!("✓ {model} 단가 삭제됨");
    } else {
        println!("✗ 저장 실패");
    }
}
