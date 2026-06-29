//! velox-server — APEX Velox 엔진의 HTTP 얼굴.
//!
//! velox-core를 그대로 재사용해 시스템 상태를 **읽기 전용** REST로 노출한다.
//! (core=엔진 · velox-cli=터미널 · velox-server=HTTP — 같은 엔진, 세 얼굴)
//!
//! 안전 원칙:
//! - **읽기 전용 GET만.** 액션(diagnose/checkpoint/전원변경)은 HTTP로 열지 않는다
//!   — 네트워크가 시스템을 바꾸는 구멍이 되므로.
//! - **localhost(127.0.0.1) 바인딩만.** 외부 네트워크에 노출하지 않는다.

use axum::{routing::get, Json, Router};
use velox_core::snapshot::Snapshot;

const ADDR: &str = "127.0.0.1:7878";

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health))
        .route("/snapshot", get(snapshot));

    let listener = tokio::net::TcpListener::bind(ADDR)
        .await
        .expect("포트 바인딩 실패");
    println!("velox-server → http://{ADDR}  (읽기 전용: GET /health, /snapshot)");
    axum::serve(listener, app).await.expect("서버 종료");
}

/// 헬스 체크 — 서버가 살아있는지.
async fn health() -> &'static str {
    "ok"
}

/// 현재 시스템 스냅샷을 JSON으로. `collect()`는 블로킹(powercfg/WMI/sysinfo)이라
/// `spawn_blocking`으로 async 런타임 스레드를 막지 않게 한다.
async fn snapshot() -> Json<Snapshot> {
    let snap = tokio::task::spawn_blocking(Snapshot::collect)
        .await
        .expect("snapshot 태스크 패닉");
    Json(snap)
}
