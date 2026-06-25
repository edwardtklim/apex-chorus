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

/// Like `ask`, but returns the AI's text instead of printing it.
/// Used by `diagnose` for the "reason" stage of the action loop.
pub async fn query_text(prompt: &str) -> Option<String> {
    if let Ok(api_key) = env::var("ANTHROPIC_API_KEY") {
        let client = Client::new();
        if let Ok(r) = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&json!({
                "model": "claude-sonnet-4-5",
                "max_tokens": 1024,
                "messages": [{ "role": "user", "content": prompt }]
            }))
            .send()
            .await
        {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["content"][0]["text"].as_str() {
                return Some(text.to_string());
            }
        }
    }
    None
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