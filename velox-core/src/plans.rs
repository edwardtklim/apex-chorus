//! velox-core::plans — AI 구독·API 계정 기록 (마스터 플랜 4장 "Keys, Plans & Purpose").
//!
//! 사용자는 AI 마다 플랜·금액·갱신일·주 용도를 따로 기억하고 있다. 이 모듈은 그것을
//! 한 곳에 적어 두고 "언제 얼마가 나가는지", "어떤 AI 를 무엇에 쓰는지"에 답한다.
//!
//! 설계에서 양보하지 않는 것:
//!
//! 1. **키를 담을 칸이 없다.** 이 파일에는 API 키 필드가 존재하지 않는다. 키는
//!    [`crate::credentials`] (OS 비밀 저장소)에만 있다. 자유 입력 칸(용도·메모·플랜명)도
//!    [`crate::project::redact_secrets`] 를 통과한 뒤 저장한다.
//! 2. **구독과 API 를 섞어 더하지 않는다.** 소비자 구독은 정해진 금액이고, API 는
//!    사용량에 따라 달라지는 **추정** 비용이다. 한 숫자로 합치면 같은 돈을 두 번 세거나
//!    추정을 확정처럼 보이게 된다. 월 합계에는 구독만 들어간다.
//! 3. **모르면 모른다고 한다.** 금액이 없는 항목이 있으면 합계가 부분값임을 알린다.
//!    단가가 없으면 예산 대비 사용률을 지어내지 않고 `Unknown` 으로 둔다.
//! 4. **지난 갱신일을 조용히 고치지 않는다.** 입력한 날짜가 지났으면 주기로 다음 날짜를
//!    계산하되 `derived = true` 로 표시한다 — 사용자가 확인한 날짜와 구분된다.
//! 5. **기록만 한다.** 결제를 실행하거나 구독을 바꾸지 않는다. 자동 조회도 하지 않는다 —
//!    모든 값은 사용자가 직접 입력한 것이고 [`PlanEntry::confirmed_at`] 에 확인 시점이 남는다.
//! 6. **손상된 파일을 덮어쓰지 않는다.** 읽기에 실패하면 저장을 거부한다. 사용자가
//!    손으로 넣은 기록을 조용히 날리는 것이 가장 나쁜 실패다.
//!
//! 날짜는 장부([`crate::ledger`])와 같은 **UTC 기준**이다. 현지 날짜와 하루 차이가 날 수 있다.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const PLANS_FILE: &str = "velox_plans.json";
/// 예산의 이 비율을 넘으면 "임박"으로 본다.
pub const BUDGET_NEAR_PCT: f64 = 80.0;

// ---------------- 날짜 ----------------

/// 달력 날짜. 시간대 없이 연·월·일만 다룬다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i64,
    pub m: u32,
    pub d: u32,
}

impl Date {
    /// `YYYY-MM-DD` 만 받는다. 존재하지 않는 날짜(2월 30일 등)는 거부한다.
    pub fn parse(s: &str) -> Option<Date> {
        let mut it = s.trim().split('-');
        let (y, m, d) = (it.next()?, it.next()?, it.next()?);
        if it.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
            return None;
        }
        let date = Date {
            y: y.parse().ok()?,
            m: m.parse().ok()?,
            d: d.parse().ok()?,
        };
        if date.m == 0 || date.m > 12 || date.d == 0 || date.d > days_in_month(date.y, date.m) {
            return None;
        }
        Some(date)
    }

    /// 1970-01-01 부터의 일수. (Howard Hinnant days_from_civil)
    pub fn to_days(self) -> i64 {
        let y = if self.m <= 2 { self.y - 1 } else { self.y };
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (self.m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + self.d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// `months` 개월 뒤. 말일을 넘으면 그 달의 말일로 맞춘다(1/31 + 1개월 = 2/28 또는 2/29).
    pub fn add_months(self, months: i64) -> Date {
        let total = self.y * 12 + (self.m as i64 - 1) + months;
        let y = total.div_euclid(12);
        let m = (total.rem_euclid(12) + 1) as u32;
        Date {
            y,
            m,
            d: self.d.min(days_in_month(y, m)),
        }
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        _ => 28,
    }
}

/// 오늘(UTC). 장부와 같은 시계를 쓴다.
pub fn today_utc() -> Date {
    let (y, m, d) = crate::ledger::civil_from_unix(crate::ledger::now_unix());
    Date { y, m, d }
}

// ---------------- 데이터 ----------------

/// 소비자 구독과 API 계정은 **다른 종류의 돈**이다. 반드시 따로 기록한다.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    /// ChatGPT Plus, Claude Max 같은 정액 구독.
    Subscription,
    /// 사용량만큼 내는 API 계정.
    Api,
}

