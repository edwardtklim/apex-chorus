use reqwest::Client;
use velox_core::ai::{env_var_for, load_providers, query_text_with, save_providers, ProviderConfig, PROVIDERS_FILE};
use serde_json::json;
use std::env;
use sysinfo::System;


fn get_system_context() -> String {
    use serde::Deserialize;
    use wmi::{COMLibrary, WMIConnection};

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_Battery")]
    struct Battery {
        #[serde(rename = "EstimatedChargeRemaining")]
        charge: u32,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_VideoController")]
    struct GPU {
        #[serde(rename = "Name")]
        name: String,
    }

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_usage: f32 = sys.cpus().iter().map(|c| c.cpu_usage()).sum::<f32>()
        / sys.cpus().len() as f32;

    let total_mem = sys.total_memory() / 1024 / 1024;
    let used_mem = sys.used_memory() / 1024 / 1024;
    let mem_percent = (used_mem as f32 / total_mem as f32) * 100.0;

    let mut context = format!(
        "[System Context]\nCPU Usage: {:.1}%\nMemory: {}MB / {}MB ({:.1}%)\n",
        cpu_usage, used_mem, total_mem, mem_percent
    );

    if let Ok(com) = COMLibrary::new() {
        if let Ok(wmi_con) = WMIConnection::new(com) {
            let gpus: Vec<GPU> = wmi_con.query().unwrap_or_default();
            for gpu in &gpus {
                context.push_str(&format!("GPU: {}\n", gpu.name));
            }
            let batteries: Vec<Battery> = wmi_con.query().unwrap_or_default();
            if !batteries.is_empty() {
                context.push_str(&format!("Battery: {}%\n", batteries[0].charge));
            }
        }
    }

    context
}

pub async fn ask(prompt: &str, model: &str, no_context: bool) {
    let full_prompt = if no_context {
        prompt.to_string()
    } else {
        let context = get_system_context();
        format!("{}\nUser question: {}", context, prompt)
    };

    // 커스텀 provider면 query_text_with 로 직접 처리 (fallback은 내장 전용)
    if load_providers().iter().any(|p| p.name == model) {
        println!("Asking {} (custom)...\n", model);
        match query_text_with(model, &full_prompt).await {
            Some(t) => println!("{}", t),
            None => println!("✗ {} 응답 실패 — URL/모델/키 확인", model),
        }
        return;
    }

    let fallback_order: Vec<&str> = match model {
        "gpt"    => vec!["gpt",    "claude", "grok", "gemini"],
        "gemini" => vec!["gemini", "claude", "gpt",  "grok"],
        "grok"   => vec!["grok",   "claude", "gpt",  "gemini"],
        _        => vec!["claude", "gpt",    "grok", "gemini"],
    };

    for (i, m) in fallback_order.iter().enumerate() {
        if i > 0 {
            println!("⚠ Falling back to: {}\n", m);
        }
        let success = match *m {
            "gpt"    => try_gpt(&full_prompt).await,
            "gemini" => try_gemini(&full_prompt).await,
            "grok"   => try_grok(&full_prompt).await,
            _        => try_claude(&full_prompt).await,
        };
        if success {
            return;
        }
    }

    println!("✗ All models failed. Check your API keys.");
}

async fn try_claude(prompt: &str) -> bool {
    let api_key = match env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("✗ Claude: ANTHROPIC_API_KEY not set"); return false; }
    };
    println!("Asking Claude...\n");
    let client = Client::new();
    let res = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&json!({
            "model": "claude-sonnet-4-5",
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }]
        }))
        .send().await;

    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["content"][0]["text"].as_str() {
                println!("{}", text);
                true
            } else {
                println!("✗ Claude error: {:?}", body["error"]["message"]);
                false
            }
        }
        Err(e) => { println!("✗ Claude request failed: {}", e); false }
    }
}

async fn try_gpt(prompt: &str) -> bool {
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("✗ GPT: OPENAI_API_KEY not set"); return false; }
    };
    println!("Asking GPT...\n");
    let client = Client::new();
    let res = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&json!({
            "model": "gpt-4o",
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }]
        }))
        .send().await;

    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["choices"][0]["message"]["content"].as_str() {
                println!("{}", text);
                true
            } else {
                println!("✗ GPT error: {:?}", body["error"]["message"]);
                false
            }
        }
        Err(e) => { println!("✗ GPT request failed: {}", e); false }
    }
}

