//! velox-core::pricing — 버전 표기된 단가표 + **추정** 비용 계산기 (v0.17).
//!
//! 정직성 규칙(브랜드 원칙 · 절대):
//! - 여기서 계산하는 값은 **`Estimated API cost` — APEX가 기록한 사용량만** 기준이다.
//! - **소비자 구독(Claude Pro / ChatGPT Plus / Gemini Advanced)의 잔액·청구서와 무관**하며,
//!   "실시간 잔액", "남은 크레딧", "이번 달 총 AI 지출", "청구서와 일치"로 표현하지 않는다.
//! - **단가를 지어내지 않는다.** 내장 표는 **비어 있는 상태로 출고**되고, 사용자가 각 provider
//!   콘솔에서 확인한 공개 단가를 직접 넣어야 비용이 계산된다. 단가를 모르면 `unknown`이다.
//! - provider가 토큰 수를 주지 않았으면(`usage unavailable`) 그 호출의 비용도 `unknown`이다.
//! - 캐시/도구/reasoning 등 모델별 과금 구조를 다 반영하지 못할 수 있으므로, 결과에는 항상
//!   "알 수 없는 호출 수"를 함께 보고해 단일 숫자로 위장하지 않는다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ledger::{SessionRecord, SessionStatus};

/// 사용자 단가표 파일.
pub const PRICING_FILE: &str = "velox_pricing.json";

/// 단가표가 이 일수보다 오래되면 "오래된 표" 경고를 띄운다.
pub const STALE_AFTER_DAYS: u64 = 120;

/// 모델 하나의 단가 (100만 토큰당, 입력/출력/캐시 분리).
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ModelPrice {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_per_mtok: Option<f64>,
}

/// 버전이 표기된 단가표.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct PricingTable {
    /// 표 버전(사용자가 갱신할 때 올린다).
    pub version: String,
    /// 이 단가를 확인한 날짜 `YYYY-MM-DD` — 오래되면 경고한다.
    pub effective_date: String,
    pub currency: String,
    /// 단가 출처(콘솔 URL 등).
    pub source: String,
    /// 모델 ID → 단가. **기본값은 비어 있음**(단가 추측 금지).
    pub models: BTreeMap<String, ModelPrice>,
}

impl Default for PricingTable {
    fn default() -> Self {
        Self {
            version: "0".into(),
            effective_date: String::new(),
            currency: "USD".into(),
            source: "각 provider 콘솔에서 공개 단가를 확인해 입력하세요".into(),
            models: BTreeMap::new(),
        }
    }
}

impl PricingTable {
    /// 단가표가 비어 있는가 (= 아직 설정 안 됨).
    pub fn is_unconfigured(&self) -> bool {
        self.models.is_empty()
    }

    /// 표가 오래됐는지 (effective_date 기준). 날짜가 없으면 `false`.
    pub fn is_stale(&self, now_unix: u64) -> bool {
        let Some(effective) = parse_date(&self.effective_date) else {
            return false;
        };
        let now = crate::ledger::civil_from_unix(now_unix);
        days_between(effective, now) > STALE_AFTER_DAYS as i64
    }

    pub fn price_for(&self, model: &str) -> Option<&ModelPrice> {
        self.models.get(model)
    }
}

fn parse_date(s: &str) -> Option<(i64, u32, u32)> {
    let mut it = s.split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) {
        return None;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let max_day = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=max_day).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

/// 두 (y,m,d) 사이 일수 차 (b - a). (days_from_civil)
fn days_between(a: (i64, u32, u32), b: (i64, u32, u32)) -> i64 {
    days_from_civil(b) - days_from_civil(a)
}

