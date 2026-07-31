//! velox-core::ledger — 로컬 세션 원장 (v0.17).
//!
//! APEX가 **자기가 호출한 AI 사용량**만 로컬에 기록한다. 소비자 구독(Claude Pro / ChatGPT Plus /
//! Gemini Advanced)의 잔액·청구서와는 **무관**하며 그렇게 표현하지 않는다.
//!
//! **개인정보 방어(구조적):** [`SessionRecord`]에는 프롬프트·AI 응답·Evidence 본문·API 키를
//! 담을 **필드 자체가 없다** → 실수로도 저장될 수 없다. 기록되는 것은 메타데이터
//! (기능·provider·모델·범위·상태·소요시간)와 provider가 응답에 실어 보낸 **토큰 수**뿐이다.
//!
//! 저장은 원자적이며, 보존 기간이 지난 기록은 자동 정리된다. 기록 자체를 끌 수도 있다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::privacy::ContextScope;

/// 세션 원장 파일 (기록 + 설정).
pub const LEDGER_FILE: &str = "velox_ledger.json";

/// 파일이 무한히 커지지 않도록 보관하는 최대 기록 수.
pub const MAX_RECORDS: usize = 5_000;

/// 한 번의 AI 호출 결과 상태.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// provider가 정상 응답.
    #[default]
    Success,
    /// 정책은 통과했으나 호출이 실패(네트워크·키·모델 등).
    Failed,
    /// 사용자가 취소.
    Cancelled,
    /// 정책 게이트가 거부(미동의·범위 초과·툴 미허용) — **호출 자체가 나가지 않음**.
    PolicyDenied,
}

impl SessionStatus {
    pub fn label(self) -> &'static str {
        match self {
            SessionStatus::Success => "success",
            SessionStatus::Failed => "failed",
            SessionStatus::Cancelled => "cancelled",
            SessionStatus::PolicyDenied => "policy_denied",
        }
    }
}

/// provider가 **응답에 실어 보낸** 토큰 수. 값이 없으면 `None`으로 남긴다(추정하지 않는다).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TokenUsage {
    pub input: Option<u64>,
    pub output: Option<u64>,
    /// 캐시에서 읽은 입력 토큰(제공하는 provider만).
    pub cache_read: Option<u64>,
}

impl TokenUsage {
    /// 아무 필드도 없으면 "usage unavailable" — 기록에 남기지 않는다.
    pub fn is_empty(&self) -> bool {
        self.input.is_none() && self.output.is_none() && self.cache_read.is_none()
    }
}

/// 한 건의 세션 기록. **프롬프트·응답·Evidence·키를 담는 필드가 존재하지 않는다.**
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SessionRecord {
    pub id: String,
    /// 시작 시각(Unix epoch seconds, UTC).
    pub unix_ts: u64,
    /// 어떤 기능이 호출했는지 (예: `diagnose`, `council.propose`).
    pub feature: String,
    pub provider: String,
    pub model: String,
    /// 사용자가 승인한 데이터 범위.
    pub scope: ContextScope,
    pub status: SessionStatus,
    pub duration_ms: u64,
    /// provider가 준 토큰 수. 없으면 `None`(= usage unavailable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

/// 기록 동작 설정.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct LedgerSettings {
    /// 기록 기능 전체 on/off.
    pub enabled: bool,
    /// 보존 기간(일). 0이면 무기한(단, MAX_RECORDS 상한은 유지).
    pub retention_days: u32,
}

impl Default for LedgerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 90,
        }
    }
}

/// 원장 파일 전체.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Ledger {
    pub settings: LedgerSettings,
    pub records: Vec<SessionRecord>,
}

