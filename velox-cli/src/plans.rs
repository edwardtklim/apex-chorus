//! `velox plans` — AI 구독·API 계정 기록 (마스터 플랜 4장).
//!
//! 판단·검증·계산은 전부 [`velox_core::plans`] 가 한다. 여기서는 표시만 한다.
//! 이 명령은 **기록만 한다** — 결제를 실행하지 않고, 값을 자동으로 조회하지 않는다.

use velox_core::plans::{
    self, BudgetStatus, Cycle, Date, NewPlan, NextRenewal, PlanBook, PlanEntry, PlanKind,
};

/// `velox plans add` 의 입력.
pub struct AddArgs {
    pub provider: String,
    pub kind: String,
    pub plan: String,
    pub amount: Option<f64>,
    pub currency: String,
    pub cycle: String,
    pub renews: Option<String>,
    pub purpose: String,
    pub budget: Option<f64>,
    pub note: String,
    pub id: Option<String>,
}

fn load_or_explain() -> Option<PlanBook> {
    match plans::load() {
        Ok(b) => Some(b),
        Err(e) => {
            println!("✗ {e}");
            None
        }
    }
}

fn money(amount: f64, currency: &str) -> String {
    // 원화처럼 소수점이 의미 없는 금액은 정수로, 그 외는 소수 둘째 자리까지.
    if amount.fract() == 0.0 && amount >= 1000.0 {
        format!("{currency} {amount:.0}")
    } else {
        format!("{currency} {amount:.2}")
    }
}

fn renewal_text(n: Option<NextRenewal>) -> String {
    match n {
        None => "미입력".into(),
        Some(n) => {
            let when = match n.days_left {
                0 => "오늘".to_string(),
                d => format!("D-{d}"),
            };
            if n.derived {
                // 사용자가 넣은 날짜가 아니라 주기로 계산한 값임을 숨기지 않는다.
                format!("{} ({when}, 추정 — 입력한 날짜가 지남)", n.date)
            } else {
                format!("{} ({when})", n.date)
            }
        }
    }
}

fn budget_text(e: &PlanEntry) -> String {
    let est = plans::month_estimate_for(&e.provider);
    let more = |partial: bool| {
        if partial {
            " 이상(부분 집계)"
        } else {
            ""
        }
    };
    match plans::budget_status(e, &est) {
        BudgetStatus::NoBudget => "예산 미설정".into(),
        BudgetStatus::Unknown { reason } => {
            format!(
                "예산 {}/월 · 사용률 알 수 없음 — {reason}",
                money(e.monthly_budget.unwrap_or(0.0), &e.currency)
            )
        }
        BudgetStatus::Within {
            spent,
            budget,
            pct,
            partial,
        } => format!(
            "예산 {}/월 · 이번 달 추정 {}{} ({pct:.0}%)",
            money(budget, &e.currency),
            money(spent, &e.currency),
            more(partial)
        ),
        BudgetStatus::Near {
            spent,
            budget,
            pct,
            partial,
        } => format!(
            "⚠ 예산 임박 — {}/월 중 추정 {}{} ({pct:.0}%)",
            money(budget, &e.currency),
            money(spent, &e.currency),
            more(partial)
        ),
        BudgetStatus::Over {
            spent,
            budget,
            pct,
            partial,
        } => format!(
            "⚠ 예산 초과 — {}/월 중 추정 {}{} ({pct:.0}%)",
            money(budget, &e.currency),
            money(spent, &e.currency),
            more(partial)
        ),
    }
}

fn print_entry(e: &PlanEntry, today: Date) {
    let plan = if e.plan.is_empty() { "-" } else { &e.plan };
    println!("  {}  [{}]", e.provider, e.id);
    match e.kind {
        PlanKind::Subscription => {
            let price = match (e.amount, e.cycle) {
                (Some(a), Cycle::None) => money(a, &e.currency),
                (Some(a), c) => format!("{}/{}", money(a, &e.currency), c.label()),
                (None, _) => "금액 미입력".into(),
            };
            println!("      플랜 {plan} · {price}");
            println!("      갱신 {}", renewal_text(plans::next_renewal(e, today)));
        }
        PlanKind::Api => {
            println!("      플랜 {plan}");
            println!("      {}", budget_text(e));
        }
    }
    if !e.purpose.is_empty() {
        println!("      주 용도: {}", e.purpose);
    }
    if !e.note.is_empty() {
        println!("      메모: {}", e.note);
    }
    println!(
        "      확인 {} (직접 입력)",
        e.confirmed_at.get(..10).unwrap_or(&e.confirmed_at)
    );
}

