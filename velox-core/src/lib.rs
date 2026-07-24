//! velox-core — APEX Velox 공유 엔진.
//!
//! CLI·GUI(Pulse)·플러그인이 공통으로 쓰는 읽기/AI/안전 엔진을 점진적으로 추출한다.
//! 원칙: **엔진은 데이터를 반환하고, 표시는 호출자(CLI 등)가 한다.**
//!
//! Phase 1 — 읽기 엔진: util(콘솔 디코딩), watch(CPU/RAM/Disk/Net).
//! Phase 2 — AI 엔진: ai(provider 호출·라우팅).
//! Phase 3 — 안전/액션 엔진: checkpoint(상태 저장·복원), action(화이트리스트 동작).
//! Phase 5 — 데이터 스냅샷: snapshot(전원/CPU/온도 → 직렬화 가능한 데이터).

pub mod action;
pub mod ai;
pub mod benchmark;
pub mod checkpoint;
pub mod council;
pub mod credentials;
pub mod evidence;
pub mod health;
pub mod policy;
pub mod privacy;
pub mod snapshot;
pub mod util;
pub mod watch;