async fn try_gemini(prompt: &str) -> bool {
    let api_key = match env::var("GEMINI_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("✗ Gemini: GEMINI_API_KEY not set"); return false; }
    };
    println!("Asking Gemini...\n");
    let client = Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent?key={}",
        api_key
    );
    let res = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&json!({
            "contents": [{ "parts": [{ "text": prompt }] }]
        }))
        .send().await;

    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["candidates"][0]["content"]["parts"][0]["text"].as_str() {
                println!("{}", text);
                true
            } else {
                println!("✗ Gemini error: {:?}", body["error"]["message"]);
                false
            }
        }
        Err(e) => { println!("✗ Gemini request failed: {}", e); false }
    }
}

async fn try_grok(prompt: &str) -> bool {
    let api_key = match env::var("GROK_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("✗ Grok: GROK_API_KEY not set"); return false; }
    };
    println!("Asking Grok...\n");
    let client = Client::new();
    let res = client
        .post("https://api.x.ai/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&json!({
            "model": "grok-3",
            "max_tokens": 1024,
            "messages": [{ "role": "user", "content": prompt }]
        }))
        .send().await;

    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["choices"][0]["message"]["content"].as_str() {
                println!("{}", text);
                true
            } else {
                println!("✗ Grok error: {:?}", body["error"]["message"]);
                false
            }
        }
        Err(e) => { println!("✗ Grok request failed: {}", e); false }
    }
}


/// 커스텀 provider 추가. `velox chorus add <name> <base_url> <model> [key]`
pub fn add_provider(name: &str, base_url: &str, model: &str, key: &str) {
    let builtin = ["claude", "gpt", "gemini", "grok", "anthropic", "openai", "google", "xai"];
    if builtin.contains(&name.to_lowercase().as_str()) {
        println!("✗ '{}'는 내장 provider 이름이라 못 써요. 다른 이름으로.", name);
        return;
    }
    let mut ps = load_providers();
    ps.retain(|x| x.name != name);
    ps.push(ProviderConfig {
        name: name.to_string(),
        base_url: base_url.to_string(),
        model: model.to_string(),
        api_key: key.to_string(),
    });
    if save_providers(&ps) {
        println!("✓ 커스텀 provider 추가됨: {} ({} / {})", name, base_url, model);
        println!("  사용: velox chorus ask \"...\" --use {}", name);
        println!("  (저장: {})", PROVIDERS_FILE);
    } else {
        println!("✗ 저장 실패");
    }
}


/// 사용자가 직접 API 키를 입력해 저장 (.env). `velox chorus set <provider> <key>`
pub fn set_key(provider: &str, key: &str) {
    let var = match env_var_for(provider) {
        Some(v) => v,
        None => {
            println!("✗ 알 수 없는 provider: {} (claude / gpt / gemini / grok)", provider);
            return;
        }
    };
    let path = ".env";
    let mut lines: Vec<String> = std::fs::read_to_string(path)
        .map(|s| s.lines().map(|l| l.to_string()).collect())
        .unwrap_or_default();

    let prefix = format!("{}=", var);
    let mut found = false;
    for l in lines.iter_mut() {
        if l.trim_start().starts_with(&prefix) {
            *l = format!("{}={}", var, key);
            found = true;
        }
    }
    if !found {
        lines.push(format!("{}={}", var, key));
    }

    match std::fs::write(path, lines.join("\n") + "\n") {
        Ok(_) => {
            println!("✓ {} ({}) 키 저장됨 → .env (다음 실행부터 적용)", provider, var);
        }
        Err(e) => println!("✗ .env 쓰기 실패: {}", e),
    }
}

/// 연결된 모든 AI에 실제로 핑을 보내 응답 여부를 검증. `velox chorus test`
pub async fn test_all() {
    println!("=== APEX Chorus — 연결 테스트 ===\n");
    for p in ["claude", "gpt", "gemini", "grok"] {
        let var = env_var_for(p).unwrap();
        if env::var(var).is_err() {
            println!("✗ {:8} 키 없음 ({}) — `velox chorus set {} <key>`", p, var, p);
            continue;
        }
        let t = std::time::Instant::now();
        let ok = query_text_with(p, "Reply with exactly: OK").await.is_some();
        let ms = t.elapsed().as_millis();
        if ok {
            println!("✓ {:8} 응답 정상 ({}ms)", p, ms);
        } else {
            println!("✗ {:8} 응답 실패 — 키/네트워크 확인", p);
        }
    }
    for p in load_providers() {
        let t = std::time::Instant::now();
        let ok = query_text_with(&p.name, "Reply with exactly: OK").await.is_some();
        let ms = t.elapsed().as_millis();
        if ok {
            println!("✓ {:8} (커스텀) 응답 정상 ({}ms)", p.name, ms);
        } else {
            println!("✗ {:8} (커스텀) 응답 실패 — URL/모델/키 확인", p.name);
        }
    }
}

