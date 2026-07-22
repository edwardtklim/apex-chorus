//! velox-server — APEX Velox 엔진의 HTTP 얼굴 + 로컬 웹 UI(Pulse 프로토타입).
//!
//! 사이트와 앱은 **완전히 같은 UI**다 — `site/index.html` 하나를 그대로 서빙한다.
//! 차이는 "권한"뿐: 이 서버가 켜져 있으면 그 UI가 /snapshot을 읽어 **앱 모드(라이브)**,
//! 공개로 호스팅되면 엔진에 못 닿아 **사이트 모드(소개+다운로드)**로 자동 전환된다.
//!
//! 안전 원칙: 읽기 전용만 · localhost(127.0.0.1) 바인딩만.

use axum::extract::{Path, Query, State};
use axum::http::{Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Html;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use velox_core::snapshot::Snapshot;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone)]
struct AppState {
    session_token: String,
}

/// 사이트와 앱이 공유하는 단일 UI 파일.
const INDEX_HTML: &str = include_str!("../../site/index.html");

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok(); // legacy/dev migration fallback; new keys use the OS credential store
    velox_core::credentials::migrate_dotenv(std::path::Path::new(".env"));
    let addr = std::env::var("VELOX_ADDR").unwrap_or_else(|_| "127.0.0.1:7878".into());
    let state = AppState {
        session_token: std::env::var("VELOX_SESSION_TOKEN")
            .unwrap_or_else(|_| format!("manual-{}", std::process::id())),
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/report/health", get(health_report))
        .route("/report/benchmark", post(cpu_benchmark))
        .route("/keys", post(save_keys))
        .route("/keys/status", get(keys_status))
        .route("/run/:cmd", get(run_cmd))
        .route("/diagnose/stream", get(diagnose_stream))
        .route("/version", get(version))
        .route("/profile", get(get_profile).post(save_profile))
        .route("/snapshot/save", post(save_baseline))
        .route("/snapshot/compare", get(compare_baseline))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_session,
        ))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("포트 바인딩 실패");
    println!("velox-server → http://{addr}");
    axum::serve(listener, app).await.expect("서버 종료");
}

async fn require_session(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if request.uri().path() == "/" {
        return next.run(request).await;
    }
    let cookie = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let expected = format!("apex_session={}", state.session_token);
    if cookie.split(';').any(|part| part.trim() == expected) {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, "invalid APEX session").into_response()
    }
}

/// 공유 UI 서빙 — 엔진 연결되면 /snapshot을 읽어 앱 모드로 뜬다.
async fn index(State(state): State<AppState>) -> impl IntoResponse {
    let cookie = format!(
        "apex_session={}; HttpOnly; SameSite=Strict; Path=/",
        state.session_token
    );
    ([(header::SET_COOKIE, cookie)], Html(INDEX_HTML))
}

/// 헬스 체크.
async fn health() -> &'static str {
    "ok"
}

/// 현재 앱 버전.
async fn version() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "version": APP_VERSION }))
}

/// 현재 스냅샷을 기준(baseline)으로 저장한다.
async fn save_baseline() -> Json<serde_json::Value> {
    let snap = tokio::task::spawn_blocking(Snapshot::collect).await.ok();
    if let Some(s) = &snap {
        let _ = std::fs::write(
            "velox_baseline.json",
            serde_json::to_string(s).unwrap_or_default(),
        );
    }
    Json(serde_json::json!({ "saved": snap.is_some() }))
}

/// 저장된 baseline과 현재 상태를 비교(구조 변화만). baseline 없으면 error.
async fn compare_baseline() -> Json<serde_json::Value> {
    let base: Option<Snapshot> = std::fs::read_to_string("velox_baseline.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let base = match base {
        Some(b) => b,
        None => return Json(serde_json::json!({ "error": "no_baseline" })),
    };
    let cur = tokio::task::spawn_blocking(Snapshot::collect)
        .await
        .unwrap_or_else(|_| base.clone());
    let diff = velox_core::snapshot::compare(&base, &cur);
    Json(serde_json::to_value(&diff).unwrap_or_default())
}

/// 사용자 프로필 (PC 이름 등). 로컬 파일 저장 — 암호화/앱파일 숨김은 별개(나중) 작업.
#[derive(Serialize, Deserialize, Default)]
struct Profile {
    #[serde(default)]
    pc_name: Option<String>,
}

async fn get_profile() -> Json<Profile> {
    let p = std::fs::read_to_string("velox_profile.json")
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Json(p)
}

async fn save_profile(Json(p): Json<Profile>) -> Json<serde_json::Value> {
    let _ = std::fs::write(
        "velox_profile.json",
        serde_json::to_string_pretty(&p).unwrap_or_default(),
    );
    Json(serde_json::json!({ "ok": true }))
}

/// 현재 시스템 스냅샷(JSON). `collect()`는 블로킹이라 `spawn_blocking`으로 감싼다.
async fn snapshot() -> Json<Snapshot> {
    let snap = tokio::task::spawn_blocking(Snapshot::collect)
        .await
        .expect("snapshot 태스크 패닉");
    Json(snap)
}

async fn health_report() -> Json<velox_core::health::HealthReport> {
    Json(
        tokio::task::spawn_blocking(velox_core::health::HealthReport::collect)
            .await
            .unwrap_or_else(|_| {
                velox_core::health::HealthReport::from_snapshot(Snapshot::default())
            }),
    )
}

