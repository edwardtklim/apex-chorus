use reqwest::Client;
use serde_json::json;
use std::env;

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

pub async fn ask(prompt: &str, model: &str) {
    match model {
        "gpt" => ask_gpt(prompt).await,
        "gemini" => ask_gemini(prompt).await,
        "grok" => ask_grok(prompt).await,
        _ => ask_claude(prompt).await,
    }
}

async fn ask_claude(prompt: &str) {
    let api_key = match env::var("ANTHROPIC_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("Error: ANTHROPIC_API_KEY not set"); return; }
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
    handle_anthropic(res).await;
}

async fn ask_gpt(prompt: &str) {
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("Error: OPENAI_API_KEY not set"); return; }
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
    handle_openai(res).await;
}

async fn ask_gemini(prompt: &str) {
    let api_key = match env::var("GEMINI_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("Error: GEMINI_API_KEY not set"); return; }
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
    handle_gemini(res).await;
}

async fn ask_grok(prompt: &str) {
    let api_key = match env::var("GROK_API_KEY") {
        Ok(k) => k,
        Err(_) => { println!("Error: GROK_API_KEY not set"); return; }
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
    handle_openai(res).await;
}

async fn handle_anthropic(res: Result<reqwest::Response, reqwest::Error>) {
    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["content"][0]["text"].as_str() {
                println!("{}", text);
            } else {
                println!("Error: {:?}", body);
            }
        }
        Err(e) => println!("Request failed: {}", e),
    }
}

async fn handle_openai(res: Result<reqwest::Response, reqwest::Error>) {
    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["choices"][0]["message"]["content"].as_str() {
                println!("{}", text);
            } else {
                println!("Error: {:?}", body);
            }
        }
        Err(e) => println!("Request failed: {}", e),
    }
}

async fn handle_gemini(res: Result<reqwest::Response, reqwest::Error>) {
    match res {
        Ok(r) => {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(text) = body["candidates"][0]["content"]["parts"][0]["text"].as_str() {
                println!("{}", text);
            } else {
                println!("Error: {:?}", body);
            }
        }
        Err(e) => println!("Request failed: {}", e),
    }
}