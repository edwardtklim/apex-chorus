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
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
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
    let _log_guard = velox_core::logging::init();
    tracing::info!(target: "velox::server", "server start");
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
        .route("/keys/:provider", delete(delete_key))
        .route("/policies/status", get(policies_status))
        .route("/policies/consent", post(policies_consent))
        .route("/policies/:provider", delete(policies_revoke))
        .route("/models", get(models_status).post(models_set))
        .route("/models/:provider", delete(models_reset))
        .route("/usage/summary", get(usage_summary))
        .route("/usage/recording", post(usage_recording))
        .route("/usage/records", delete(usage_clear))
        .route("/project/scan", post(project_scan))
        .route("/run/:cmd", get(run_cmd))
        .route("/diagnose/stream", get(diagnose_stream))
        .route("/council/stream", get(council_stream))
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

#[derive(Deserialize)]
struct ProjectScanRequest {
    path: String,
}

/// Scan a local project without executing commands, writing files, or sending
/// project data to a cloud provider. The response deliberately omits the
/// absolute project root.
async fn project_scan(
    Json(request): Json<ProjectScanRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if request.path.trim().is_empty() {
        return Err(project_api_error(
            StatusCode::BAD_REQUEST,
            "invalid_project_path",
            "Choose a local project directory.",
        ));
    }

    let result = tokio::task::spawn_blocking(move || {
        let session = velox_core::project::open(
            std::path::Path::new(&request.path),
            velox_core::project::ProjectLimits::default(),
        )?;
        let name = session.name().to_owned();
        let scan = session.scan();
        Ok::<_, velox_core::project::ProjectError>((name, scan))
    })
    .await;

    match result {
        Ok(Ok((name, scan))) => Ok(Json(serde_json::json!({
            "project": { "name": name },
            "scan": scan,
            "safety": {
                "read_only": true,
                "cloud_sent": false,
                "writes_performed": false,
                "commands_executed": false
            }
        }))),
        Ok(Err(_)) => Err(project_api_error(
            StatusCode::BAD_REQUEST,
            "scan_failed",
            "The project could not be scanned. Check the directory and its permissions.",
        )),
        Err(_) => Err(project_api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "scan_unavailable",
            "The project scanner is temporarily unavailable.",
        )),
    }
}

fn project_api_error(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({ "error": { "code": code, "message": message } })),
    )
}

/// 현재 스냅샷을 기준(baseline)으로 저장한다.
async fn save_baseline() -> Json<serde_json::Value> {
    let snap = tokio::task::spawn_blocking(Snapshot::collect).await.ok();
    if let Some(s) = &snap {
        let _ = std::fs::write(
            velox_core::paths::report_file("velox_baseline.json"),
            serde_json::to_string(s).unwrap_or_default(),
        );
    }
    Json(serde_json::json!({ "saved": snap.is_some() }))
}