impl PlanKind {
    pub fn parse(s: &str) -> Option<PlanKind> {
        match s.trim().to_lowercase().as_str() {
            "subscription" | "sub" | "구독" => Some(PlanKind::Subscription),
            "api" => Some(PlanKind::Api),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PlanKind::Subscription => "구독",
            PlanKind::Api => "API",
        }
    }
    fn slug(self) -> &'static str {
        match self {
            PlanKind::Subscription => "subscription",
            PlanKind::Api => "api",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cycle {
    Monthly,
    Yearly,
    /// 정해진 주기 없음(종량제 등). 갱신일을 가질 수 없다.
    #[default]
    None,
}

impl Cycle {
    pub fn parse(s: &str) -> Option<Cycle> {
        match s.trim().to_lowercase().as_str() {
            "monthly" | "month" | "월" => Some(Cycle::Monthly),
            "yearly" | "year" | "annual" | "연" => Some(Cycle::Yearly),
            "none" | "" => Some(Cycle::None),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Cycle::Monthly => "월",
            Cycle::Yearly => "연",
            Cycle::None => "-",
        }
    }
    fn months(self) -> Option<i64> {
        match self {
            Cycle::Monthly => Some(1),
            Cycle::Yearly => Some(12),
            Cycle::None => None,
        }
    }
}

/// 값의 출처. 지금은 수동 입력뿐이다 — 자동 조회를 붙이면 여기에 추가한다.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Manual,
}

/// 기록 한 건. **API 키 필드는 없다 — 의도된 설계다.**
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct PlanEntry {
    pub id: String,
    pub provider: String,
    pub kind: PlanKind,
    /// 플랜 이름(Plus, Max, Pay-as-you-go 등).
    #[serde(default)]
    pub plan: String,
    /// 주기당 금액. 모르면 None — 0 으로 채우지 않는다.
    #[serde(default)]
    pub amount: Option<f64>,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub cycle: Cycle,
    /// 사용자가 입력한 다음 갱신일(`YYYY-MM-DD`).
    #[serde(default)]
    pub renews_on: Option<String>,
    /// 이 AI 를 주로 무엇에 쓰는지 — 사용자가 정한다. 제공자별로 고정하지 않는다.
    #[serde(default)]
    pub purpose: String,
    /// API 계정의 월 예산(경고 기준). 결제를 막지 않는다 — 알려주기만 한다.
    #[serde(default)]
    pub monthly_budget: Option<f64>,
    #[serde(default)]
    pub source: Source,
    /// 이 값들을 사용자가 마지막으로 입력·확인한 시각.
    #[serde(default)]
    pub confirmed_at: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PlanBook {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<PlanEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    Invalid(String),
    NotFound(String),
    /// 파일은 있는데 읽을 수 없다. **덮어쓰지 않는다.**
    Corrupt(String),
    Io(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::Invalid(m) => write!(f, "{m}"),
            PlanError::NotFound(id) => write!(
                f,
                "그런 기록이 없습니다: {id}\n  다음 행동: `velox plans list` 로 id 를 확인하세요."
            ),
            PlanError::Corrupt(e) => write!(
                f,
                "구독 기록 파일을 읽을 수 없습니다: {e}\n  다음 행동: 기록을 보호하려고 저장을 중단했습니다. {PLANS_FILE} 을 열어 고치거나 다른 이름으로 옮긴 뒤 다시 시도하세요."
            ),
            PlanError::Io(e) => write!(
                f,
                "구독 기록을 저장하지 못했습니다: {e}\n  다음 행동: 디스크 공간과 권한을 확인하세요."
            ),
        }
    }
}

fn path() -> PathBuf {
    crate::paths::resolve(PLANS_FILE)
}

pub fn load() -> Result<PlanBook, PlanError> {
    load_from(&path())
}

fn load_from(p: &std::path::Path) -> Result<PlanBook, PlanError> {
    match std::fs::read_to_string(p) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| PlanError::Corrupt(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PlanBook::default()),
        Err(e) => Err(PlanError::Io(e.to_string())),
    }
}

fn save_to(p: &std::path::Path, book: &PlanBook) -> Result<(), PlanError> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| PlanError::Io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(book).map_err(|e| PlanError::Io(e.to_string()))?;
    // 원자적 저장: tmp → rename.
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| PlanError::Io(e.to_string()))?;
    std::fs::rename(&tmp, p).map_err(|e| PlanError::Io(e.to_string()))
}

