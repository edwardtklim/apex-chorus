use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use sysinfo::System;

pub fn route_model(prompt: &str) -> &'static str {
    let p = prompt.to_lowercase();

    if p.contains("code") || p.contains("rust") || p.contains("bug") ||
       p.contains("error") || p.contains("function") || p.contains("review") ||
       p.contains("코드") || p.contains("버그") || p.contains("함수") {
        "claude"
    } else if p.contains("market") || p.contains("business") || p.contains("strategy") ||
              p.contains("idea") || p.contains("revenue") || p.contains("growth") ||
              p.contains("시장") || p.contains("전략") || p.contains("아이디어") {
        "gpt"
    } else if p.contains("latest") || p.contains("recent") || p.contains("news") ||
              p.contains("today") || p.contains("2025") || p.contains("2026") ||
              p.contains("최신") || p.contains("뉴스") || p.contains("오늘") {
        "grok"
    } else if p.contains("document") || p.contains("analyze") || p.contains("summarize") ||
              p.contains("file") || p.contains("문서") || p.contains("분석") || p.contains("요약") {
        "gemini"
    } else {
        "claude"
    }
}

/// 의미기반 라우팅 — 라우터 모델이 요청 의도를 보고 최적 모델을 고른다.
/// 실패하면 키워드 라우팅(route_model)으로 폴백.
pub async fn route_semantic(prompt: &str) -> String {
    let router_prompt = format!(
        "You are a routing classifier. Pick the single best AI model for the user's request.\n\
         - claude: coding, architecture, systems, careful step-by-step reasoning\n\
         - gpt: strategy, business, general problem solving\n\
         - gemini: documents, analysis, summarization, multimodal\n\
         - grok: latest news, real-time/current events, search\n\
         Reply with ONLY one word: claude, gpt, gemini, or grok.\n\n\
         Request: {}",
        prompt
    );
    // 라우터는 빠른 gpt 사용 (없으면 claude). 둘 다 안 되면 키워드 폴백.
    for router in ["gpt", "claude"] {
        if env_var_for(router).map(|v| env::var(v).is_ok()).unwrap_or(false) {
            if let Some(resp) = query_text_with(router, &router_prompt).await {
                let pick = resp.to_lowercase();
                for m in ["claude", "gpt", "gemini", "grok"] {
                    if pick.contains(m) {
                        return m.to_string();
                    }
                }
            }
        }
    }
    route_model(prompt).to_string()
}

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

/// Query a SPECIFIC provider, returning its text. Powers diagnose's 3-stage
/// pipeline (Customer=Claude, Engineer=GPT, Confirmer=Gemini).
pub async fn query_text_with(model: &str, prompt: &str) -> Option<String> {
    let client = Client::new();
    match model {
        "gpt" => {
            let key = env::var("OPENAI_API_KEY").ok()?;
            let r = client
                .post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", key))
                .header("content-type", "application/json")
                .json(&json!({
                    "model": "gpt-4o", "max_tokens": 1024,
                    "messages": [{ "role": "user", "content": prompt }]
                }))
                .send().await.ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            body["choices"][0]["message"]["content"].as_str().map(|s| s.to_string())
        }
        "gemini" => {
            let key = env::var("GEMINI_API_KEY").ok()?;
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent?key={}",
                key
            );
            let r = client
                .post(&url)
                .header("content-type", "application/json")
                .json(&json!({ "contents": [{ "parts": [{ "text": prompt }] }] }))
                .send().await.ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            // Gemini 2.5 Pro는 thinking 모델 — parts에 thought 조각이 섞일 수 있으니
            // 모든 part의 text를 모아 답을 추출한다 (parts[0]만 보면 None 날 수 있음).
            let parts = body["candidates"][0]["content"]["parts"].as_array()?;
            let text: String = parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            (!text.is_empty()).then_some(text)
        }
        "grok" => {
            let key = env::var("GROK_API_KEY").ok()?;
            let r = client
                .post("https://api.x.ai/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", key))
                .header("content-type", "application/json")
                .json(&json!({
                    "model": "grok-3", "max_tokens": 1024,
                    "messages": [{ "role": "user", "content": prompt }]
                }))
                .send().await.ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            body["choices"][0]["message"]["content"].as_str().map(|s| s.to_string())
        }
        "claude" | "anthropic" => {
            let key = env::var("ANTHROPIC_API_KEY").ok()?;
            let r = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&json!({
                    "model": "claude-sonnet-4-5", "max_tokens": 1024,
                    "messages": [{ "role": "user", "content": prompt }]
                }))
                .send().await.ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            body["content"][0]["text"].as_str().map(|s| s.to_string())
        }
        other => {
            // 커스텀 provider (OpenAI 호환): velox_providers.json 에서 조회
            let p = load_providers().into_iter().find(|x| x.name == other)?;
            query_openai_compatible(&client, &p.base_url, &p.model, &p.api_key, prompt).await
        }
    }
}

// ---------------- 커스텀 provider (OpenAI 호환) ----------------

const PROVIDERS_FILE: &str = "velox_providers.json";

#[derive(Serialize, Deserialize, Clone)]
struct ProviderConfig {
    name: String,
    base_url: String,
    model: String,
    #[serde(default)]
    api_key: String,
}

fn load_providers() -> Vec<ProviderConfig> {
    std::fs::read_to_string(PROVIDERS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_providers(ps: &[ProviderConfig]) -> bool {
    serde_json::to_string_pretty(ps)
        .ok()
        .and_then(|s| std::fs::write(PROVIDERS_FILE, s).ok())
        .is_some()
}

/// OpenAI 호환 엔드포인트 호출 — OpenRouter / Ollama(localhost:11434/v1) / 커스텀 등 대부분 호환.
async fn query_openai_compatible(
    client: &Client,
    base_url: &str,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Option<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&json!({
            "model": model,
            "messages": [{ "role": "user", "content": prompt }]
        }));
    if !api_key.is_empty() && api_key.to_lowercase() != "none" {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }
    let r = req.send().await.ok()?;
    let body: serde_json::Value = r.json().await.ok()?;
    body["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
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

/// provider 별칭 → .env 환경변수 이름.
fn env_var_for(provider: &str) -> Option<&'static str> {
    match provider.to_lowercase().as_str() {
        "claude" | "anthropic" => Some("ANTHROPIC_API_KEY"),
        "gpt" | "openai" => Some("OPENAI_API_KEY"),
        "gemini" | "google" => Some("GEMINI_API_KEY"),
        "grok" | "xai" => Some("GROK_API_KEY"),
        _ => None,
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
    rows.sort_by(|a, b| avg_score(b).partial_cmp(&avg_score(a)).unwrap());

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