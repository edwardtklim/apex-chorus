use reqwest::Client;
use serde_json::json;
use std::env;

pub async fn ask(prompt: &str) {
    let api_key = match env::var("ANTHROPIC_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            println!("Error: ANTHROPIC_API_KEY not set");
            println!("Set it with: $env:ANTHROPIC_API_KEY = \"your-key-here\"");
            return;
        }
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
            "messages": [
                { "role": "user", "content": prompt }
            ]
        }))
        .send()
        .await;

    match res {
        Ok(response) => {
            let body: serde_json::Value = response.json().await.unwrap_or_default();
            if let Some(text) = body["content"][0]["text"].as_str() {
                println!("{}", text);
            } else {
                println!("Error: {:?}", body);
            }
        }
        Err(e) => println!("Request failed: {}", e),
    }
}