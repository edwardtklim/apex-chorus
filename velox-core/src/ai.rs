//! velox-core::ai — AI provider 엔진.
//!
//! 여러 AI(claude/gpt/gemini/grok + 커스텀 OpenAI 호환)를 호출하고 라우팅하는 순수 엔진.
//! **데이터를 반환만 한다 — 표시는 호출자(CLI/GUI/플러그인)가 한다.**

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

pub const PROVIDERS_FILE: &str = "velox_providers.json";
pub const MODELS_FILE: &str = "velox_models.json";

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

/// Resolve a provider key for the current process. Environment variables remain a
/// migration/development fallback; newly saved keys live in the OS credential store.
pub fn api_key_for(provider: &str) -> Option<String> {
    env_var_for(provider)
        .and_then(|name| env::var(name).ok())
        .filter(|key| !key.trim().is_empty())
        .or_else(|| crate::credentials::get(provider))
}

pub fn has_key(provider: &str) -> bool {
    api_key_for(provider).is_some()
}

/// 내장 provider(claude/gpt/gemini/grok)가 쓸 **모델 이름**.
/// 코드에 박지 않고 설정으로 분리한다 (Council/Agent Policy가 역할별로 모델을 고르기 위한 토대).
/// 해석 우선순위: 환경변수(`VELOX_MODEL_*`) > 설정파일(`velox_models.json`) > 기본값.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ModelConfig {
    pub claude: String,
    pub gpt: String,
    pub gemini: String,
    pub grok: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            claude: "claude-sonnet-4-5".into(),
            gpt: "gpt-4o".into(),
            gemini: "gemini-2.5-pro".into(),
            grok: "grok-3".into(),
        }
    }
}

/// 모델 ID의 최대 허용 길이(문자 수).
pub const MAX_MODEL_ID_LEN: usize = 128;

/// **파일만** 읽은 모델 구성(환경변수 오버라이드 없음). 파일이 없으면 기본값.
/// 저장 시 기준값 — env 오버라이드를 파일에 굳혀 넣지 않기 위해 분리한다.
fn load_models_file() -> ModelConfig {
    std::fs::read_to_string(MODELS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 설정파일 + 환경변수를 반영한 **현재 유효** 모델 구성. 파일이 없으면 기본값.
pub fn load_models() -> ModelConfig {
    let mut m = load_models_file();
    let ov = |var: &str, cur: &mut String| {
        if let Ok(v) = env::var(var) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                *cur = v;
            }
        }
    };
    ov("VELOX_MODEL_CLAUDE", &mut m.claude);
    ov("VELOX_MODEL_GPT", &mut m.gpt);
    ov("VELOX_MODEL_GEMINI", &mut m.gemini);
    ov("VELOX_MODEL_GROK", &mut m.grok);
    m
}

/// 원자적 파일 쓰기 — 임시 파일에 쓴 뒤 rename. 쓰다가 죽어도 원본이 깨지지 않는다.
pub(crate) fn atomic_write(path: &str, contents: &str) -> std::io::Result<()> {
    let tmp = format!("{path}.tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// 모델 구성을 `velox_models.json`에 원자적으로 저장.
pub fn save_models(m: &ModelConfig) -> bool {
    serde_json::to_string_pretty(m)
        .ok()
        .and_then(|s| atomic_write(MODELS_FILE, &s).ok())
        .is_some()
}

/// 사용자가 준 모델 ID 검증 — 공백 제거 후 빈 값 / 제어문자 / 과도한 길이를 거부.
/// 성공하면 정규화(trim)된 ID를 돌려준다.
pub fn validate_model_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("모델 ID가 비어 있습니다".into());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("모델 ID에 제어문자가 포함되어 있습니다".into());
    }
    if trimmed.chars().count() > MAX_MODEL_ID_LEN {
        return Err(format!("모델 ID가 너무 깁니다 (최대 {MAX_MODEL_ID_LEN}자)"));
    }
    Ok(trimmed.to_string())
}

/// 내장 provider의 슬롯을 가리키는 가변 참조. 알 수 없는 provider면 None.
fn model_slot<'a>(m: &'a mut ModelConfig, provider: &str) -> Option<&'a mut String> {
    match provider.to_lowercase().as_str() {
        "claude" | "anthropic" => Some(&mut m.claude),
        "gpt" | "openai" => Some(&mut m.gpt),
        "gemini" | "google" => Some(&mut m.gemini),
        "grok" | "xai" => Some(&mut m.grok),
        _ => None,
    }
}