/// 사용자가 입력한 값 묶음. 검증 전 상태다.
#[derive(Clone, Debug, Default)]
pub struct NewPlan {
    pub id: Option<String>,
    pub provider: String,
    pub kind: Option<PlanKind>,
    pub plan: String,
    pub amount: Option<f64>,
    pub currency: String,
    pub cycle: Cycle,
    pub renews_on: Option<String>,
    pub purpose: String,
    pub monthly_budget: Option<f64>,
    pub note: String,
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(48).collect()
}

fn clean(s: &str, max: usize) -> String {
    crate::project::redact_secrets(s.trim())
        .chars()
        .take(max)
        .collect()
}

fn money_ok(v: f64) -> bool {
    v.is_finite() && (0.0..=1_000_000.0).contains(&v)
}

/// 입력을 검증해 저장 가능한 항목으로 만든다. 디스크를 건드리지 않는다.
pub fn build(input: NewPlan, now_rfc3339: &str) -> Result<PlanEntry, PlanError> {
    let invalid = |m: &str| Err(PlanError::Invalid(m.to_string()));

    let provider = clean(&input.provider, 40);
    if provider.is_empty() {
        return invalid(
            "제공자 이름이 비었습니다.\n  다음 행동: --provider claude 처럼 지정하세요.",
        );
    }
    let Some(kind) = input.kind else {
        return invalid(
            "종류가 없습니다.\n  다음 행동: --kind subscription 또는 --kind api 를 지정하세요. (정액 구독과 API 는 따로 기록합니다)",
        );
    };

    if let Some(a) = input.amount
        && !money_ok(a)
    {
        return invalid("금액은 0 이상 1,000,000 이하의 숫자여야 합니다.");
    }
    if let Some(b) = input.monthly_budget {
        if !money_ok(b) || b == 0.0 {
            return invalid("예산은 0 보다 큰 숫자여야 합니다.");
        }
        if kind != PlanKind::Api {
            return invalid(
                "예산(--budget)은 API 계정에만 씁니다.\n  이유: 정액 구독은 금액이 이미 정해져 있습니다.",
            );
        }
    }

    let currency = input.currency.trim().to_uppercase();
    let needs_currency = input.amount.is_some() || input.monthly_budget.is_some();
    if needs_currency && !(currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase()))
    {
        return invalid(
            "통화는 USD·KRW 같은 3글자 코드여야 합니다.\n  다음 행동: --currency USD 처럼 지정하세요.",
        );
    }

    let renews_on = match input.renews_on.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => match Date::parse(raw) {
            Some(d) => Some(d.to_string()),
            None => {
                return invalid(
                    "갱신일을 읽을 수 없습니다.\n  다음 행동: --renews 2026-11-03 처럼 YYYY-MM-DD 로 입력하세요.",
                );
            }
        },
    };
    if renews_on.is_some() && input.cycle == Cycle::None {
        return invalid(
            "갱신일이 있으면 주기도 필요합니다.\n  다음 행동: --cycle monthly 또는 --cycle yearly 를 함께 지정하세요.",
        );
    }

    let id = match input.id.as_deref().map(slugify) {
        Some(s) if !s.is_empty() => s,
        Some(_) => return invalid("id 는 영문·숫자·하이픈으로 지정하세요."),
        None => {
            let base = slugify(&provider);
            if base.is_empty() {
                return invalid(
                    "제공자 이름으로 id 를 만들 수 없습니다.\n  다음 행동: --id my-plan 처럼 영문 id 를 직접 지정하세요.",
                );
            }
            format!("{base}-{}", kind.slug())
        }
    };

    Ok(PlanEntry {
        id,
        provider,
        kind,
        plan: clean(&input.plan, 60),
        amount: input.amount,
        currency,
        cycle: input.cycle,
        renews_on,
        purpose: clean(&input.purpose, 200),
        monthly_budget: input.monthly_budget,
        source: Source::Manual,
        confirmed_at: now_rfc3339.to_string(),
        note: clean(&input.note, 300),
    })
}

/// 추가하거나 같은 id 를 교체한다. (true = 새로 추가, false = 교체)
pub fn upsert(input: NewPlan) -> Result<(PlanEntry, bool), PlanError> {
    upsert_at(&path(), input, &crate::util::now_rfc3339())
}

fn upsert_at(
    p: &std::path::Path,
    input: NewPlan,
    now: &str,
) -> Result<(PlanEntry, bool), PlanError> {
    let entry = build(input, now)?;
    // 손상된 파일이면 여기서 멈춘다 — 빈 장부로 덮어쓰지 않는다.
    let mut book = load_from(p)?;
    let added = match book.entries.iter_mut().find(|e| e.id == entry.id) {
        Some(slot) => {
            *slot = entry.clone();
            false
        }
        None => {
            book.entries.push(entry.clone());
            true
        }
    };
    book.version = 1;
    save_to(p, &book)?;
    Ok((entry, added))
}