/// 심판 응답에서 0~10 점수 추출.
fn parse_score(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_ascii_digit() || c == '.' { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .map(|v| v.clamp(0.0, 1000.0))
}

/// 벤치 문제 세트. hard=true면 변별력 있는 어려운(오염 적은 새) 문제.
fn prompt_set(hard: bool) -> Vec<(&'static str, &'static str)> {
    if hard {
        vec![
            ("코딩", "Write a Rust function `longest_palindrome(s: &str) -> String` returning the longest palindromic substring. Code only, correct and efficient."),
            ("추론", "A bat and a ball cost $1.10 total. The bat costs $1.00 more than the ball. How much is the ball? Show your reasoning."),
            ("수학", "What is the remainder when 7^100 is divided by 13? Show the key step."),
            ("지식", "In exactly 2 sentences, explain why TCP uses a three-way handshake instead of a two-way one."),
            ("글쓰기", "Write one haiku (5-7-5 syllables) about a compiler error. Output only the haiku."),
        ]
    } else {
        vec![
            ("코딩", "Write a Rust function `fib(n: u64) -> u64` returning the nth Fibonacci number. Code only."),
            ("추론", "A train travels 60 km in 45 minutes. What is its speed in km/h?"),
            ("지식", "In one sentence, what is the key difference between TCP and UDP?"),
        ]
    }
}

/// AI 모델 벤치 (LLM-as-judge). 연결된 모든 모델에 같은 질문 → judge가 채점 → 리더보드.
/// 측정: 점수(카테고리별), 속도(지연), 처리량(chars/s), 크기(응답 길이).
/// `velox chorus bench [--judge <model>] [--hard]`
pub async fn bench(hard: bool) {
    println!("=== APEX Chorus — AI 모델 벤치 (다중 심판, 0~1000{}) ===", if hard { ", HARD" } else { "" });

    let prompts = prompt_set(hard);

    // 심판 패널 = 키 있는 내장 모델. 자기 답은 자기가 채점 안 함 → self-bias 제거.
    let panel: Vec<String> = ["claude", "gpt", "gemini", "grok"]
        .iter()
        .filter(|m| env_var_for(m).map(|v| env::var(v).is_ok()).unwrap_or(false))
        .map(|s| s.to_string())
        .collect();
    if panel.is_empty() {
        println!("심판으로 쓸 모델 없음 (키 필요).");
        return;
    }
    println!("심판 패널: {} · 각 답을 자기 외 심판들이 0~1000 채점 → 평균\n", panel.join(", "));

    // 대상 = 키 있는 내장 + 커스텀
    let mut models: Vec<String> = Vec::new();
    for m in ["claude", "gpt", "gemini", "grok"] {
        if env_var_for(m).map(|v| env::var(v).is_ok()).unwrap_or(false) {
            models.push(m.to_string());
        }
    }
    for p in load_providers() {
        models.push(p.name);
    }
    if models.is_empty() {
        println!("테스트할 모델 없음 (키 설정 필요).");
        return;
    }

    struct Row {
        model: String,
        cat_scores: Vec<f64>,
        lat_ms: u128,
        len: usize,
    }
    let mut rows: Vec<Row> = Vec::new();

    for model in &models {
        println!("[{}] 측정 중...", model);
        let mut cat_scores = Vec::new();
        let (mut lat, mut len) = (0u128, 0usize);
        for (cat, q) in &prompts {
            let t = std::time::Instant::now();
            let answer = query_text_with(model, q).await;
            lat += t.elapsed().as_millis();
            let answer = match answer {
                Some(a) => a,
                None => {
                    println!("  {} ✗ 응답 실패", cat);
                    cat_scores.push(0.0);
                    continue;
                }
            };
            len += answer.chars().count();
            let jp = format!(
                "You are an impartial expert evaluator. Rate the answer's correctness and quality on a \
                 precise 0 to 1000 scale (use the full range for fine discrimination). Reply with ONLY the integer.\n\n\
                 Question: {}\nAnswer: {}",
                q, answer
            );
            let mut sum = 0.0;
            let mut cnt = 0u32;
            for j in panel.iter().filter(|j| j.as_str() != model.as_str()) {
                if let Some(sc) = query_text_with(j, &jp).await.and_then(|s| parse_score(&s)) {
                    sum += sc;
                    cnt += 1;
                }
            }
            let score = if cnt > 0 { sum / cnt as f64 } else { 0.0 };
            println!("  {} → {:.0}/1000 (심판 {}명)", cat, score, cnt);
            cat_scores.push(score);
        }
        rows.push(Row { model: model.clone(), cat_scores, lat_ms: lat, len });
    }

    let np = prompts.len().max(1);
    let avg_score = |r: &Row| r.cat_scores.iter().sum::<f64>() / np as f64;
    rows.sort_by(|a, b| avg_score(b).partial_cmp(&avg_score(a)).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n=== 리더보드 (다중 심판 평균) ===");
    println!("{:<12}{:>10}{:>9}{:>11}{:>9}", "모델", "점수/1000", "속도ms", "처리량c/s", "크기");
    println!("{}", "-".repeat(53));
    for r in &rows {
        let sc = avg_score(r);
        let lat = r.lat_ms / np as u128;
        let size = r.len / np;
        let thru = if r.lat_ms > 0 {
            r.len as f64 / (r.lat_ms as f64 / 1000.0)
        } else {
            0.0
        };
        println!("{:<12}{:>10.0}{:>7}ms{:>11.0}{:>9}", r.model, sc, lat, thru, size);
    }

    println!("\n=== 카테고리별 점수 (/1000) ===");
    print!("{:<12}", "모델");
    for (cat, _) in &prompts {
        print!("{:>8}", cat);
    }
    println!();
    for r in &rows {
        print!("{:<12}", r.model);
        for s in &r.cat_scores {
            print!("{:>8.0}", s);
        }
        println!();
    }
    println!("\n※ 자기 답은 자기가 채점 안 함 → self-bias 완화. 패널 공통 편향은 남음(상대 참고용).");
}

/// AI 합의 — 같은 질문을 여러 모델에 → 공통점/차이를 종합. `velox chorus consensus "질문"`
pub async fn consensus(question: &str) {
    println!("=== APEX Chorus — AI 합의 (Consensus) ===\n");

    let mut models: Vec<String> = Vec::new();
    for m in ["claude", "gpt", "gemini", "grok"] {
        if env_var_for(m).map(|v| env::var(v).is_ok()).unwrap_or(false) {
            models.push(m.to_string());
        }
    }
    for p in load_providers() {
        models.push(p.name);
    }
    if models.len() < 2 {
        println!("합의하려면 응답 가능한 모델이 2개 이상 필요.");
        return;
    }

    println!("질문: {}\n", question);
    let mut answers: Vec<(String, String)> = Vec::new();
    for model in &models {
        print!("[{}] 응답 중...", model);
        std::io::Write::flush(&mut std::io::stdout()).ok();
        match query_text_with(model, question).await {
            Some(a) => {
                println!(" ✓");
                answers.push((model.clone(), a));
            }
            None => println!(" ✗"),
        }
    }
    if answers.len() < 2 {
        println!("\n응답이 2개 미만이라 합의 불가.");
        return;
    }

    println!("\n--- 각 모델 답변(앞부분) ---");
    for (m, a) in &answers {
        let snip: String = a.chars().take(110).collect::<String>().replace('\n', " ");
        let more = if a.chars().count() > 110 { "…" } else { "" };
        println!("[{}] {}{}", m, snip, more);
    }

    let mut combined = String::new();
    for (m, a) in &answers {
        combined.push_str(&format!("[{}]\n{}\n\n", m, a));
    }
    let synth_prompt = format!(
        "아래는 여러 AI가 같은 질문에 답한 것이다. 한국어로 간결하게:\n\
         1) 공통된 핵심 (모두 동의하는 내용)\n\
         2) 의견이 갈리는 지점 (차이/불일치)\n\
         답변을 그대로 반복하지 말고 합의/차이만 정리하라.\n\n\
         질문: {}\n\n{}",
        question, combined
    );
    let synthesizer = if env_var_for("claude").map(|v| env::var(v).is_ok()).unwrap_or(false) {
        "claude".to_string()
    } else {
        answers[0].0.clone()
    };
    println!("\n--- 종합 (by {}) ---", synthesizer);
    match query_text_with(&synthesizer, &synth_prompt).await {
        Some(s) => println!("{}", s.trim()),
        None => println!("(종합 실패)"),
    }
}

pub fn show_models() {
    let models = [
        ("claude", "ANTHROPIC_API_KEY", "Code / Architecture"),
        ("gpt",    "OPENAI_API_KEY",    "Strategy / Business"),
        ("gemini", "GEMINI_API_KEY",    "Docs / Analysis"),
        ("grok",   "GROK_API_KEY",      "Search / Latest info"),
    ];

    println!("=== APEX Chorus — Connected Models ===\n");
    println!("[내장]");
    for (name, key, role) in &models {
        let status = if env::var(key).is_ok() { "✓" } else { "✗" };
        println!("{} {:8} — {}", status, name, role);
    }
    let custom = load_providers();
    if !custom.is_empty() {
        println!("\n[커스텀 — chorus add 로 추가됨]");
        for p in &custom {
            println!("✓ {:8} — {} ({})", p.name, p.model, p.base_url);
        }
    }
    println!();
}