/// 현재 Unix epoch seconds.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 원장 로드. 없거나 **손상되면 기본값**(빈 기록) — 손상 파일로 오동작하지 않는다.
pub fn load() -> Ledger {
    std::fs::read_to_string(LEDGER_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 원장을 원자적으로 저장.
pub fn save(ledger: &Ledger) -> bool {
    serde_json::to_string_pretty(ledger)
        .ok()
        .and_then(|s| crate::ai::atomic_write(LEDGER_FILE, &s).ok())
        .is_some()
}

/// 보존 기간과 최대 개수를 적용해 오래된 기록을 정리한다.
fn prune(ledger: &mut Ledger, now: u64) {
    if ledger.settings.retention_days > 0 {
        let cutoff = now.saturating_sub(ledger.settings.retention_days as u64 * 86_400);
        ledger.records.retain(|r| r.unix_ts >= cutoff);
    }
    if ledger.records.len() > MAX_RECORDS {
        let drop = ledger.records.len() - MAX_RECORDS;
        ledger.records.drain(0..drop);
    }
}

/// 기록 한 건 추가(설정이 꺼져 있으면 아무 것도 하지 않는다).
/// 호출자는 메타데이터만 넘긴다 — 프롬프트/응답을 넘길 수 있는 인자가 없다.
#[allow(clippy::too_many_arguments)]
pub fn record(
    feature: &str,
    provider: &str,
    model: &str,
    scope: ContextScope,
    status: SessionStatus,
    duration_ms: u64,
    usage: Option<TokenUsage>,
) {
    let mut ledger = load();
    if !ledger.settings.enabled {
        return;
    }
    let now = now_unix();
    let id = format!("{}-{:04}", now, ledger.records.len() % 10_000);
    ledger.records.push(SessionRecord {
        id,
        unix_ts: now,
        feature: feature.to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        scope,
        status,
        duration_ms,
        usage: usage.filter(|u| !u.is_empty()),
    });
    prune(&mut ledger, now);
    let _ = save(&ledger);
}

/// 모든 기록 삭제(설정은 유지). 삭제된 건수를 반환.
pub fn clear() -> usize {
    let mut ledger = load();
    let n = ledger.records.len();
    ledger.records.clear();
    let _ = save(&ledger);
    n
}

/// 기록 on/off.
pub fn set_enabled(enabled: bool) -> bool {
    let mut ledger = load();
    ledger.settings.enabled = enabled;
    save(&ledger)
}

/// 보존 기간(일) 설정. 0 = 무기한.
pub fn set_retention_days(days: u32) -> bool {
    let mut ledger = load();
    ledger.settings.retention_days = days;
    let now = now_unix();
    prune(&mut ledger, now);
    save(&ledger)
}

// ---------------- 날짜 (UTC, 외부 의존성 없음) ----------------

/// Unix seconds → (year, month, day) UTC. (Howard Hinnant civil_from_days)
pub fn civil_from_unix(ts: u64) -> (i64, u32, u32) {
    let days = (ts / 86_400) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `YYYY-MM-DD` (UTC).
pub fn date_string(ts: u64) -> String {
    let (y, m, d) = civil_from_unix(ts);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `YYYY-MM` (UTC).
pub fn month_string(ts: u64) -> String {
    let (y, m, _) = civil_from_unix(ts);
    format!("{y:04}-{m:02}")
}

// ---------------- 집계 ----------------

/// provider(또는 모델·기능)별 집계 한 줄. 토큰은 **알려진 값만** 합산한다.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct UsageGroup {
    pub key: String,
    pub calls: u64,
    pub success: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub policy_denied: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    /// 토큰 정보를 제공하지 않은 호출 수 — "usage unavailable"로 표시할 근거.
    pub calls_without_usage: u64,
}

impl UsageGroup {
    fn add(&mut self, r: &SessionRecord) {
        self.calls += 1;
        match r.status {
            SessionStatus::Success => self.success += 1,
            SessionStatus::Failed => self.failed += 1,
            SessionStatus::Cancelled => self.cancelled += 1,
            SessionStatus::PolicyDenied => self.policy_denied += 1,
        }
        match &r.usage {
            Some(u) => {
                self.input_tokens += u.input.unwrap_or(0);
                self.output_tokens += u.output.unwrap_or(0);
                self.cache_read_tokens += u.cache_read.unwrap_or(0);
            }
            // 정책 거부는 호출이 나가지 않았으므로 "usage 없음"으로 세지 않는다.
            None if r.status != SessionStatus::PolicyDenied => self.calls_without_usage += 1,
            None => {}
        }
    }
}

/// 기간 필터.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Day,
    Week,
    Month,
    All,
}

impl Period {
    pub fn parse(s: &str) -> Option<Period> {
        match s.trim().to_lowercase().as_str() {
            "day" | "today" => Some(Period::Day),
            "week" => Some(Period::Week),
            "month" => Some(Period::Month),
            "all" => Some(Period::All),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Period::Day => "day",
            Period::Week => "week",
            Period::Month => "month",
            Period::All => "all",
        }
    }
}

/// 기간에 포함되는 기록만 골라낸다. Month는 **달력상 같은 달**(UTC) 기준.
pub fn in_period(records: &[SessionRecord], period: Period, now: u64) -> Vec<&SessionRecord> {
    let this_month = month_string(now);
    records
        .iter()
        .filter(|r| match period {
            Period::All => true,
            Period::Day => now.saturating_sub(r.unix_ts) < 86_400,
            Period::Week => now.saturating_sub(r.unix_ts) < 7 * 86_400,
            Period::Month => month_string(r.unix_ts) == this_month,
        })
        .collect()
}

fn group_by(records: &[&SessionRecord], key: impl Fn(&SessionRecord) -> String) -> Vec<UsageGroup> {
    let mut map: BTreeMap<String, UsageGroup> = BTreeMap::new();
    for r in records {
        let k = key(r);
        let g = map.entry(k.clone()).or_insert_with(|| UsageGroup {
            key: k,
            ..Default::default()
        });
        g.add(r);
    }
    map.into_values().collect()
}

/// provider별 집계.
pub fn by_provider(records: &[&SessionRecord]) -> Vec<UsageGroup> {
    group_by(records, |r| r.provider.clone())
}

/// provider+모델별 집계 (비용 추정의 단위).
pub fn by_model(records: &[&SessionRecord]) -> Vec<UsageGroup> {
    group_by(records, |r| r.model.clone())
}

/// 기능별 집계.
pub fn by_feature(records: &[&SessionRecord]) -> Vec<UsageGroup> {
    group_by(records, |r| r.feature.clone())
}

/// 전체 합계 한 줄.
pub fn totals(records: &[&SessionRecord]) -> UsageGroup {
    let mut g = UsageGroup {
        key: "total".into(),
        ..Default::default()
    };
    for r in records {
        g.add(r);
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        ts: u64,
        provider: &str,
        model: &str,
        status: SessionStatus,
        usage: Option<TokenUsage>,
    ) -> SessionRecord {
        SessionRecord {
            id: format!("id-{ts}"),
            unix_ts: ts,
            feature: "diagnose".into(),
            provider: provider.into(),
            model: model.into(),
            scope: ContextScope::Minimal,
            status,
            duration_ms: 100,
            usage,
        }
    }

    fn usage(i: u64, o: u64) -> Option<TokenUsage> {
        Some(TokenUsage {
            input: Some(i),
            output: Some(o),
            cache_read: None,
        })
    }

    #[test]
    fn record_json_has_no_prompt_or_secret_fields() {
        // 구조적 방어: 직렬화 결과에 프롬프트/응답/키를 담는 키가 존재하지 않는다.
        let json = serde_json::to_string(&rec(
            1,
            "gpt",
            "gpt-4o",
            SessionStatus::Success,
            usage(10, 5),
        ))
        .unwrap();
        for forbidden in [
            "prompt",
            "response",
            "text",
            "evidence",
            "api_key",
            "key",
            "authorization",
            "secret",
            "content",
            "message",
        ] {
            assert!(
                !json.contains(forbidden),
                "직렬화에 금지 필드 '{forbidden}' 포함: {json}"
            );
        }
        // 기록되어야 하는 메타데이터는 존재.
        assert!(
            json.contains("provider") && json.contains("model") && json.contains("duration_ms")
        );
    }

    #[test]
    fn usage_absent_is_counted_not_invented() {
        // 토큰 정보를 안 준 호출은 0으로 합산되지 않고 '알 수 없음'으로 센다.
        let records = [
            rec(100, "gpt", "gpt-4o", SessionStatus::Success, usage(10, 5)),
            rec(200, "gpt", "gpt-4o", SessionStatus::Success, None),
        ];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let t = totals(&refs);
        assert_eq!(t.calls, 2);
        assert_eq!(t.input_tokens, 10); // 없는 건 지어내지 않음
        assert_eq!(t.calls_without_usage, 1);
    }

    #[test]
    fn policy_denied_not_counted_as_missing_usage() {
        // 정책 거부는 네트워크 호출이 없었으므로 usage 결측으로 세지 않는다.
        let records = [rec(100, "gpt", "gpt-4o", SessionStatus::PolicyDenied, None)];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let t = totals(&refs);
        assert_eq!(t.policy_denied, 1);
        assert_eq!(t.calls_without_usage, 0);
    }

    #[test]
    fn grouping_splits_providers_and_models() {
        let records = [
            rec(1, "gpt", "gpt-4o", SessionStatus::Success, usage(10, 5)),
            rec(
                2,
                "claude",
                "claude-sonnet-4-5",
                SessionStatus::Failed,
                None,
            ),
            rec(3, "gpt", "gpt-4o-mini", SessionStatus::Success, usage(7, 3)),
        ];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let p = by_provider(&refs);
        assert_eq!(p.len(), 2);
        let gpt = p.iter().find(|g| g.key == "gpt").unwrap();
        assert_eq!(gpt.calls, 2);
        assert_eq!(gpt.input_tokens, 17);
        assert_eq!(by_model(&refs).len(), 3);
        let claude = p.iter().find(|g| g.key == "claude").unwrap();
        assert_eq!(claude.failed, 1);
    }

    #[test]
    fn period_filter_uses_calendar_month() {
        // 2026-07-24 12:00 UTC 기준
        let now = 1_785_240_000;
        let same_month = now - 3 * 86_400;
        let last_month = now - 40 * 86_400;
        let records = [
            rec(same_month, "gpt", "m", SessionStatus::Success, None),
            rec(last_month, "gpt", "m", SessionStatus::Success, None),
        ];
        assert_eq!(in_period(&records, Period::Month, now).len(), 1);
        assert_eq!(in_period(&records, Period::All, now).len(), 2);
        assert_eq!(in_period(&records, Period::Day, now).len(), 0);
        assert_eq!(in_period(&records, Period::Week, now).len(), 1);
    }

    #[test]
    fn prune_drops_old_records() {
        let now = 1_000_000_000;
        let mut ledger = Ledger {
            settings: LedgerSettings {
                enabled: true,
                retention_days: 7,
            },
            records: vec![
                rec(now - 30 * 86_400, "gpt", "m", SessionStatus::Success, None),
                rec(now - 86_400, "gpt", "m", SessionStatus::Success, None),
            ],
        };
        prune(&mut ledger, now);
        assert_eq!(ledger.records.len(), 1);
    }

    #[test]
    fn civil_date_conversion_is_correct() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1));
        assert_eq!(date_string(1_785_240_000), "2026-07-28");
        assert_eq!(month_string(1_785_240_000), "2026-07");
        // 월 경계: 2026-08-01 00:00 UTC
        assert_eq!(date_string(1_785_542_400), "2026-08-01");
    }

    #[test]
    fn settings_default_to_recording_with_retention() {
        let s = LedgerSettings::default();
        assert!(s.enabled);
        assert_eq!(s.retention_days, 90);
    }
}