pub fn remove(id: &str) -> Result<PlanEntry, PlanError> {
    remove_at(&path(), id)
}

fn remove_at(p: &std::path::Path, id: &str) -> Result<PlanEntry, PlanError> {
    let mut book = load_from(p)?;
    let Some(i) = book.entries.iter().position(|e| e.id == id) else {
        return Err(PlanError::NotFound(id.to_string()));
    };
    let gone = book.entries.remove(i);
    save_to(p, &book)?;
    Ok(gone)
}

/// "이 값이 아직 맞다"고 사용자가 확인한다. 갱신일을 함께 고칠 수 있다.
pub fn confirm(id: &str, renews_on: Option<&str>) -> Result<PlanEntry, PlanError> {
    confirm_at(&path(), id, renews_on, &crate::util::now_rfc3339())
}

fn confirm_at(
    p: &std::path::Path,
    id: &str,
    renews_on: Option<&str>,
    now: &str,
) -> Result<PlanEntry, PlanError> {
    let mut book = load_from(p)?;
    let Some(e) = book.entries.iter_mut().find(|e| e.id == id) else {
        return Err(PlanError::NotFound(id.to_string()));
    };
    if let Some(raw) = renews_on {
        let Some(d) = Date::parse(raw) else {
            return Err(PlanError::Invalid(
                "갱신일을 읽을 수 없습니다.\n  다음 행동: --renews 2026-11-03 처럼 YYYY-MM-DD 로 입력하세요.".into(),
            ));
        };
        if e.cycle == Cycle::None {
            return Err(PlanError::Invalid(
                "이 기록에는 주기가 없어 갱신일을 넣을 수 없습니다.\n  다음 행동: `velox plans add` 로 --cycle 을 포함해 다시 등록하세요.".into(),
            ));
        }
        e.renews_on = Some(d.to_string());
    }
    e.confirmed_at = now.to_string();
    let out = e.clone();
    save_to(p, &book)?;
    Ok(out)
}

// ---------------- 갱신 ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct NextRenewal {
    #[serde(serialize_with = "ser_date")]
    pub date: Date,
    /// 오늘부터 며칠 남았는지(0 = 오늘).
    pub days_left: i64,
    /// true 면 입력한 날짜가 이미 지나 **주기로 계산한 추정**이다. 사용자가 확인한 값이 아니다.
    pub derived: bool,
}

fn ser_date<S: serde::Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&d.to_string())
}

/// 다음 갱신일. 갱신일이 없거나 주기가 없으면 None.
pub fn next_renewal(e: &PlanEntry, today: Date) -> Option<NextRenewal> {
    let start = Date::parse(e.renews_on.as_deref()?)?;
    if start >= today {
        return Some(NextRenewal {
            date: start,
            days_left: start.to_days() - today.to_days(),
            derived: false,
        });
    }
    let step = e.cycle.months()?;
    // 원래 날짜에서 k 주기 뒤를 직접 계산한다 — 반복해서 더하면 말일 보정이 누적돼 날짜가 밀린다.
    let months_apart = (today.y - start.y) * 12 + (today.m as i64 - start.m as i64);
    let mut k = (months_apart / step).max(1);
    while start.add_months(k * step) < today {
        k += 1;
    }
    let date = start.add_months(k * step);
    Some(NextRenewal {
        date,
        days_left: date.to_days() - today.to_days(),
        derived: true,
    })
}

/// `within_days` 안에 갱신되는 항목. 가까운 순.
pub fn upcoming(book: &PlanBook, today: Date, within_days: i64) -> Vec<(&PlanEntry, NextRenewal)> {
    let mut out: Vec<(&PlanEntry, NextRenewal)> = book
        .entries
        .iter()
        .filter_map(|e| next_renewal(e, today).map(|n| (e, n)))
        .filter(|(_, n)| n.days_left <= within_days)
        .collect();
    out.sort_by_key(|(_, n)| n.days_left);
    out
}

// ---------------- 합계 ----------------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CurrencyTotal {
    pub currency: String,
    /// 정액 구독의 월 환산 합계(연 구독은 12 로 나눈다).
    pub monthly: f64,
    pub entries: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Totals {
    /// 통화별 — 환율을 지어내지 않으므로 서로 더하지 않는다.
    pub by_currency: Vec<CurrencyTotal>,
    /// 금액이나 주기가 없어 합계에 **못 넣은** 구독 수. 0 이 아니면 합계는 부분값이다.
    pub subscriptions_without_amount: usize,
    /// API 계정 수 — 합계에 넣지 않는다(사용량에 따른 추정 비용이라 성격이 다르다).
    pub api_accounts: usize,
}

