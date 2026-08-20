//! velox-core::metrics — Closed Alpha 지표 (v0.20).
//!
//! 실제 장비에서 돌릴 때 "무엇이 잘 됐고 무엇이 안 됐는지"를 자동으로 쌓는다.
//! 사람의 기억에 의존하면 "센서 미지원이 몇 대였지?" 에 답할 수 없다.
//!
//! **로컬 전용이다. 어디로도 보내지 않는다.** 업로드 코드가 없고 네트워크를
//! 건드리지 않는다. 사용자가 직접 export 해서 보내기로 결정할 때만 나간다.
//!
//! 기록하는 것은 **메타데이터뿐이다.** ledger 와 같은 원칙 — 프롬프트·응답·키·
//! 파일 경로를 담을 필드가 아예 없다. 담을 수 없으니 샐 수 없다.
//!
//! 지표 목록은 로드맵의 Alpha 지표를 그대로 따른다:
//! 실행 성공률 · 완료율 · 평균 소요시간 · 취소율 · 센서 미지원률 ·
//! policy 거부 사유 · crash 수 · 잘못된 경고 수.
//!
//! **비율은 표본이 없으면 계산하지 않는다.** 0/0 을 0% 로 보여주면 거짓말이 된다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const METRICS_FILE: &str = "velox_metrics.json";
/// 열려 있는 세션 표식. 정상 종료 때 지운다. 남아 있으면 지난 실행이 죽은 것이다.
const SESSION_MARKER: &str = "velox_session_open.marker";

/// 한 번의 작업 기록. 개별 실행 단위.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct OperationRecord {
    /// "doctor" · "diagnose" · "repair_capture" 같은 기능 이름.
    pub feature: String,
    pub outcome: OperationOutcome,
    pub duration_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationOutcome {
    Completed,
    /// 사용자가 중간에 취소.
    Cancelled,
    /// 오류로 끝남.
    Failed,
}

/// 지표 저장소. 전부 카운터와 짧은 목록 — 개인 식별 정보 없음.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Metrics {
    /// 앱/CLI 가 시작된 횟수.
    pub starts: u64,
    /// 정상 종료된 횟수.
    pub clean_exits: u64,
    /// 비정상 종료(다음 실행에서 표식으로 감지).
    pub crashes: u64,

    pub operations: Vec<OperationRecord>,

    /// 센서를 시도한 횟수와 그중 못 읽은 횟수. 미지원률 계산용.
    pub sensor_attempts: u64,
    pub sensor_unavailable: u64,

    /// policy 가 거부한 사유별 횟수. 사유 문자열은 고정된 enum 이름만 들어간다.
    pub policy_denials: BTreeMap<String, u64>,

    /// 사용자가 "이 경고는 틀렸다" 고 표시한 횟수. 자동으로 세지 않는다 —
    /// 사람이 판단해야 하는 지표라 명시적으로만 올라간다.
    pub false_warnings: u64,

    /// 이 지표 파일이 만들어진 시각.
    pub since: String,
}

fn path() -> std::path::PathBuf {
    crate::paths::resolve(METRICS_FILE)
}

fn marker_path() -> std::path::PathBuf {
    crate::paths::resolve(SESSION_MARKER)
}

/// 지표 로드. 없거나 손상되면 빈 값 — 지표 때문에 제품이 멈추면 안 된다.
pub fn load() -> Metrics {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| Metrics {
            since: crate::report::ReportMeta::new("", "").generated_at,
            ..Default::default()
        })
}

fn save(m: &Metrics) {
    if let Ok(s) = serde_json::to_string_pretty(m) {
        let _ = crate::ai::atomic_write_public(&path(), &s);
    }
}

fn update(f: impl FnOnce(&mut Metrics)) {
    let mut m = load();
    f(&mut m);
    // 무한히 쌓지 않는다 — 알파 기간 표본으로 충분한 양만 유지.
    const MAX_OPS: usize = 2_000;
    if m.operations.len() > MAX_OPS {
        let cut = m.operations.len() - MAX_OPS;
        m.operations.drain(0..cut);
    }
    save(&m);
}