fn days_from_civil((y, m, d): (i64, u32, u32)) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 단가표 로드. 없거나 손상되면 **빈 표**(= 비용 unknown) — permissive 추정 금지.
pub fn load() -> PricingTable {
    std::fs::read_to_string(crate::paths::resolve(PRICING_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 단가표를 원자적으로 저장.
pub fn save(table: &PricingTable) -> bool {
    serde_json::to_string_pretty(table)
        .ok()
        .and_then(|s| crate::ai::atomic_write(&crate::paths::resolve(PRICING_FILE), &s).ok())
        .is_some()
}

/// 모델 단가 설정(검증 포함). 음수·비정상 값은 거부한다.
pub fn set_price(
    model: &str,
    input_per_mtok: f64,
    output_per_mtok: f64,
    cache_read_per_mtok: Option<f64>,
    effective_date: &str,
    source: Option<&str>,
) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("모델 ID가 비어 있습니다".into());
    }
    for (label, v) in [("입력", input_per_mtok), ("출력", output_per_mtok)] {
        if !v.is_finite() || v < 0.0 {
            return Err(format!("{label} 단가가 올바르지 않습니다"));
        }
    }
    if let Some(c) = cache_read_per_mtok
        && (!c.is_finite() || c < 0.0)
    {
        return Err("캐시 단가가 올바르지 않습니다".into());
    }
    if parse_date(effective_date).is_none() {
        return Err("확인 날짜는 YYYY-MM-DD 형식이어야 합니다".into());
    }
    let mut table = load();
    table.models.insert(
        model.to_string(),
        ModelPrice {
            input_per_mtok,
            output_per_mtok,
            cache_read_per_mtok,
        },
    );
    table.effective_date = effective_date.to_string();
    if let Some(src) = source {
        table.source = src.to_string();
    }
    // 표가 바뀌었으니 버전을 올린다(단조 증가).
    table.version = (table.version.parse::<u64>().unwrap_or(0) + 1).to_string();
    if save(&table) {
        Ok(())
    } else {
        Err("단가표 저장 실패".into())
    }
}

/// 모델 단가 삭제.
pub fn remove_price(model: &str) -> bool {
    let mut table = load();
    table.models.remove(model);
    save(&table)
}

/// **추정** 비용 결과. 단일 숫자로 위장하지 않도록 '알 수 없는 호출'을 함께 담는다.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct CostEstimate {
    /// 단가와 토큰이 **둘 다 있는** 호출만 합산한 금액.
    pub known_cost: f64,
    pub currency: String,
    /// 비용을 계산한 호출 수.
    pub priced_calls: u64,
    /// 토큰은 있으나 **단가가 없어** 계산 못 한 호출 수.
    pub calls_missing_price: u64,
    /// provider가 **토큰을 주지 않아** 계산 못 한 호출 수.
    pub calls_missing_usage: u64,
    /// 단가가 없는 모델 목록(사용자에게 무엇을 넣어야 하는지 알려준다).
    pub models_missing_price: Vec<String>,
    /// 단가표가 아예 설정되지 않음.
    pub pricing_unconfigured: bool,
    /// 단가표가 오래됨.
    pub pricing_stale: bool,
    pub pricing_version: String,
    pub pricing_effective_date: String,
}

impl CostEstimate {
    /// 비용을 신뢰할 수 있게 보여줄 수 있는가 — 하나라도 결측이면 부분값임을 표시해야 한다.
    pub fn is_complete(&self) -> bool {
        !self.pricing_unconfigured
            && self.calls_missing_price == 0
            && self.calls_missing_usage == 0
            && self.priced_calls > 0
    }

    /// 화면에 쓸 금액 문자열. 계산 가능한 게 없으면 `unknown`.
    /// 아주 작은 금액을 `0.0000`으로 반올림해 "안 썼다"처럼 보이지 않게 정밀도를 높인다.
    pub fn display(&self) -> String {
        if self.priced_calls == 0 {
            return "unknown".into();
        }
        let amount = if self.known_cost > 0.0 && self.known_cost < 0.01 {
            format!("{} {:.6}", self.currency, self.known_cost)
        } else {
            format!("{} {:.4}", self.currency, self.known_cost)
        };
        if self.is_complete() {
            amount
        } else {
            // 부분 집계임을 명시 — 단일 숫자로 위장하지 않는다.
            format!("≥ {amount} (부분)")
        }
    }
}

