//! velox-core::ai — AI provider 엔진.
//!
//! 여러 AI(claude/gpt/gemini/grok + 커스텀 OpenAI 호환)를 호출하고 라우팅하는 순수 엔진.
//! **데이터를 반환만 한다 — 표시는 호출자(CLI/GUI/플러그인)가 한다.**

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

pub const PROVIDERS_FILE: &str = "velox_providers.json";

/// 타임아웃이 걸린 HTTP 클라이언트 — provider가 느리거나 무응답이어도 무한 대기하지 않는다.
/// (diagnose는 AI를 3번 연속 호출하므로 한 곳이 매달리면 전체가 멈춘다)
fn http_client() -> Client {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// provider 별칭 → .env 환경변수 이름.
pub fn env_var_for(provider: &str) -> Option<&'static str> {
    match provider.to_lowercase().as_str() {
        "claude" | "anthropic" => Some("ANTHROPIC_API_KEY"),
        "gpt" | "openai" => Some("OPENAI_API_KEY"),
        "gemini" | "google" => Some("GEMINI_API_KEY"),
        "grok" | "xai" => Some("GROK_API_KEY"),
        _ => None,
    }
}

/// 키워드 기반 라우팅.
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

/// 특정 provider 호출, 텍스트 반환. diagnose 3단계 파이프라인·bench·consensus의 엔진.
pub async fn query_text_with(model: &str, prompt: &str) -> Option<String> {
    let client = http_client();
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
            // Gemini 2.5 Pro는 thinking 모델 — 모든 part의 text를 모아 답을 추출.
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

#[derive(Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub name: String,
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
}

pub fn load_providers() -> Vec<ProviderConfig> {
    std::fs::read_to_string(PROVIDERS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_providers(ps: &[ProviderConfig]) -> bool {
    serde_json::to_string_pretty(ps)
        .ok()
        .and_then(|s| std::fs::write(PROVIDERS_FILE, s).ok())
        .is_some()
}

/// OpenAI 호환 엔드포인트 호출 — OpenRouter / Ollama(localhost:11434/v1) / 커스텀 등 호환.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_aliases_resolve() {
        assert_eq!(env_var_for("claude"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(env_var_for("anthropic"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(env_var_for("gpt"), Some("OPENAI_API_KEY"));
        assert_eq!(env_var_for("GEMINI"), Some("GEMINI_API_KEY")); // 대소문자 무시
        assert_eq!(env_var_for("grok"), Some("GROK_API_KEY"));
        assert_eq!(env_var_for("unknown_provider"), None);
    }

    #[test]
    fn keyword_routing_picks_specialist() {
        assert_eq!(route_model("fix this rust error"), "claude");
        assert_eq!(route_model("코드 버그 봐줘"), "claude");
        assert_eq!(route_model("go-to-market strategy"), "gpt");
        assert_eq!(route_model("latest news today"), "grok");
        assert_eq!(route_model("analyze this document"), "gemini");
    }

    #[test]
    fn routing_defaults_to_claude() {
        assert_eq!(route_model("hello there"), "claude");
    }
}

#[cfg(test)]
mod net_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    /// 응답 본문을 고정으로 돌려주는 1회용 로컬 HTTP 목 서버. base_url을 반환한다.
    /// (외부 mock 크레이트 없이 std만 — 진짜 네트워크는 안 탐)
    fn spawn_mock(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("목 서버 bind 실패");
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf); // 요청은 일부만 읽어도 응답엔 충분
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn parses_openai_style_reply() {
        let base = spawn_mock(r#"{"choices":[{"message":{"content":"mocked reply"}}]}"#);
        let client = http_client();
        let got = query_openai_compatible(&client, &base, "test-model", "", "hi").await;
        assert_eq!(got, Some("mocked reply".to_string()));
    }

    #[tokio::test]
    async fn returns_none_on_unexpected_shape() {
        // 서버가 형식 다른 JSON을 줘도 패닉 없이 None.
        let base = spawn_mock(r#"{"error":"rate limited"}"#);
        let client = http_client();
        let got = query_openai_compatible(&client, &base, "test-model", "", "hi").await;
        assert_eq!(got, None);
    }
}