async fn cpu_benchmark() -> Json<velox_core::benchmark::CpuBenchmarkReport> {
    Json(
        tokio::task::spawn_blocking(velox_core::benchmark::CpuBenchmarkReport::run)
            .await
            .expect("benchmark task failed"),
    )
}

/// 브라우저에서 넘어온 API 키. (앱 모드에서만 호출됨)
#[derive(Deserialize)]
struct Keys {
    claude: Option<String>,
    gpt: Option<String>,
    gemini: Option<String>,
    grok: Option<String>,
}

/// API 키를 OS 자격증명 저장소에 저장한다. 값은 응답/로그에 절대 포함하지 않는다.
async fn save_keys(Json(k): Json<Keys>) -> Json<serde_json::Value> {
    let pairs: Vec<(&str, String)> = [
        ("claude", k.claude),
        ("gpt", k.gpt),
        ("gemini", k.gemini),
        ("grok", k.grok),
    ]
    .into_iter()
    .filter_map(|(name, v)| v.filter(|s| !s.trim().is_empty()).map(|s| (name, s)))
    .collect();

    let mut saved = 0;
    let mut errors = Vec::new();
    for (provider, secret) in pairs {
        match velox_core::credentials::set(provider, &secret) {
            Ok(()) => saved += 1,
            Err(error) => errors.push(format!("{provider}: {error}")),
        }
    }
    Json(serde_json::json!({ "saved": saved, "errors": errors }))
}

/// 어떤 키가 설정됐는지만 반환 (값은 절대 노출 안 함).
async fn keys_status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "claude": velox_core::ai::has_key("claude"),
        "gpt": velox_core::ai::has_key("gpt"),
        "gemini": velox_core::ai::has_key("gemini"),
        "grok": velox_core::ai::has_key("grok"),
    }))
}

#[derive(Deserialize, Default)]
struct DiagnoseQuery {
    #[serde(default)]
    scope: velox_core::privacy::ContextScope,
}

/// 3단계 AI 진단을 **실시간 스트리밍(SSE)**으로 보낸다 — UI가 채팅처럼 한 줄씩 보여준다.
/// 읽기 전용 "대화"다(시스템을 바꾸지 않음). 데모용으로 95°C를 주입한다.
async fn diagnose_stream(
    Query(options): Query<DiagnoseQuery>,
) -> Sse<ReceiverStream<Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(16);
    tokio::spawn(async move {
        fn ev(who: &str, text: &str) -> Result<Event, Infallible> {
            Ok(Event::default()
                .json_data(serde_json::json!({ "who": who, "text": text }))
                .unwrap_or_else(|_| Event::default().data("")))
        }
        // 클라이언트가 취소/닫으면 tx.send가 실패 → 남은 (느린) AI 호출을 중단한다.
        macro_rules! push {
            ($e:expr) => {
                if tx.send($e).await.is_err() {
                    return;
                }
            };
        }

        let snap = tokio::task::spawn_blocking(Snapshot::collect).await.ok();
        let context = snap
            .as_ref()
            .map(|s| velox_core::privacy::AiContext::from_snapshot(s, options.scope));
        let state = context
            .as_ref()
            .map(|c| c.to_prompt_json())
            .unwrap_or_else(|| "{}".into());
        push!(ev(
            "시스템",
            &format!(
                "AI 전송 범위: {:?}\n전송 전 미리보기: {state}",
                options.scope
            )
        ));

        let intent = velox_core::ai::query_text_with(
            "claude",
            &format!(
                "다음 시스템 상태에서 사용자가 가장 걱정할 점을 한국어 한 줄로 요약:\n{state}"
            ),
        )
        .await
        .unwrap_or_else(|| "(응답 없음 — API 키 확인)".into());
        push!(ev("Customer · Claude", &intent));

        let eng = velox_core::ai::query_text_with(
            "gpt",
            &format!(
                "너는 시스템 엔지니어 AI다. 안전하고 되돌릴 수 있는 조치를 한국어 한 줄로 제안하라(애매하면 '조치 없음').\n사용자 의도: {}\n상태:\n{state}",
                intent.trim()
            ),
        )
        .await
        .unwrap_or_else(|| "(응답 없음 — API 키 확인)".into());
        push!(ev("Engineer · GPT", &eng));

        push!(ev(
            "done",
            "진단 완료. 실제 조치는 CLI(velox diagnose --fix)에서 승인 후에만 실행됩니다."
        ));
    });
    Sse::new(ReceiverStream::new(rx))
}

/// velox CLI 실행 파일 경로 (같은 폴더의 velox.exe 우선, 없으면 PATH의 velox).
fn velox_bin() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let cand = dir.join("velox.exe");
        if cand.exists() {
            return cand;
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

/// **화이트리스트된 읽기전용/dry-run 명령만** 실행한다 (임의 명령 실행 방지).
/// 시스템을 바꾸는 것(diagnose --fix, checkpoint restore, 전원변경)은 여기 없다 = HTTP로 안 엶.
async fn run_cmd(Path(cmd): Path<String>) -> String {
    let args: Vec<&'static str> = match cmd.as_str() {
        "doctor" => vec!["doctor"],
        "diagnose" => vec!["diagnose", "--simulate-hot"],
        "bench" => vec!["bench", "cpu"],
        "drivers" => vec!["drivers"],
        "gpu" => vec!["gpu", "status"],
        "thermals" => vec!["thermals"],
        _ => return "알 수 없는 명령입니다.".into(),
    };
    tokio::task::spawn_blocking(move || run_velox(&args))
        .await
        .unwrap_or_else(|_| "실행 태스크 실패".into())
}