/// 기록들의 **추정** 비용. 단가·토큰이 모두 있는 호출만 더하고, 나머지는 결측으로 보고한다.
pub fn estimate(records: &[&SessionRecord], table: &PricingTable, now_unix: u64) -> CostEstimate {
    let mut est = CostEstimate {
        currency: table.currency.clone(),
        pricing_unconfigured: table.is_unconfigured(),
        pricing_stale: table.is_stale(now_unix),
        pricing_version: table.version.clone(),
        pricing_effective_date: table.effective_date.clone(),
        ..Default::default()
    };
    let mut missing: std::collections::BTreeSet<String> = Default::default();
    for r in records {
        // 정책 거부는 호출이 나가지 않았으므로 비용 계산 대상이 아니다.
        if r.status == SessionStatus::PolicyDenied {
            continue;
        }
        let Some(usage) = r.usage.as_ref().filter(|u| !u.is_empty()) else {
            est.calls_missing_usage += 1;
            continue;
        };
        let Some(price) = table.price_for(&r.model) else {
            est.calls_missing_price += 1;
            missing.insert(r.model.clone());
            continue;
        };
        let per_mtok = |tokens: u64, rate: f64| tokens as f64 / 1_000_000.0 * rate;
        let total_input = usage.input.unwrap_or(0);
        let input_cost = match (usage.cache_read, price.cache_read_per_mtok) {
            (Some(cached), Some(cache_rate)) => {
                // OpenAI/Gemini report cached tokens as a subset of total input.
                // Charge that subset at the cache rate instead of charging it
                // once at the normal rate and again at the cache rate.
                let uncached = total_input.saturating_sub(cached);
                per_mtok(uncached, price.input_per_mtok) + per_mtok(cached, cache_rate)
            }
            (Some(cached), None) if cached > 0 => {
                // A cache hit with no cache price cannot be represented
                // honestly. Exclude the whole call instead of fabricating a
                // normal-input or zero-cost assumption.
                est.calls_missing_price += 1;
                missing.insert(r.model.clone());
                continue;
            }
            _ => per_mtok(total_input, price.input_per_mtok),
        };
        let cost = input_cost + per_mtok(usage.output.unwrap_or(0), price.output_per_mtok);
        est.known_cost += cost;
        est.priced_calls += 1;
    }
    est.models_missing_price = missing.into_iter().collect();
    est
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::TokenUsage;
    use crate::privacy::ContextScope;

    fn rec(model: &str, usage: Option<TokenUsage>, status: SessionStatus) -> SessionRecord {
        SessionRecord {
            id: "x".into(),
            unix_ts: 1_785_240_000,
            feature: "diagnose".into(),
            provider: "gpt".into(),
            model: model.into(),
            scope: ContextScope::Minimal,
            status,
            duration_ms: 10,
            usage,
        }
    }

    fn tokens(i: u64, o: u64) -> Option<TokenUsage> {
        Some(TokenUsage {
            input: Some(i),
            output: Some(o),
            cache_read: None,
        })
    }

    fn table_with(model: &str, inp: f64, out: f64) -> PricingTable {
        let mut t = PricingTable::default();
        t.models.insert(
            model.into(),
            ModelPrice {
                input_per_mtok: inp,
                output_per_mtok: out,
                cache_read_per_mtok: None,
            },
        );
        t.effective_date = "2026-07-24".into();
        t
    }

    #[test]
    fn default_table_is_empty_no_invented_prices() {
        // 단가를 지어내지 않는다 — 기본 표는 비어 있어야 한다.
        let t = PricingTable::default();
        assert!(t.is_unconfigured());
        assert!(t.models.is_empty());
    }

    #[test]
    fn unconfigured_pricing_yields_unknown() {
        let records = [rec("gpt-4o", tokens(1000, 500), SessionStatus::Success)];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &PricingTable::default(), 1_785_240_000);
        assert!(est.pricing_unconfigured);
        assert_eq!(est.priced_calls, 0);
        assert_eq!(est.calls_missing_price, 1);
        assert_eq!(est.display(), "unknown"); // 숫자를 지어내지 않음
    }

    #[test]
    fn missing_usage_is_not_estimated() {
        let records = [rec("gpt-4o", None, SessionStatus::Success)];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &table_with("gpt-4o", 1.0, 2.0), 1_785_240_000);
        assert_eq!(est.calls_missing_usage, 1);
        assert_eq!(est.priced_calls, 0);
        assert_eq!(est.display(), "unknown");
    }

    #[test]
    fn known_price_and_usage_compute_cost() {
        // 1M 입력 @ $1, 0.5M 출력 @ $2 = 1.0 + 1.0 = 2.0
        let records = [rec(
            "gpt-4o",
            tokens(1_000_000, 500_000),
            SessionStatus::Success,
        )];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &table_with("gpt-4o", 1.0, 2.0), 1_785_240_000);
        assert!((est.known_cost - 2.0).abs() < 1e-9);
        assert_eq!(est.priced_calls, 1);
        assert!(est.is_complete());
        assert!(est.display().starts_with("USD 2.0"));
    }

    #[test]
    fn partial_estimate_is_marked_not_a_single_number() {
        let records = [
            rec("gpt-4o", tokens(1_000_000, 0), SessionStatus::Success),
            rec("unknown-model", tokens(500, 500), SessionStatus::Success),
        ];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &table_with("gpt-4o", 1.0, 2.0), 1_785_240_000);
        assert_eq!(est.priced_calls, 1);
        assert_eq!(est.calls_missing_price, 1);
        assert_eq!(est.models_missing_price, vec!["unknown-model".to_string()]);
        assert!(!est.is_complete());
        assert!(est.display().contains("부분")); // 부분 집계임을 표시
    }

    #[test]
    fn policy_denied_excluded_from_cost() {
        let records = [rec("gpt-4o", None, SessionStatus::PolicyDenied)];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &table_with("gpt-4o", 1.0, 2.0), 1_785_240_000);
        assert_eq!(est.calls_missing_usage, 0);
        assert_eq!(est.priced_calls, 0);
    }

    #[test]
    fn stale_pricing_is_flagged() {
        let mut t = table_with("gpt-4o", 1.0, 2.0);
        t.effective_date = "2026-01-01".into();
        // 2026-07-24 기준 200일 경과 > 120일 → stale
        assert!(t.is_stale(1_785_240_000));
        t.effective_date = "2026-07-01".into();
        assert!(!t.is_stale(1_785_240_000));
    }

    #[test]
    fn set_price_rejects_bad_input() {
        assert!(set_price("", 1.0, 2.0, None, "2026-07-24", None).is_err());
        assert!(set_price("m", -1.0, 2.0, None, "2026-07-24", None).is_err());
        assert!(set_price("m", 1.0, 2.0, None, "24/07/2026", None).is_err());
        assert!(set_price("m", 1.0, 2.0, None, "2026-02-30", None).is_err());
        assert!(set_price("m", 1.0, 2.0, None, "2026-13-01", None).is_err());
        assert!(set_price("m", f64::NAN, 2.0, None, "2026-07-24", None).is_err());
    }

    #[test]
    fn cached_input_replaces_normal_input_cost_instead_of_double_counting() {
        let mut t = table_with("gpt-4o", 10.0, 20.0);
        t.models.get_mut("gpt-4o").unwrap().cache_read_per_mtok = Some(1.0);
        let records = [rec(
            "gpt-4o",
            Some(TokenUsage {
                input: Some(1_000_000),
                output: Some(100_000),
                cache_read: Some(800_000),
            }),
            SessionStatus::Success,
        )];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &t, 1_785_240_000);

        // 0.2M uncached * $10 + 0.8M cached * $1 + 0.1M output * $20 = $4.8.
        assert!((est.known_cost - 4.8).abs() < 1e-9);
        assert!(est.is_complete());
    }

    #[test]
    fn cache_hit_without_cache_price_is_not_silently_estimated() {
        let records = [rec(
            "gpt-4o",
            Some(TokenUsage {
                input: Some(1_000),
                output: Some(100),
                cache_read: Some(800),
            }),
            SessionStatus::Success,
        )];
        let refs: Vec<&SessionRecord> = records.iter().collect();
        let est = estimate(&refs, &table_with("gpt-4o", 10.0, 20.0), 1_785_240_000);

        assert_eq!(est.priced_calls, 0);
        assert_eq!(est.calls_missing_price, 1);
        assert_eq!(est.display(), "unknown");
    }

    #[test]
    fn date_math_matches_calendar() {
        assert_eq!(days_between((2026, 1, 1), (2026, 1, 31)), 30);
        assert_eq!(days_between((2026, 1, 1), (2027, 1, 1)), 365);
        assert_eq!(parse_date("2024-02-29"), Some((2024, 2, 29)));
        assert_eq!(parse_date("2026-02-29"), None);
    }
}
