//! `velox metrics` — Closed Alpha 지표 표시 (v0.20).
//!
//! 엔진은 `velox_core::metrics`. 여기는 표시만 한다.
//!
//! 표본이 없는 비율은 숫자를 만들어내지 않고 "표본 없음"으로 적는다.

fn pct(v: Option<f64>) -> String {
    match v {
        Some(p) => format!("{p:.0}%"),
        None => "표본 없음".to_string(),
    }
}

pub fn summary(json: bool) {
    let s = velox_core::metrics::summary();

    if json {
        match serde_json::to_string_pretty(&s) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        return;
    }

    println!("=== APEX Closed Alpha 지표 ===");
    if !s.since.is_empty() {
        println!("수집 시작: {}", s.since);
    }
    println!("이 기록은 이 PC 안에만 있습니다 — 자동 전송되지 않습니다.\n");

    println!("[실행]");
    println!("  시작 횟수      {}", s.starts);
    println!("  비정상 종료    {}", s.crashes);
    println!("  실행 성공률    {}", pct(s.start_success_rate));

    println!("\n[작업]");
    println!("  기록된 작업    {}", s.operations);
    println!("  완료율        {}", pct(s.completion_rate));
    println!("  취소율        {}", pct(s.cancel_rate));
    match s.avg_duration_ms {
        Some(ms) => println!("  평균 소요시간  {:.1}초", ms as f64 / 1000.0),
        None => println!("  평균 소요시간  표본 없음"),
    }

    if !s.by_feature.is_empty() {
        println!("\n[기능별 — 완료 / 취소 / 실패]");
        for (f, c) in &s.by_feature {
            println!("  {f:<18} {} / {} / {}", c[0], c[1], c[2]);
        }
    }

    println!("\n[센서]");
    println!("  미지원률       {}", pct(s.sensor_unavailable_rate));
    if s.sensor_unavailable_rate.is_some() {
        println!("  (센서를 못 읽는 건 흔한 정상 상황입니다 — 고장이 아닙니다)");
    }

    println!("\n[AI 정책 거부]");
    if s.policy_denials.is_empty() {
        println!("  없음");
    } else {
        for (reason, n) in &s.policy_denials {
            let label = match reason.as_str() {
                "consent_missing" => "동의 안 함(정상 — deny-by-default)",
                "scope_exceeded" => "데이터 범위 초과",
                "tool_not_allowed" => "허용되지 않은 툴",
                "unknown_provider" => "알 수 없는 provider",
                "provider_call_failed" => "provider 호출 실패",
                _ => "기타",
            };
            println!("  {label:<34} {n}");
        }
    }

    println!("\n[잘못된 경고]");
    println!("  사용자가 표시한 횟수  {}", s.false_warnings);
    println!("  (틀린 경고를 봤다면: velox metrics false-warning)");

    println!("\n내보내려면: velox metrics export --out alpha.json");
}

pub fn export(out: Option<&str>) {
    let s = velox_core::metrics::summary();
    let json = match serde_json::to_string_pretty(&s) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("✗ 직렬화 실패: {e}");
            return;
        }
    };

    match out {
        Some(path) => match std::fs::write(path, &json) {
            Ok(_) => {
                println!("✓ 지표 내보냄: {path}");
                println!("  이 파일에는 프롬프트·API 키·파일 경로가 들어 있지 않습니다.");
                println!("  개수와 비율만 담깁니다 — 열어서 직접 확인하실 수 있습니다.");
            }
            Err(e) => eprintln!("✗ 저장 실패 ({path}): {e}"),
        },
        None => println!("{json}"),
    }
}

pub fn clear() {
    velox_core::metrics::clear();
    println!("✓ 지표를 삭제했습니다.");
}

pub fn false_warning() {
    velox_core::metrics::record_false_warning();
    println!("✓ 잘못된 경고로 기록했습니다.");
    println!("  어떤 경고였는지도 함께 알려주시면 고치는 데 큰 도움이 됩니다.");
}