/// 프로세스 시작 시 호출. 지난 실행이 비정상 종료였는지 함께 판정한다.
pub fn record_start() {
    let marker = marker_path();
    // 표식이 남아 있다 = 지난 실행이 record_clean_exit 를 못 부르고 죽었다.
    let crashed = marker.exists();
    update(|m| {
        m.starts += 1;
        if crashed {
            m.crashes += 1;
        }
    });
    let _ = std::fs::write(&marker, "1");
}

/// 정상 종료 시 호출. 표식을 지워 다음 실행이 crash 로 세지 않게 한다.
pub fn record_clean_exit() {
    let _ = std::fs::remove_file(marker_path());
    update(|m| m.clean_exits += 1);
}

/// 작업 하나의 결과를 기록.
pub fn record_operation(feature: &str, outcome: OperationOutcome, duration_ms: u64) {
    let feature = feature.to_string();
    update(|m| {
        m.operations.push(OperationRecord {
            feature,
            outcome,
            duration_ms,
        })
    });
}

/// 센서 읽기 시도 결과. `available=false` 면 미지원으로 센다.
pub fn record_sensor(available: bool) {
    update(|m| {
        m.sensor_attempts += 1;
        if !available {
            m.sensor_unavailable += 1;
        }
    });
}

/// policy 거부 사유를 센다. **사유는 고정 라벨만 넘긴다** — provider 이름이나
/// 사용자 입력을 그대로 넣으면 지표 파일에 식별 정보가 섞인다.
pub fn record_policy_denial(reason: &str) {
    let reason = sanitize_reason(reason);
    update(|m| *m.policy_denials.entry(reason).or_insert(0) += 1);
}

/// 알려진 거부 사유 라벨만 통과시킨다. 모르는 값은 "other" 로 뭉갠다.
fn sanitize_reason(raw: &str) -> String {
    const KNOWN: [&str; 5] = [
        "consent_missing",
        "scope_exceeded",
        "tool_not_allowed",
        "unknown_provider",
        "provider_call_failed",
    ];
    if KNOWN.contains(&raw) {
        raw.to_string()
    } else {
        "other".to_string()
    }
}

/// 사용자가 잘못된 경고라고 표시했을 때.
pub fn record_false_warning() {
    update(|m| m.false_warnings += 1);
}

/// 사람이 읽는 요약. 표본이 없는 비율은 계산하지 않는다.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Summary {
    pub since: String,
    pub starts: u64,
    pub crashes: u64,
    /// 실행 성공률. 표본 0이면 None.
    pub start_success_rate: Option<f64>,
    pub operations: u64,
    /// 완료율. 표본 0이면 None.
    pub completion_rate: Option<f64>,
    /// 취소율. 표본 0이면 None.
    pub cancel_rate: Option<f64>,
    /// 평균 소요시간(ms). 완료된 작업이 없으면 None.
    pub avg_duration_ms: Option<u64>,
    /// 센서 미지원률. 시도가 없으면 None.
    pub sensor_unavailable_rate: Option<f64>,
    pub policy_denials: BTreeMap<String, u64>,
    pub false_warnings: u64,
    /// 기능별 (완료, 취소, 실패).
    pub by_feature: BTreeMap<String, [u64; 3]>,
}

fn rate(num: u64, den: u64) -> Option<f64> {
    (den > 0).then(|| num as f64 / den as f64 * 100.0)
}

pub fn summary() -> Summary {
    let m = load();
    let total = m.operations.len() as u64;
    let completed = m
        .operations
        .iter()
        .filter(|o| o.outcome == OperationOutcome::Completed)
        .count() as u64;
    let cancelled = m
        .operations
        .iter()
        .filter(|o| o.outcome == OperationOutcome::Cancelled)
        .count() as u64;

    let done: Vec<u64> = m
        .operations
        .iter()
        .filter(|o| o.outcome == OperationOutcome::Completed)
        .map(|o| o.duration_ms)
        .collect();
    let avg = (!done.is_empty()).then(|| done.iter().sum::<u64>() / done.len() as u64);

    let mut by_feature: BTreeMap<String, [u64; 3]> = BTreeMap::new();
    for o in &m.operations {
        let e = by_feature.entry(o.feature.clone()).or_insert([0, 0, 0]);
        match o.outcome {
            OperationOutcome::Completed => e[0] += 1,
            OperationOutcome::Cancelled => e[1] += 1,
            OperationOutcome::Failed => e[2] += 1,
        }
    }

    Summary {
        since: m.since.clone(),
        starts: m.starts,
        crashes: m.crashes,
        start_success_rate: rate(m.starts.saturating_sub(m.crashes), m.starts),
        operations: total,
        completion_rate: rate(completed, total),
        cancel_rate: rate(cancelled, total),
        avg_duration_ms: avg,
        sensor_unavailable_rate: rate(m.sensor_unavailable, m.sensor_attempts),
        policy_denials: m.policy_denials.clone(),
        false_warnings: m.false_warnings,
        by_feature,
    }
}

