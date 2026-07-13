//! velox-server — APEX Velox 엔진의 HTTP 얼굴 + 로컬 웹 UI(Pulse 프로토타입).
//!
//! 사이트와 앱은 **완전히 같은 UI**다 — `site/index.html` 하나를 그대로 서빙한다.
//! 차이는 "권한"뿐: 이 서버가 켜져 있으면 그 UI가 /snapshot을 읽어 **앱 모드(라이브)**,
//! 공개로 호스팅되면 엔진에 못 닿아 **사이트 모드(소개+다운로드)**로 자동 전환된다.
//!
//! 안전 원칙: 읽기 전용만 · localhost(127.0.0.1) 바인딩만.

use axum::extract::Path;
use axum::response::sse::{Event, Sse};
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use velox_core::snapshot::Snapshot;

const ADDR: &str = "127.0.0.1:7878";
const APP_VERSION: &str = "0.13.0";

/// 사이트와 앱이 공유하는 단일 UI 파일.
const INDEX_HTML: &str = include_str!("../../site/index.html");

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok(); // 저장된 API 키(.env)를 프로세스 env로 로드
    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/snapshot", get(snapshot))
        .route("/keys", post(save_keys))
        .route("/keys/status", get(keys_status))
        .route("/run/:cmd", get(run_cmd))
        .route("/diagnose/stream", get(diagnose_stream))
        .route("/version", get(version))
        .route("/profile", get(get_profile).post(save_profile))
        .route("/snapshot/save", post(save_baseline))
        .route("/snapshot/compare", get(compare_baseline));

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
    // 현재 서버 프로세스 env에도 즉시 반영 → 방금 저장한 키로 바로 스트리밍 진단이 됨.
    for (name, val) in &pairs {
        unsafe { std::env::set_var(name, val) };
    }
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

/// 3단계 AI 진단을 **실시간 스트리밍(SSE)**으로 보낸다 — UI가 채팅처럼 한 줄씩 보여준다.
/// 읽기 전용 "대화"다(시스템을 바꾸지 않음). 데모용으로 95°C를 주입한다.
async fn diagnose_stream() -> Sse<ReceiverStream<Result<Event, Infallible>>> {
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
        let base = snap
            .as_ref()
            .map(summarize)
            .unwrap_or_else(|| "스냅샷 읽기 실패".to_string());
        let state = format!("{base}\n- 최고 온도: 95.0°C  [시뮬레이션]");
        push!(ev("시스템", &format!("시스템을 읽었습니다.\n{state}")));

        let intent = velox_core::ai::query_text_with(
            "claude",
            &format!("다음 시스템 상태에서 사용자가 가장 걱정할 점을 한국어 한 줄로 요약:\n{state}"),
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

/// 스냅샷을 짧은 요약 문자열로.
fn summarize(s: &Snapshot) -> String {
    format!(
        "- CPU: {} ({}코어), 사용률 {:.0}%\n- RAM: {} MB\n- 전원 모드: {}\n- 설치 드라이버: {}개",
        s.system.cpu_model,
        s.system.logical_cores,
        s.cpu_usage,
        s.system.ram_total_mb,
        s.plan_label,
        s.drivers.len(),
    )
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