impl Totals {
    pub fn is_complete(&self) -> bool {
        self.subscriptions_without_amount == 0
    }
}

/// 월 고정 지출. **구독만** 더한다 — API 는 사용량 추정이라 여기에 섞지 않는다.
pub fn totals(book: &PlanBook) -> Totals {
    let mut t = Totals::default();
    for e in &book.entries {
        if e.kind == PlanKind::Api {
            t.api_accounts += 1;
            continue;
        }
        let monthly = match (e.amount, e.cycle) {
            (Some(a), Cycle::Monthly) => a,
            (Some(a), Cycle::Yearly) => a / 12.0,
            _ => {
                t.subscriptions_without_amount += 1;
                continue;
            }
        };
        match t.by_currency.iter_mut().find(|c| c.currency == e.currency) {
            Some(c) => {
                c.monthly += monthly;
                c.entries += 1;
            }
            None => t.by_currency.push(CurrencyTotal {
                currency: e.currency.clone(),
                monthly,
                entries: 1,
            }),
        }
    }
    t.by_currency.sort_by(|a, b| a.currency.cmp(&b.currency));
    t
}

// ---------------- API 예산 ----------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum BudgetStatus {
    /// 예산을 정하지 않았다.
    NoBudget,
    /// 사용률을 계산할 수 없다 — 이유를 함께 준다. **0% 로 보여주지 않는다.**
    Unknown { reason: String },
    Within {
        spent: f64,
        budget: f64,
        pct: f64,
        partial: bool,
    },
    Near {
        spent: f64,
        budget: f64,
        pct: f64,
        partial: bool,
    },
    Over {
        spent: f64,
        budget: f64,
        pct: f64,
        partial: bool,
    },
}

/// 이번 달 **추정** 사용액과 예산을 비교한다.
///
/// `partial` 은 단가·토큰이 빠진 호출이 있어 실제 사용액이 더 클 수 있다는 뜻이다.
pub fn budget_status(e: &PlanEntry, est: &crate::pricing::CostEstimate) -> BudgetStatus {
    let Some(budget) = e.monthly_budget else {
        return BudgetStatus::NoBudget;
    };
    if est.pricing_unconfigured {
        return BudgetStatus::Unknown {
            reason: "단가표가 없습니다. `velox usage pricing set` 으로 단가를 넣으면 계산됩니다"
                .into(),
        };
    }
    if est.priced_calls == 0 {
        return BudgetStatus::Unknown {
            reason: "이번 달에 비용을 계산할 수 있는 호출이 없습니다".into(),
        };
    }
    if !est.currency.eq_ignore_ascii_case(&e.currency) {
        return BudgetStatus::Unknown {
            reason: format!(
                "예산 통화({})와 단가표 통화({})가 달라 비교하지 않습니다",
                e.currency, est.currency
            ),
        };
    }
    let spent = est.known_cost;
    let pct = spent / budget * 100.0;
    let partial = !est.is_complete();
    if pct >= 100.0 {
        BudgetStatus::Over {
            spent,
            budget,
            pct,
            partial,
        }
    } else if pct >= BUDGET_NEAR_PCT {
        BudgetStatus::Near {
            spent,
            budget,
            pct,
            partial,
        }
    } else {
        BudgetStatus::Within {
            spent,
            budget,
            pct,
            partial,
        }
    }
}

