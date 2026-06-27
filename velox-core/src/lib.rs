//! velox-core — APEX Velox 공유 엔진.
//!
//! CLI·GUI(Pulse)·플러그인이 공통으로 쓰는 읽기/AI/안전 엔진을 점진적으로 추출한다.
//! 원칙: **엔진은 데이터를 반환하고, 표시는 호출자(CLI 등)가 한다.**
//!
//! Phase 1 — 읽기 엔진: util(콘솔 디코딩), watch(CPU/RAM/Disk/Net).

pub mod util;
pub mod watch;