fn print_totals(book: &PlanBook) {
    let t = plans::totals(book);
    println!("--- 월 고정 지출 (구독만) ---");
    if t.by_currency.is_empty() {
        println!("  합산할 구독이 없습니다");
    }
    for c in &t.by_currency {
        println!(
            "  {}  ({}건, 연 구독은 12로 나눔)",
            money(c.monthly, &c.currency),
            c.entries
        );
    }
    if !t.is_complete() {
        println!(
            "  ! 금액 또는 주기가 없는 구독 {}건은 빠져 있습니다 — 합계는 부분값입니다",
            t.subscriptions_without_amount
        );
    }
    if t.api_accounts > 0 {
        println!(
            "  API 계정 {}개는 합계에 넣지 않았습니다. 사용량에 따른 추정 비용이라 성격이 다릅니다 → velox usage summary",
            t.api_accounts
        );
    }
}

/// `velox plans list`
pub fn list(json: bool) {
    let Some(book) = load_or_explain() else {
        return;
    };
    if json {
        match serde_json::to_string_pretty(&book) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        return;
    }

    let today = plans::today_utc();
    println!("=== AI 구독·API 기록 ===");
    println!("오늘 {today} (UTC 기준 — 현지 날짜와 하루 차이 날 수 있음)\n");

    if book.entries.is_empty() {
        println!("기록이 없습니다.");
        println!(
            "  다음 행동: 예) velox plans add --provider claude --kind subscription --plan Max \\"
        );
        println!(
            "               --amount 100 --currency USD --cycle monthly --renews 2026-11-03 --purpose \"코딩\""
        );
        return;
    }

    for (title, kind) in [
        ("구독", PlanKind::Subscription),
        ("API 계정", PlanKind::Api),
    ] {
        let group: Vec<&PlanEntry> = book.entries.iter().filter(|e| e.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        println!("--- {title} {}건 ---", group.len());
        for e in group {
            print_entry(e, today);
        }
        println!();
    }

    print_totals(&book);
    println!(
        "\n모든 값은 직접 입력한 것입니다(자동 조회 없음). 이 명령은 결제를 실행하지 않습니다."
    );
}

/// `velox plans add ...` — 같은 id 가 있으면 교체한다.
pub fn add(a: AddArgs) {
    let Some(cycle) = Cycle::parse(&a.cycle) else {
        println!("✗ 주기를 읽을 수 없습니다: {}", a.cycle);
        println!("  다음 행동: --cycle monthly / yearly / none 중 하나를 쓰세요.");
        return;
    };
    let input = NewPlan {
        id: a.id,
        provider: a.provider,
        kind: PlanKind::parse(&a.kind),
        plan: a.plan,
        amount: a.amount,
        currency: a.currency,
        cycle,
        renews_on: a.renews,
        purpose: a.purpose,
        monthly_budget: a.budget,
        note: a.note,
    };
    match plans::upsert(input) {
        Ok((e, added)) => {
            println!(
                "✓ {}: {} [{}]",
                if added { "추가" } else { "교체" },
                e.provider,
                e.id
            );
            print_entry(&e, plans::today_utc());
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox plans remove <id>`
pub fn remove(id: &str) {
    match plans::remove(id) {
        Ok(e) => println!("✓ 기록을 삭제했습니다: {} [{}]", e.provider, e.id),
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox plans confirm <id> [--renews DATE]` — 값이 아직 맞다고 확인한다.
pub fn confirm(id: &str, renews: Option<&str>) {
    match plans::confirm(id, renews) {
        Ok(e) => {
            println!("✓ 확인했습니다: {} [{}]", e.provider, e.id);
            print_entry(&e, plans::today_utc());
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox plans upcoming [--days N]`
pub fn upcoming(days: i64) {
    let Some(book) = load_or_explain() else {
        return;
    };
    let today = plans::today_utc();
    let items = plans::upcoming(&book, today, days);
    println!("=== {days}일 안에 갱신 ===");
    println!("오늘 {today} (UTC 기준)\n");
    if items.is_empty() {
        println!("  없음");
    }
    for (e, n) in &items {
        let price = match e.amount {
            Some(a) => money(a, &e.currency),
            None => "금액 미입력".into(),
        };
        println!(
            "  {}  {} {} · {}",
            renewal_text(Some(*n)),
            e.provider,
            if e.plan.is_empty() { "" } else { &e.plan },
            price
        );
    }
    let no_date = book
        .entries
        .iter()
        .filter(|e| e.kind == PlanKind::Subscription && e.renews_on.is_none())
        .count();
    if no_date > 0 {
        println!(
            "\n  ! 갱신일이 없는 구독 {no_date}건은 여기에 나오지 않습니다 → velox plans confirm <id> --renews YYYY-MM-DD"
        );
    }
}
