//! velox-server — APEX Velox 엔진의 HTTP 얼굴 + 로컬 웹 UI(Pulse 프로토타입).
//!
//! 사이트와 앱은 **완전히 같은 UI**다 — `site/index.html` 하나를 그대로 서빙한다.
//! 차이는 "권한"뿐: 이 서버가 켜져 있으면 그 UI가 /snapshot을 읽어 **앱 모드(라이브)**,
//! 공개로 호스팅되면 엔진에 못 닿아 **사이트 모드(소개+다운로드)**로 자동 전환된다.
//!
//! 안전 원칙: 읽기 전용만 · localhost(127.0.0.1) 바인딩만.

use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use velox_core::snapshot::Snapshot;

const ADDR: &str = "127.0.0.1:7878";

/// 사이트와 앱이 공유하는 단일 UI 파일.
const INDEX_HTML: &str = include_str!("../../site/index.html");

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/keys", post(save_keys))
        .route("/keys/status", get(keys_status))
        .route("/doctor", get(doctor))
        .route("/diagnose", get(diagnose));

    let listener = tokio::net::TcpListener::bind(ADDR)
        .await
        .expect("포트 바인딩 실패");
    println!("velox-server → http://{ADDR}  (브라우저로 열어보세요 · 읽기 전용)");
    axum::serve(listener, app).await.expect("서버 종료");
}

/// 공유 UI 서빙 — 엔진 연결되면 /snapshot을 읽어 앱 모드로 뜬다.
async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// 헬스 체크.
async fn health() -> &'static str {
    "ok"
}

/// 현재 시스템 스냅샷(JSON). `collect()`는 블로킹이라 `spawn_blocking`으로 감싼다.
async fn snapshot() -> Json<Snapshot> {
    let snap = tokio::task::spawn_blocking(Snapshot::collect)
        .await
        .expect("snapshot 태스크 패닉");
    Json(snap)
}

/// 브라우저에서 넘어온 API 키. (앱 모드에서만 호출됨)
#[derive(Deserialize)]
struct Keys {
    claude: Option<String>,
    gpt: Option<String>,
    gemini: Option<String>,
    grok: Option<String>,
}

/// API 키를 로컬 `.env`에 저장한다. **설정(자격증명) 쓰기**이지 시스템 조치가 아니며,
/// localhost 전용이다. diagnose 같은 *시스템 변경* 액션은 여전히 HTTP로 열지 않는다.
async fn save_keys(Json(k): Json<Keys>) -> Json<serde_json::Value> {
    let pairs: Vec<(&str, String)> = [
        ("ANTHROPIC_API_KEY", k.claude),
        ("OPENAI_API_KEY", k.gpt),
        ("GEMINI_API_KEY", k.gemini),
        ("GROK_API_KEY", k.grok),
    ]
    .into_iter()
    .filter_map(|(name, v)| v.filter(|s| !s.trim().is_empty()).map(|s| (name, s)))
    .collect();

    let saved = upsert_env(&pairs);
    Json(serde_json::json!({ "saved": saved }))
}

/// 어떤 키가 설정됐는지만 반환 (값은 절대 노출 안 함).
async fn keys_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "claude": env_has("ANTHROPIC_API_KEY"),
        "gpt": env_has("OPENAI_API_KEY"),
        "gemini": env_has("GEMINI_API_KEY"),
        "grok": env_has("GROK_API_KEY"),
    }))
}

/// `.env`에 `NAME=<비어있지 않은 값>` 줄이 있는지.
fn env_has(name: &str) -> bool {
    std::fs::read_to_string(".env")
        .map(|s| {
            s.lines().any(|l| {
                let l = l.trim_start();
                l.starts_with(&format!("{name}="))
                    && l.splitn(2, '=').nth(1).map_or(false, |v| !v.trim().is_empty())
            })
        })
        .unwrap_or(false)
}

/// velox CLI 실행 파일 경로 (같은 폴더의 velox.exe 우선, 없으면 PATH의 velox).
fn velox_bin() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let cand = dir.join("velox.exe");
            if cand.exists() {
                return cand;
            }
        }
    }
    std::path::PathBuf::from("velox")
}

/// velox CLI를 subprocess로 실행하고 출력(텍스트)을 돌려준다.
fn run_velox(args: &[&str]) -> String {
    match std::process::Command::new(velox_bin()).args(args).output() {
        Ok(o) => {
            let mut s = velox_core::util::decode_console(&o.stdout);
            let err = velox_core::util::decode_console(&o.stderr);
            if !err.trim().is_empty() {
                s.push('\n');
                s.push_str(&err);
            }
            s
        }
        Err(e) => format!("velox 실행 실패: {e}"),
    }
}

/// AI 종합 진단 (읽기 전용 + AI). 시스템을 바꾸지 않는다.
async fn doctor() -> String {
    tokio::task::spawn_blocking(|| run_velox(&["doctor"]))
        .await
        .unwrap_or_else(|_| "doctor 태스크 실패".into())
}

/// 3단계 AI 안전 진단 — **--simulate-hot(제안만·실행 X)**. 실제 조치는 HTTP로 열지 않는다.
async fn diagnose() -> String {
    tokio::task::spawn_blocking(|| run_velox(&["diagnose", "--simulate-hot"]))
        .await
        .unwrap_or_else(|_| "diagnose 태스크 실패".into())
}

/// `.env`(gitignore됨)의 `KEY=값` 줄을 upsert — 기존 다른 키는 보존한다.
fn upsert_env(pairs: &[(&str, String)]) -> usize {
    if pairs.is_empty() {
        return 0;
    }
    let path = ".env";
    let mut lines: Vec<String> = std::fs::read_to_string(path)
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    for (name, val) in pairs {
        let prefix = format!("{name}=");
        let entry = format!("{name}={val}");
        match lines.iter_mut().find(|l| l.trim_start().starts_with(&prefix)) {
            Some(l) => *l = entry,
            None => lines.push(entry),
        }
    }
    let _ = std::fs::write(path, lines.join("\n") + "\n");
    pairs.len()
}