const UNKNOWN_PROVIDER: &str = "알 수 없는 provider (claude / gpt / gemini / grok)";

/// provider의 모델을 검증 후 설정하고 저장. 성공하면 저장된 모델 ID를 반환.
pub fn set_model(provider: &str, model_id: &str) -> Result<String, String> {
    let id = validate_model_id(model_id)?;
    let mut m = load_models_file();
    match model_slot(&mut m, provider) {
        Some(slot) => *slot = id.clone(),
        None => return Err(UNKNOWN_PROVIDER.into()),
    }
    if save_models(&m) {
        Ok(id)
    } else {
        Err("설정 저장 실패".into())
    }
}

/// provider의 모델을 기본값으로 되돌리고 저장. 성공하면 복원된 기본 모델 ID를 반환.
pub fn reset_model(provider: &str) -> Result<String, String> {
    let mut defaults = ModelConfig::default();
    let default_id = match model_slot(&mut defaults, provider) {
        Some(slot) => slot.clone(),
        None => return Err(UNKNOWN_PROVIDER.into()),
    };
    let mut m = load_models_file();
    if let Some(slot) = model_slot(&mut m, provider) {
        *slot = default_id.clone();
    }
    if save_models(&m) {
        Ok(default_id)
    } else {
        Err("설정 저장 실패".into())
    }
}

/// provider 별칭 → 설정된(유효) 모델 이름. 알 수 없는 provider면 빈 문자열.
pub fn model_name(provider: &str) -> String {
    let mut m = load_models();
    model_slot(&mut m, provider)
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// 키워드 기반 라우팅.
pub fn route_model(prompt: &str) -> &'static str {
    let p = prompt.to_lowercase();

    if p.contains("code")
        || p.contains("rust")
        || p.contains("bug")
        || p.contains("error")
        || p.contains("function")
        || p.contains("review")
        || p.contains("코드")
        || p.contains("버그")
        || p.contains("함수")
    {
        "claude"
    } else if p.contains("market")
        || p.contains("business")
        || p.contains("strategy")
        || p.contains("idea")
        || p.contains("revenue")
        || p.contains("growth")
        || p.contains("시장")
        || p.contains("전략")
        || p.contains("아이디어")
    {
        "gpt"
    } else if p.contains("latest")
        || p.contains("recent")
        || p.contains("news")
        || p.contains("today")
        || p.contains("2025")
        || p.contains("2026")
        || p.contains("최신")
        || p.contains("뉴스")
        || p.contains("오늘")
    {
        "grok"
    } else if p.contains("document")
        || p.contains("analyze")
        || p.contains("summarize")
        || p.contains("file")
        || p.contains("문서")
        || p.contains("분석")
        || p.contains("요약")
    {
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
        if has_key(router)
            && let Some(resp) = query_text_with(router, &router_prompt).await
        {
            let pick = resp.to_lowercase();
            for m in ["claude", "gpt", "gemini", "grok"] {
                if pick.contains(m) {
                    return m.to_string();
                }
            }
        }
    }
    route_model(prompt).to_string()
}