/// 저장된 baseline과 현재 상태를 비교(구조 변화만). baseline 없으면 error.
async fn compare_baseline() -> Json<serde_json::Value> {
    let base: Option<Snapshot> =
        std::fs::read_to_string(velox_core::paths::report_file("velox_baseline.json"))
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
    let p = std::fs::read_to_string(velox_core::paths::resolve("velox_profile.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    Json(p)
}

async fn save_profile(Json(p): Json<Profile>) -> Json<serde_json::Value> {
    let _ = std::fs::write(
        velox_core::paths::resolve("velox_profile.json"),
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

const BUILTIN_PROVIDERS: [&str; 4] = ["claude", "gpt", "gemini", "grok"];

/// provider별 정책 요약 (키·툴·정책 원문 없음). 앱이 어떤 provider가 동의됐는지 표시하는 용도.
async fn policies_status() -> Json<serde_json::Value> {
    let providers: Vec<velox_core::policy::ProviderPolicyStatus> = BUILTIN_PROVIDERS
        .iter()
        .map(|p| velox_core::policy::policy_status(p))
        .collect();
    Json(serde_json::json!({ "providers": providers }))
}

/// 사용자의 명시적 클릭으로만 호출되는 Cloud 호출 동의. scope는 enum 검증됨.
#[derive(Deserialize)]
struct ConsentReq {
    provider: String,
    #[serde(default)]
    scope: velox_core::privacy::ContextScope,
}

/// provider별 Cloud 동의 저장 (allowed_tools=[], require_confirmation=true 고정은 grant_consent가 강제).
async fn policies_consent(Json(req): Json<ConsentReq>) -> Json<serde_json::Value> {
    if !velox_core::policy::provider_exists(&req.provider) {
        return Json(serde_json::json!({ "ok": false, "error": "unknown_provider" }));
    }
    let ok = velox_core::policy::grant_consent(&req.provider, req.scope);
    Json(serde_json::json!({
        "ok": ok,
        "status": velox_core::policy::policy_status(&req.provider),
    }))
}

/// provider 동의 철회.
async fn policies_revoke(Path(provider): Path<String>) -> Json<serde_json::Value> {
    let ok = velox_core::policy::revoke_consent(&provider);
    Json(serde_json::json!({ "ok": ok }))
}

/// provider의 API 키를 OS 자격증명 저장소에서 삭제 (값은 절대 다루지 않음).
async fn delete_key(Path(provider): Path<String>) -> Json<serde_json::Value> {
    match velox_core::credentials::delete(&provider) {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

/// 내장 provider별 현재 모델 ID.
async fn models_status() -> Json<serde_json::Value> {
    let providers: Vec<serde_json::Value> = BUILTIN_PROVIDERS
        .iter()
        .map(|p| serde_json::json!({ "provider": p, "model": velox_core::ai::model_name(p) }))
        .collect();
    Json(serde_json::json!({ "providers": providers }))
}

/// 모델 설정 요청 (사용자의 명시적 저장으로만 호출).
#[derive(Deserialize)]
struct ModelSetReq {
    provider: String,
    model: String,
}

/// provider의 모델 ID 설정 — set_model이 검증(빈/제어문자/길이)을 강제한다.
async fn models_set(Json(req): Json<ModelSetReq>) -> Json<serde_json::Value> {
    match velox_core::ai::set_model(&req.provider, &req.model) {
        Ok(model) => Json(serde_json::json!({ "ok": true, "model": model })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
}

/// APEX가 기록한 사용량 요약 + **추정** 비용.
/// 표시 규칙: `Estimated API cost` · `APEX-recorded usage only` ·
/// 구독 청구서/잔액이 아님. 단가 미설정이면 비용은 unknown.
async fn usage_summary(Query(q): Query<UsageQuery>) -> Json<serde_json::Value> {
    let period =
        velox_core::ledger::Period::parse(&q.period).unwrap_or(velox_core::ledger::Period::Month);
    let ledger = velox_core::ledger::load();
    let now = velox_core::ledger::now_unix();
    let records = velox_core::ledger::in_period(&ledger.records, period, now);
    let totals = velox_core::ledger::totals(&records);
    let est = velox_core::pricing::estimate(&records, &velox_core::pricing::load(), now);
    let recent: Vec<serde_json::Value> = ledger
        .records
        .iter()
        .rev()
        .take(10)
        .map(|r| {
            serde_json::json!({
                "date": velox_core::ledger::date_string(r.unix_ts),
                "feature": r.feature,
                "provider": r.provider,
                "model": r.model,
                "status": r.status.label(),
                "duration_ms": r.duration_ms,
            })
        })
        .collect();
    Json(serde_json::json!({
        "period": period.label(),
        "recording_enabled": ledger.settings.enabled,
        "retention_days": ledger.settings.retention_days,
        "totals": totals,
        "by_provider": velox_core::ledger::by_provider(&records),
        "cost": {
            "display": est.display(),
            "complete": est.is_complete(),
            "currency": est.currency,
            "pricing_unconfigured": est.pricing_unconfigured,
            "pricing_stale": est.pricing_stale,
            "pricing_version": est.pricing_version,
            "pricing_updated": est.pricing_effective_date,
            "calls_missing_price": est.calls_missing_price,
            "calls_missing_usage": est.calls_missing_usage,
            "models_missing_price": est.models_missing_price,
        },
        "recent": recent,
        "notice": "Estimated API cost · APEX-recorded usage only · not subscription billing or provider balance",
    }))
}

#[derive(Deserialize, Default)]
struct UsageQuery {
    #[serde(default = "default_period")]
    period: String,
}

fn default_period() -> String {
    "month".into()
}

/// 기록 on/off (사용자의 명시적 조작으로만).
#[derive(Deserialize)]
struct RecordingReq {
    enabled: bool,
}

async fn usage_recording(Json(req): Json<RecordingReq>) -> Json<serde_json::Value> {
    let ok = velox_core::ledger::set_enabled(req.enabled);
    Json(serde_json::json!({ "ok": ok, "enabled": req.enabled }))
}

/// 모든 세션 기록 삭제.
async fn usage_clear() -> Json<serde_json::Value> {
    let removed = velox_core::ledger::clear();
    Json(serde_json::json!({ "ok": true, "removed": removed }))
}

/// provider의 모델을 기본값으로 초기화.
async fn models_reset(Path(provider): Path<String>) -> Json<serde_json::Value> {
    match velox_core::ai::reset_model(&provider) {
        Ok(model) => Json(serde_json::json!({ "ok": true, "model": model })),
        Err(error) => Json(serde_json::json!({ "ok": false, "error": error })),
    }
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

        // AI로 나가는 payload는 오직 typed EvidenceBundle에서 생성한다(승인 범위로 최소화).
        let snap = tokio::task::spawn_blocking(Snapshot::collect)
            .await
            .unwrap_or_default();
        let health = velox_core::health::HealthReport::from_snapshot(snap);
        let bundle = match velox_core::evidence::EvidenceBundle::from_health_report(
            &health,
            options.scope,
        ) {
            Ok(b) => b,
            Err(e) => {
                push!(ev("오류", &format!("Evidence 생성 실패: {e}")));
                return;
            }
        };
        let state = bundle.to_prompt();
        push!(ev(
            "시스템",
            &format!(
                "AI 전송 범위: {:?} · {} 항목\n전송 전 미리보기:\n{state}",
                options.scope,
                bundle.items.len()
            )
        ));

        // 정책 게이트 경유 — 미승인/범위초과면 이유를 말풍선으로 알리고 중단.
        macro_rules! gated {
            ($provider:expr, $purpose:expr, $prompt:expr) => {
                match velox_core::policy::execute_agent(velox_core::policy::AgentRequest {
                    provider: $provider.to_string(),
                    purpose: $purpose,
                    prompt: $prompt,
                    scope: options.scope,
                    requested_tools: std::collections::BTreeSet::new(),
                })
                .await
                {
                    Ok(r) => r.text,
                    Err(e) => {
                        push!(ev(
                            "시스템",
                            &format!(
                                "{} 호출 불가: {e}\n(동의: `velox chorus consent {}`)",
                                $provider, $provider
                            )
                        ));
                        return;
                    }
                }
            };
        }

        let intent = gated!(
            "claude",
            velox_core::policy::AgentPurpose::Diagnose,
            format!("다음 시스템 상태에서 사용자가 가장 걱정할 점을 한국어 한 줄로 요약:\n{state}")
        );
        push!(ev("Customer · Claude", &intent));

        let eng = gated!(
            "gpt",
            velox_core::policy::AgentPurpose::Propose,
            format!(
                "너는 시스템 엔지니어 AI다. 안전하고 되돌릴 수 있는 조치를 한국어 한 줄로 제안하라(애매하면 '조치 없음').\n사용자 의도: {}\n상태:\n{state}",
                intent.trim()
            )
        );
        push!(ev("Engineer · GPT", &eng));

        push!(ev(
            "done",
            "진단 완료. 실제 조치는 CLI(velox diagnose --fix)에서 승인 후에만 실행됩니다."
        ));
    });
    Sse::new(ReceiverStream::new(rx))
}

#[derive(Deserialize, Default)]
struct CouncilQuery {
    #[serde(default)]
    scope: velox_core::privacy::ContextScope,
    #[serde(default)]
    objective: String,
}

/// 한 CouncilEvent를 (말풍선 제목, 본문)으로. Provider/model을 함께 표시(불변조건: 어떤 provider인지).
fn council_label(e: &velox_core::council::CouncilEvent) -> (String, String) {
    use velox_core::council::CouncilEvent as E;
    let m = velox_core::ai::model_name;
    match e {
        E::Evidence { scope, items } => (
            "시스템 Evidence".into(),
            format!("{items} 항목 (scope={scope:?}) — 승인된 데이터만 전송"),
        ),
        E::Proposed { summary, findings } => (
            format!("Proposer · Claude ({})", m("claude")),
            format!("{summary} · finding {findings}개"),
        ),
        E::Reviewed { verdict, reasons } => (
            format!("Reviewer · GPT ({})", m("gpt")),
            if reasons.is_empty() {
                verdict.clone()
            } else {
                format!("{verdict} — {}", reasons.join("; "))
            },
        ),
        E::Revised { summary, findings } => (
            format!("Reviser · Claude ({})", m("claude")),
            format!("{summary} · finding {findings}개"),
        ),
        E::Gated { passed, reasons } => (
            "APEX Safety Gate".into(),
            if *passed {
                "통과 ✓".into()
            } else {
                format!("불통과: {}", reasons.join("; "))
            },
        ),
    }
}

/// 최종 CouncilDecision을 사람용 텍스트로.
fn council_decision_text(d: &velox_core::council::CouncilDecision) -> String {
    use velox_core::council::CouncilStatus as S;
    let head = match d.status {
        S::Approved => "승인 ✓",
        S::Rejected => "반려 ✗",
        S::Inconclusive => "결론 없음",
        S::Cancelled => "취소됨",
    };
    let mut s = format!("결정: {head}");
    if let Some(p) = &d.proposal {
        s.push_str(&format!("\n요약: {}", p.summary));
        for f in &p.findings {
            let cites: Vec<String> = f.evidence.iter().map(|id| id.0.clone()).collect();
            s.push_str(&format!("\n• {} [근거: {}]", f.statement, cites.join(", ")));
        }
    }
    if !d.reviewer_reasons.is_empty() {
        s.push_str(&format!("\n사유: {}", d.reviewer_reasons.join("; ")));
    }
    if d.requires_human_confirmation {
        s.push_str("\n⚠ 실행 전 사람 승인 필요");
    }
    s
}

/// Council 실시간 스트림. Server는 SSE 변환·취소만 하고, 판단은 velox-core::council이 한다.
async fn council_stream(
    Query(q): Query<CouncilQuery>,
) -> Sse<UnboundedReceiverStream<Result<Event, Infallible>>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
    tokio::spawn(async move {
        fn ev(who: &str, text: &str) -> Result<Event, Infallible> {
            Ok(Event::default()
                .json_data(serde_json::json!({ "who": who, "text": text }))
                .unwrap_or_else(|_| Event::default().data("")))
        }
        let scope = q.scope;
        let objective = if q.objective.trim().is_empty() {
            "이 시스템의 상태를 진단하고 개선점을 근거와 함께 제시하라".to_string()
        } else {
            q.objective.clone()
        };

        // Evidence: 결정론적 HealthReport에서 승인 범위로 최소화해 생성(임의 prompt 금지).
        let snap = tokio::task::spawn_blocking(Snapshot::collect)
            .await
            .unwrap_or_default();
        let health = velox_core::health::HealthReport::from_snapshot(snap);
        let bundle = match velox_core::evidence::EvidenceBundle::from_health_report(&health, scope)
        {
            Ok(b) => b,
            Err(e) => {
                let _ = tx.send(ev("오류", &format!("Evidence 생성 실패: {e}")));
                return;
            }
        };

        let req = velox_core::council::CouncilRequest {
            objective,
            evidence: bundle,
            approved_scope: scope,
        };
        // 클라이언트가 닫으면 send 실패 → cancel 세팅 → Council이 다음 단계에서 멈춘다.
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_cb = cancel.clone();
        let tx_cb = tx.clone();
        let on_event = move |e: &velox_core::council::CouncilEvent| {
            let (who, text) = council_label(e);
            if tx_cb.send(ev(&who, &text)).is_err() {
                cancel_cb.store(true, Ordering::Relaxed);
            }
        };
        let decision = velox_core::council::run(req, &cancel, &on_event).await;
        let _ = tx.send(ev("done", &council_decision_text(&decision)));
    });
    Sse::new(UnboundedReceiverStream::new(rx))
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
