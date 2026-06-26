use reqwest::Client;
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
        _ => {
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
}

pub fn show_models() {
    let models = [
        ("claude", "ANTHROPIC_API_KEY", "Code / Architecture"),
        ("gpt",    "OPENAI_API_KEY",    "Strategy / Business"),
        ("gemini", "GEMINI_API_KEY",    "Docs / Analysis"),
        ("grok",   "GROK_API_KEY",      "Search / Latest info"),
    ];

    println!("=== APEX Chorus — Connected Models ===\n");
    for (name, key, role) in &models {
        let status = if env::var(key).is_ok() { "✓" } else { "✗" };
        println!("{} {:8} — {}", status, name, role);
    }
    println!();
}