/// 특정 provider 호출, 텍스트 반환. diagnose 3단계 파이프라인·bench·consensus의 엔진.
pub async fn query_text_with(model: &str, prompt: &str) -> Option<String> {
    let client = http_client();
    let models = load_models();
    match model {
        "gpt" => {
            let key = api_key_for("gpt")?;
            let r = client
                .post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", key))
                .header("content-type", "application/json")
                .json(&json!({
                    "model": &models.gpt, "max_tokens": 1024,
                    "messages": [{ "role": "user", "content": prompt }]
                }))
                .send()
                .await
                .ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            body["choices"][0]["message"]["content"]
                .as_str()
                .map(|s| s.to_string())
        }
        "gemini" => {
            let key = api_key_for("gemini")?;
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                models.gemini, key
            );
            let r = client
                .post(&url)
                .header("content-type", "application/json")
                .json(&json!({ "contents": [{ "parts": [{ "text": prompt }] }] }))
                .send()
                .await
                .ok()?;
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
            let key = api_key_for("grok")?;
            let r = client
                .post("https://api.x.ai/v1/chat/completions")
                .header("Authorization", format!("Bearer {}", key))
                .header("content-type", "application/json")
                .json(&json!({
                    "model": &models.grok, "max_tokens": 1024,
                    "messages": [{ "role": "user", "content": prompt }]
                }))
                .send()
                .await
                .ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            body["choices"][0]["message"]["content"]
                .as_str()
                .map(|s| s.to_string())
        }
        "claude" | "anthropic" => {
            let key = api_key_for("claude")?;
            let r = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", &key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&json!({
                    "model": &models.claude, "max_tokens": 1024,
                    "messages": [{ "role": "user", "content": prompt }]
                }))
                .send()
                .await
                .ok()?;
            let body: serde_json::Value = r.json().await.ok()?;
            body["content"][0]["text"].as_str().map(|s| s.to_string())
        }
        other => {
            // 커스텀 provider (OpenAI 호환): velox_providers.json 에서 조회
            let p = load_providers().into_iter().find(|x| x.name == other)?;
            let key = if p.api_key.is_empty() {
                crate::credentials::get(other).unwrap_or_default()
            } else {
                p.api_key
            };
            query_openai_compatible(&client, &p.base_url, &p.model, &key, prompt).await
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
    /// 명시적으로 로컬(오프라인/사설) provider로 표시. `policy`가 이 값 + loopback
    /// 엔드포인트를 함께 확인해야만 Local로 취급한다 (기본 false = Cloud).
    #[serde(default)]
    pub local: bool,
}

pub fn load_providers() -> Vec<ProviderConfig> {
    std::fs::read_to_string(PROVIDERS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_providers(ps: &[ProviderConfig]) -> bool {
    let scrubbed: Vec<ProviderConfig> = ps
        .iter()
        .cloned()
        .map(|mut provider| {
            if !provider.api_key.is_empty() && provider.api_key.to_lowercase() != "none" {
                let _ = crate::credentials::set(&provider.name, &provider.api_key);
                provider.api_key.clear();
            }
            provider
        })
        .collect();
    serde_json::to_string_pretty(&scrubbed)
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

    #[test]
    fn model_config_defaults_match_builtins() {
        // 기본값은 코드에서 뽑아낸 원래 하드코딩 모델과 같아야 한다(동작 불변).
        let m = ModelConfig::default();
        assert_eq!(m.claude, "claude-sonnet-4-5");
        assert_eq!(m.gpt, "gpt-4o");
        assert_eq!(m.gemini, "gemini-2.5-pro");
        assert_eq!(m.grok, "grok-3");
    }

    #[test]
    fn model_config_partial_json_fills_defaults() {
        // 일부만 지정한 설정도 나머지 필드는 기본값으로 채워진다.
        let m: ModelConfig = serde_json::from_str(r#"{"gpt":"gpt-4o-mini"}"#).unwrap();
        assert_eq!(m.gpt, "gpt-4o-mini");
        assert_eq!(m.claude, "claude-sonnet-4-5");
        assert_eq!(m.grok, "grok-3");
    }

    #[test]
    fn model_config_round_trips_through_json() {
        let m = ModelConfig {
            claude: "claude-opus-4-8".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: ModelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn validate_model_id_accepts_and_trims_normal_ids() {
        assert_eq!(validate_model_id("gpt-4o").unwrap(), "gpt-4o");
        assert_eq!(
            validate_model_id("  claude-sonnet-4-5  ").unwrap(),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            validate_model_id("anthropic/claude-3.5-sonnet").unwrap(),
            "anthropic/claude-3.5-sonnet"
        );
    }

    #[test]
    fn validate_model_id_rejects_bad_input() {
        assert!(validate_model_id("").is_err()); // 빈 값
        assert!(validate_model_id("   ").is_err()); // 공백뿐
        assert!(validate_model_id("gpt\n4o").is_err()); // 줄바꿈(제어문자)
        assert!(validate_model_id("bad\tid").is_err()); // 탭(제어문자)
        assert!(validate_model_id(&"x".repeat(MAX_MODEL_ID_LEN + 1)).is_err()); // 초과
        assert!(validate_model_id(&"x".repeat(MAX_MODEL_ID_LEN)).is_ok()); // 경계 OK
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
