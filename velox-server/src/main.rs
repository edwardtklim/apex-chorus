//! velox-server — APEX Velox 엔진의 HTTP 얼굴 + 로컬 웹 UI(Pulse 프로토타입).
//!
//! 사이트와 앱은 **완전히 같은 UI**다 — `site/index.html` 하나를 그대로 서빙한다.
//! 차이는 "권한"뿐: 이 서버가 켜져 있으면 그 UI가 /snapshot을 읽어 **앱 모드(라이브)**,
//! 공개로 호스팅되면 엔진에 못 닿아 **사이트 모드(소개+다운로드)**로 자동 전환된다.
//!
//! 안전 원칙: 읽기 전용만 · localhost(127.0.0.1) 바인딩만.

use axum::response::Html;
use axum::{routing::get, Json, Router};
use velox_core::snapshot::Snapshot;

const ADDR: &str = "127.0.0.1:7878";

/// 사이트와 앱이 공유하는 단일 UI 파일.
const INDEX_HTML: &str = include_str!("../../site/index.html");

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/snapshot", get(snapshot));

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