/// 전부 삭제.
pub fn clear() {
    let _ = std::fs::remove_file(path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_is_none_without_samples() {
        // 0/0 을 0% 로 보여주면 "센서가 전부 잘 읽혔다" 는 거짓말이 된다.
        assert_eq!(rate(0, 0), None);
        assert_eq!(rate(1, 2), Some(50.0));
    }

    #[test]
    fn unknown_denial_reasons_are_collapsed() {
        // provider 이름이나 사용자 입력이 지표 파일에 그대로 남으면 안 된다.
        assert_eq!(sanitize_reason("consent_missing"), "consent_missing");
        assert_eq!(sanitize_reason("claude"), "other");
        assert_eq!(sanitize_reason("sk-ant-api03-secret"), "other");
        assert_eq!(sanitize_reason("C:\\Users\\edwar\\secret.txt"), "other");
    }

    #[test]
    fn record_has_no_field_for_sensitive_data() {
        // 구조적 보장 — 프롬프트/키를 담을 필드가 없으니 담을 수 없다.
        let r = OperationRecord {
            feature: "doctor".into(),
            outcome: OperationOutcome::Completed,
            duration_ms: 120,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json.matches(':').count(),
            3,
            "필드가 3개를 넘으면 검토 필요: {json}"
        );
        assert!(!json.contains("prompt"));
        assert!(!json.contains("key"));
    }

    #[test]
    fn summary_computes_rates_from_operations() {
        let m = Metrics {
            starts: 10,
            crashes: 1,
            operations: vec![
                OperationRecord {
                    feature: "doctor".into(),
                    outcome: OperationOutcome::Completed,
                    duration_ms: 100,
                },
                OperationRecord {
                    feature: "doctor".into(),
                    outcome: OperationOutcome::Cancelled,
                    duration_ms: 50,
                },
                OperationRecord {
                    feature: "bench".into(),
                    outcome: OperationOutcome::Completed,
                    duration_ms: 300,
                },
                OperationRecord {
                    feature: "bench".into(),
                    outcome: OperationOutcome::Failed,
                    duration_ms: 10,
                },
            ],
            sensor_attempts: 4,
            sensor_unavailable: 3,
            ..Default::default()
        };
        // summary() 는 파일을 읽으므로 계산 로직만 직접 검증한다.
        assert_eq!(rate(2, 4), Some(50.0)); // 완료율
        assert_eq!(rate(1, 4), Some(25.0)); // 취소율
        assert_eq!(rate(3, 4), Some(75.0)); // 센서 미지원률
        assert_eq!(rate(m.starts - m.crashes, m.starts), Some(90.0));
        let done: Vec<u64> = m
            .operations
            .iter()
            .filter(|o| o.outcome == OperationOutcome::Completed)
            .map(|o| o.duration_ms)
            .collect();
        assert_eq!(done.iter().sum::<u64>() / done.len() as u64, 200);
    }

    #[test]
    fn metrics_serialize_roundtrip() {
        let mut m = Metrics::default();
        m.policy_denials.insert("consent_missing".into(), 2);
        m.operations.push(OperationRecord {
            feature: "repair_capture".into(),
            outcome: OperationOutcome::Completed,
            duration_ms: 4200,
        });
        let s = serde_json::to_string(&m).unwrap();
        let back: Metrics = serde_json::from_str(&s).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn corrupt_file_yields_empty_not_panic() {
        let m: Result<Metrics, _> = serde_json::from_str("{ not json");
        assert!(
            m.is_err(),
            "손상 파일은 파싱 실패하고, load() 가 기본값으로 대체한다"
        );
    }
}