/// 이 제공자의 이번 달(UTC) 추정 비용. 장부·단가표를 읽는다.
pub fn month_estimate_for(provider: &str) -> crate::pricing::CostEstimate {
    let ledger = crate::ledger::load();
    let now = crate::ledger::now_unix();
    let records: Vec<&crate::ledger::SessionRecord> =
        crate::ledger::in_period(&ledger.records, crate::ledger::Period::Month, now)
            .into_iter()
            .filter(|r| r.provider.eq_ignore_ascii_case(provider))
            .collect();
    crate::pricing::estimate(&records, &crate::pricing::load(), now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn sub(provider: &str, amount: Option<f64>, cycle: Cycle, renews: Option<&str>) -> NewPlan {
        NewPlan {
            provider: provider.into(),
            kind: Some(PlanKind::Subscription),
            plan: "Pro".into(),
            amount,
            currency: "USD".into(),
            cycle,
            renews_on: renews.map(str::to_string),
            ..Default::default()
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "velox-plans-{tag}-{}-{}",
            std::process::id(),
            crate::ledger::now_unix()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(PLANS_FILE)
    }

    #[test]
    fn date_parsing_rejects_impossible_dates() {
        assert!(Date::parse("2026-02-28").is_some());
        assert!(Date::parse("2024-02-29").is_some(), "윤년");
        for bad in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "26-01-01",
            "2026/01/01",
            "2026-1-1",
            "",
            "내일",
        ] {
            assert!(Date::parse(bad).is_none(), "{bad} 는 거부돼야 한다");
        }
    }

    #[test]
    fn day_arithmetic_matches_the_calendar() {
        assert_eq!(d("1970-01-01").to_days(), 0);
        assert_eq!(d("2026-10-05").to_days() - d("2026-10-01").to_days(), 4);
        assert_eq!(
            d("2024-03-01").to_days() - d("2024-02-28").to_days(),
            2,
            "윤년"
        );
        assert_eq!(d("2026-03-01").to_days() - d("2026-02-28").to_days(), 1);
        assert_eq!(d("2027-01-01").to_days() - d("2026-01-01").to_days(), 365);
    }

    #[test]
    fn adding_months_clamps_to_month_end() {
        assert_eq!(d("2026-01-31").add_months(1), d("2026-02-28"));
        assert_eq!(d("2024-01-31").add_months(1), d("2024-02-29"));
        assert_eq!(d("2026-11-15").add_months(3), d("2027-02-15"));
        assert_eq!(d("2026-12-31").add_months(12), d("2027-12-31"));
    }

    #[test]
    fn future_renewal_is_reported_as_entered() {
        let e = build(
            sub("claude", Some(20.0), Cycle::Monthly, Some("2026-10-20")),
            "t",
        )
        .unwrap();
        let n = next_renewal(&e, d("2026-10-05")).unwrap();
        assert_eq!(n.date, d("2026-10-20"));
        assert_eq!(n.days_left, 15);
        assert!(!n.derived, "사용자가 넣은 날짜 그대로다");

        let today = next_renewal(&e, d("2026-10-20")).unwrap();
        assert_eq!(today.days_left, 0);
        assert!(!today.derived);
    }

    /// 지난 갱신일은 주기로 굴리되, **추정임을 표시**한다.
    #[test]
    fn past_renewal_rolls_forward_and_is_marked_derived() {
        let e = build(
            sub("gpt", Some(20.0), Cycle::Monthly, Some("2026-07-10")),
            "t",
        )
        .unwrap();
        let n = next_renewal(&e, d("2026-10-05")).unwrap();
        assert_eq!(n.date, d("2026-10-10"));
        assert_eq!(n.days_left, 5);
        assert!(n.derived);

        let y = build(
            sub("x", Some(100.0), Cycle::Yearly, Some("2024-03-01")),
            "t",
        )
        .unwrap();
        let n = next_renewal(&y, d("2026-10-05")).unwrap();
        assert_eq!(n.date, d("2027-03-01"));
        assert!(n.derived);
    }

    /// 31일 결제는 2월을 지나도 다시 31일로 돌아와야 한다(말일 보정이 누적되면 안 된다).
    #[test]
    fn month_end_anchor_does_not_drift() {
        let e = build(sub("a", Some(1.0), Cycle::Monthly, Some("2026-01-31")), "t").unwrap();
        assert_eq!(
            next_renewal(&e, d("2026-02-10")).unwrap().date,
            d("2026-02-28")
        );
        assert_eq!(
            next_renewal(&e, d("2026-03-05")).unwrap().date,
            d("2026-03-31"),
            "2월에 28일로 맞췄다고 3월까지 28일이 되면 안 된다"
        );
    }

    #[test]
    fn entries_without_a_date_have_no_renewal() {
        let e = build(sub("a", Some(1.0), Cycle::Monthly, None), "t").unwrap();
        assert!(next_renewal(&e, d("2026-10-05")).is_none());
    }

    #[test]
    fn upcoming_filters_by_window_and_sorts_soonest_first() {
        let book = PlanBook {
            version: 1,
            entries: vec![
                build(
                    sub("far", Some(1.0), Cycle::Monthly, Some("2026-11-20")),
                    "t",
                )
                .unwrap(),
                build(
                    sub("soon", Some(1.0), Cycle::Monthly, Some("2026-10-07")),
                    "t",
                )
                .unwrap(),
                build(
                    sub("mid", Some(1.0), Cycle::Monthly, Some("2026-10-15")),
                    "t",
                )
                .unwrap(),
                build(sub("none", Some(1.0), Cycle::Monthly, None), "t").unwrap(),
            ],
        };
        let up = upcoming(&book, d("2026-10-05"), 14);
        let names: Vec<&str> = up.iter().map(|(e, _)| e.provider.as_str()).collect();
        assert_eq!(names, ["soon", "mid"]);
    }

    /// **구독과 API 를 한 숫자로 합치지 않는다.**
    #[test]
    fn totals_sum_subscriptions_only_and_never_mix_in_api() {
        let api = NewPlan {
            provider: "claude".into(),
            kind: Some(PlanKind::Api),
            amount: Some(500.0),
            currency: "USD".into(),
            monthly_budget: Some(50.0),
            ..Default::default()
        };
        let book = PlanBook {
            version: 1,
            entries: vec![
                build(sub("claude", Some(100.0), Cycle::Monthly, None), "t").unwrap(),
                build(sub("gpt", Some(240.0), Cycle::Yearly, None), "t").unwrap(),
                build(api, "t").unwrap(),
            ],
        };
        let t = totals(&book);
        assert_eq!(t.by_currency.len(), 1);
        assert!(
            (t.by_currency[0].monthly - 120.0).abs() < 1e-9,
            "100 + 240/12"
        );
        assert_eq!(t.by_currency[0].entries, 2);
        assert_eq!(t.api_accounts, 1, "API 는 세기만 하고 더하지 않는다");
        assert!(t.is_complete());
    }

    #[test]
    fn totals_keep_currencies_apart_and_flag_missing_amounts() {
        let mut krw = sub("local", Some(29_000.0), Cycle::Monthly, None);
        krw.currency = "KRW".into();
        let book = PlanBook {
            version: 1,
            entries: vec![
                build(sub("a", Some(20.0), Cycle::Monthly, None), "t").unwrap(),
                build(krw, "t").unwrap(),
                build(sub("unknown-price", None, Cycle::Monthly, None), "t").unwrap(),
            ],
        };
        let t = totals(&book);
        assert_eq!(t.by_currency.len(), 2, "환율을 지어내지 않는다");
        assert_eq!(t.subscriptions_without_amount, 1);
        assert!(
            !t.is_complete(),
            "금액 없는 항목이 있으면 합계는 부분값이다"
        );
    }

    #[test]
    fn validation_explains_what_to_do() {
        let no_kind = NewPlan {
            provider: "claude".into(),
            ..Default::default()
        };
        assert!(matches!(build(no_kind, "t"), Err(PlanError::Invalid(m)) if m.contains("--kind")));

        assert!(build(sub("", Some(1.0), Cycle::Monthly, None), "t").is_err());
        assert!(build(sub("a", Some(-5.0), Cycle::Monthly, None), "t").is_err());
        assert!(build(sub("a", Some(f64::NAN), Cycle::Monthly, None), "t").is_err());
        assert!(build(sub("a", Some(1.0), Cycle::Monthly, Some("2026-02-30")), "t").is_err());
        // 갱신일만 있고 주기가 없으면 다음 날짜를 계산할 수 없다.
        assert!(build(sub("a", Some(1.0), Cycle::None, Some("2026-11-01")), "t").is_err());

        let mut bad_cur = sub("a", Some(1.0), Cycle::Monthly, None);
        bad_cur.currency = "달러".into();
        assert!(build(bad_cur, "t").is_err());

        // 예산은 API 전용.
        let mut sub_budget = sub("a", Some(1.0), Cycle::Monthly, None);
        sub_budget.monthly_budget = Some(10.0);
        assert!(build(sub_budget, "t").is_err());
    }

    #[test]
    fn ids_are_derived_and_sanitised() {
        let e = build(sub("Claude AI!", Some(1.0), Cycle::Monthly, None), "t").unwrap();
        assert_eq!(e.id, "claude-ai-subscription");

        let mut custom = sub("x", Some(1.0), Cycle::Monthly, None);
        custom.id = Some("../My Plan".into());
        assert_eq!(build(custom, "t").unwrap().id, "my-plan");
    }

    /// 사용자가 메모에 키를 붙여넣어도 **파일에는 남지 않는다.**
    #[test]
    fn free_text_is_redacted_before_saving() {
        let mut p = sub("claude", Some(20.0), Cycle::Monthly, None);
        p.note = "키: sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into();
        p.purpose = "코딩".into();
        let path = tmp("redact");
        upsert_at(&path, p, "t").unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("sk-ant-api03-AAAA"), "키가 평문으로 저장됐다");
        assert!(raw.contains("코딩"));
    }

    #[test]
    fn upsert_replaces_same_id_and_remove_deletes() {
        let path = tmp("upsert");
        let (_, added) =
            upsert_at(&path, sub("claude", Some(20.0), Cycle::Monthly, None), "t1").unwrap();
        assert!(added);
        let (e, added) = upsert_at(
            &path,
            sub("claude", Some(100.0), Cycle::Monthly, None),
            "t2",
        )
        .unwrap();
        assert!(!added, "같은 id 는 교체다");
        assert_eq!(e.confirmed_at, "t2");

        let book = load_from(&path).unwrap();
        assert_eq!(book.entries.len(), 1);
        assert_eq!(book.entries[0].amount, Some(100.0));

        assert_eq!(
            remove_at(&path, "nope"),
            Err(PlanError::NotFound("nope".into()))
        );
        remove_at(&path, "claude-subscription").unwrap();
        assert!(load_from(&path).unwrap().entries.is_empty());
    }

    /// **손상된 파일을 빈 장부로 덮어쓰지 않는다.**
    #[test]
    fn corrupt_file_is_never_overwritten() {
        let path = tmp("corrupt");
        std::fs::write(&path, "{ 이건 JSON 이 아니다").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        let r = upsert_at(&path, sub("claude", Some(20.0), Cycle::Monthly, None), "t");
        assert!(matches!(r, Err(PlanError::Corrupt(_))));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "원본이 그대로 남아야 한다"
        );
    }

    #[test]
    fn confirm_updates_timestamp_and_optionally_the_date() {
        let path = tmp("confirm");
        upsert_at(
            &path,
            sub("claude", Some(20.0), Cycle::Monthly, Some("2026-07-10")),
            "t1",
        )
        .unwrap();
        let e = confirm_at(&path, "claude-subscription", Some("2026-11-10"), "t2").unwrap();
        assert_eq!(e.confirmed_at, "t2");
        assert_eq!(e.renews_on.as_deref(), Some("2026-11-10"));
        // 확인한 뒤에는 추정이 아니다.
        assert!(!next_renewal(&e, d("2026-10-05")).unwrap().derived);

        assert!(confirm_at(&path, "claude-subscription", Some("엉터리"), "t3").is_err());
        assert!(confirm_at(&path, "missing", None, "t3").is_err());
    }

    fn api_entry(budget: Option<f64>) -> PlanEntry {
        build(
            NewPlan {
                provider: "claude".into(),
                kind: Some(PlanKind::Api),
                currency: "USD".into(),
                monthly_budget: budget,
                ..Default::default()
            },
            "t",
        )
        .unwrap()
    }

    fn est(cost: f64, priced: u64, missing: u64) -> crate::pricing::CostEstimate {
        crate::pricing::CostEstimate {
            known_cost: cost,
            currency: "USD".into(),
            priced_calls: priced,
            calls_missing_price: missing,
            ..Default::default()
        }
    }

    #[test]
    fn budget_thresholds() {
        let e = api_entry(Some(50.0));
        assert!(matches!(
            budget_status(&e, &est(10.0, 5, 0)),
            BudgetStatus::Within { partial: false, .. }
        ));
        assert!(matches!(
            budget_status(&e, &est(40.0, 5, 0)),
            BudgetStatus::Near { .. }
        ));
        assert!(matches!(
            budget_status(&e, &est(50.0, 5, 0)),
            BudgetStatus::Over { .. }
        ));
        assert_eq!(
            budget_status(&api_entry(None), &est(10.0, 5, 0)),
            BudgetStatus::NoBudget
        );
    }

    /// **계산할 수 없으면 0% 가 아니라 Unknown 이다.**
    #[test]
    fn budget_is_unknown_rather_than_zero_when_it_cannot_be_computed() {
        let e = api_entry(Some(50.0));

        let unconfigured = crate::pricing::CostEstimate {
            pricing_unconfigured: true,
            ..Default::default()
        };
        assert!(matches!(
            budget_status(&e, &unconfigured),
            BudgetStatus::Unknown { .. }
        ));
        assert!(matches!(
            budget_status(&e, &est(0.0, 0, 3)),
            BudgetStatus::Unknown { .. }
        ));

        let mut other_currency = est(10.0, 5, 0);
        other_currency.currency = "KRW".into();
        assert!(matches!(
            budget_status(&e, &other_currency),
            BudgetStatus::Unknown { .. }
        ));

        // 단가 없는 호출이 섞였으면 실제 사용액이 더 클 수 있다고 표시한다.
        assert!(matches!(
            budget_status(&e, &est(10.0, 5, 2)),
            BudgetStatus::Within { partial: true, .. }
        ));
    }
}
