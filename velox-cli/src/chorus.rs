use velox_core::ai::{
    MODELS_FILE, PROVIDERS_FILE, ProviderConfig, env_var_for, has_key, load_providers, model_name,
    save_providers,
};
use velox_core::policy::{AgentPurpose, AgentRequest, PolicyError, execute_agent};
use velox_core::privacy::ContextScope;

fn get_system_context() -> String {
    velox_core::privacy::AiContext::from_snapshot(
        &velox_core::snapshot::Snapshot::collect(),
        velox_core::privacy::ContextScope::Minimal,
    )
    .to_prompt_json()
}

/// 한 번 묻고 답을 출력한다. `conversation` 을 주면 그 대화를 이어간다.
///
/// `conversation` 이 있으면: 그 대화의 최근 맥락을 프롬프트 앞에 붙이고,
/// 질문과 답을 대화에 기록한다. **다른 모델을 지정해도 같은 대화를 이어갈 수 있다** —
/// 마스터 플랜의 "AI → APEX 공통 맥락 → 다음 AI" 흐름이다.
pub async fn ask_in(prompt: &str, model: &str, no_context: bool, conversation: Option<&str>) {
    use velox_core::conversation as conv;

    // 이어갈 대화를 먼저 읽는다 — 없는 id 면 AI 를 호출하기 전에 멈춘다.
    let existing = match conversation {
        Some(id) => match conv::load(id) {
            Ok(c) => Some(c),
            Err(e) => {
                println!("✗ {e}");
                return;
            }
        },
        None => None,
    };

    // 보낼 맥락: 시스템 정보(Minimal) + 이전 대화 발췌. 전체 대화를 그대로 보내지 않는다.
    let history = existing
        .as_ref()
        .map(|c| conv::context_text(c, 12, 6_000))
        .unwrap_or_default();
    let mut full_prompt = String::new();
    if !no_context {
        full_prompt.push_str(&get_system_context());
        full_prompt.push('\n');
    }
    if !history.is_empty() {
        full_prompt.push_str(&history);
        full_prompt.push('\n');
    }
    full_prompt.push_str("User question: ");
    full_prompt.push_str(prompt);

    if let Some(c) = &existing {
        println!(
            "대화 이어가기: {} ({}개 메시지){}",
            c.meta.title,
            c.meta.message_count,
            if c.meta.model != model && !c.meta.model.is_empty() {
                format!(" · 모델 변경 {} → {model}", c.meta.model)
            } else {
                String::new()
            }
        );
    }
    let label = if load_providers().iter().any(|p| p.name == model) {
        format!("{model} (custom)")
    } else {
        model.to_string()
    };
    println!("Asking {label}...\n");
    // 정책 게이트를 거친다 — 미승인 provider로 자동 대체(fallback)하지 않는다.
    let Some(text) = gated_text(
        model,
        AgentPurpose::Other,
        ContextScope::Minimal,
        full_prompt,
    )
    .await
    else {
        // 거부·실패한 호출은 대화에 기록하지 않는다(답이 없으므로).
        return;
    };
    println!("{text}");

    // 답을 받은 뒤에만 기록한다. 기록 실패가 답을 가리지 않게 경고만 띄운다.
    let id = match existing {
        Some(c) => Some(c.meta.id),
        None => None,
    };
    if let Some(id) = id {
        if let Err(e) = conv::append(&id, conv::Role::User, prompt, None, None) {
            println!("\n! 질문을 대화에 기록하지 못했습니다: {e}");
        }
        match conv::append(
            &id,
            conv::Role::Assistant,
            &text,
            Some(model),
            Some(&model_name(model)),
        ) {
            Ok(meta) => println!("\n(대화 {} · 메시지 {}개)", meta.id, meta.message_count),
            Err(e) => println!("\n! 답을 대화에 기록하지 못했습니다: {e}"),
        }
    }
}

/// 새 대화를 만든다. 이후 `chorus ask --conversation <id>` 로 이어간다.
pub fn chat_new(title: &str, model: &str) {
    match velox_core::conversation::create(title, model, &model_name(model)) {
        Ok(c) => {
            println!("✓ 새 대화: {}", c.meta.title);
            println!("  id: {}", c.meta.id);
            println!(
                "  이어가기: velox chorus ask \"질문\" --conversation {}",
                c.meta.id
            );
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// 대화 목록 — 최근에 쓴 것부터.
pub fn chat_list(json: bool) {
    let items = velox_core::conversation::list();
    if json {
        // 메뉴·화면이 사람용 출력을 파싱하지 않게 구조화된 목록을 준다.
        match serde_json::to_string_pretty(&items) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        return;
    }
    if items.is_empty() {
        println!("저장된 대화가 없습니다.");
        println!("  다음 행동: velox chorus chat new \"제목\" 으로 시작하세요.");
        return;
    }
    println!("=== 대화 {}개 ===\n", items.len());
    for m in &items {
        println!("{}  {}", m.updated_at, m.title);
        println!(
            "    id {} · 메시지 {} · 마지막 모델 {}",
            m.id,
            m.message_count,
            if m.model.is_empty() { "-" } else { &m.model }
        );
    }
}

/// 대화 한 건의 전체 내용.
pub fn chat_show(id: &str) {
    match velox_core::conversation::load(id) {
        Ok(c) => {
            println!("=== {} ===", c.meta.title);
            println!(
                "{} ~ {} · 메시지 {}개\n",
                c.meta.created_at, c.meta.updated_at, c.meta.message_count
            );
            for m in &c.messages {
                let who = match (&m.provider, &m.model) {
                    (Some(p), Some(mo)) => format!("{} [{p}/{mo}]", m.role.label()),
                    _ => m.role.label().to_string(),
                };
                println!("[{}] {who}", m.at);
                println!("{}\n", m.text);
            }
            if c.messages.is_empty() {
                println!("(아직 메시지가 없습니다)");
            }
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// 대화 삭제 — 되돌릴 수 없으므로 사용자가 id 를 정확히 줘야 한다.
pub fn chat_delete(id: &str) {
    match velox_core::conversation::delete(id) {
        Ok(()) => println!("✓ 대화를 삭제했습니다: {id}"),
        Err(e) => println!("✗ {e}"),
    }
}

/// 대화를 Markdown 파일로 내보낸다. `out` 이 없으면 APEX 리포트 폴더에 저장한다.
///
/// 이미 있는 파일은 덮어쓰지 않는다 — 노트 폴더(Obsidian Vault 등)를 지정했을 때
/// 사용자가 고친 노트를 날리지 않기 위해서다. 덮어쓰려면 `--force`.
pub fn chat_export(id: &str, out: Option<&str>, force: bool) {
    let c = match velox_core::conversation::load(id) {
        Ok(c) => c,
        Err(e) => {
            println!("✗ {e}");
            return;
        }
    };
    let path = match out {
        Some(p) => std::path::PathBuf::from(p),
        None => velox_core::paths::report_file(&format!("chat-{}.md", c.meta.id)),
    };
    if path.exists() && !force {
        println!("✗ 이미 파일이 있습니다: {}", path.display());
        println!("  다음 행동: 다른 경로를 --out 으로 주거나, 덮어쓰려면 --force 를 붙이세요.");
        return;
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        println!("✗ 폴더를 만들지 못했습니다 ({}): {e}", dir.display());
        return;
    }
    match std::fs::write(&path, velox_core::conversation::to_markdown(&c)) {
        Ok(()) => {
            println!("✓ 내보냈습니다: {}", path.display());
            println!(
                "  메시지 {}개 · 제목 {}",
                c.meta.message_count, c.meta.title
            );
        }
        Err(e) => {
            println!("✗ 저장하지 못했습니다 ({}): {e}", path.display());
            println!("  다음 행동: 경로와 쓰기 권한을 확인하세요.");
        }
    }
}

/// 정책 게이트([`execute_agent`])를 통과해 AI를 호출한다. 거부되면 이유(+동의 방법)를
/// 출력하고 `None`. 대표 제품 경로(ask/diagnose/doctor)가 공유하는 진입점.
pub async fn gated_text(
    provider: &str,
    purpose: AgentPurpose,
    scope: ContextScope,
    prompt: String,
) -> Option<String> {
    let req = AgentRequest {
        provider: provider.to_string(),
        purpose,
        prompt,
        scope,
        requested_tools: std::collections::BTreeSet::new(),
    };
    match execute_agent(req).await {
        Ok(r) => Some(r.text),
        // 정책 실패는 전부 guidance 를 거쳐 "다음 행동"까지 함께 보여준다.
        // 동시에 거부 사유를 알파 지표로 센다(고정 라벨만 — provider 이름은 넘기지 않는다).
        Err(PolicyError::CloudNotAllowed(p)) => {
            velox_core::metrics::record_policy_denial("consent_missing");
            println!(
                "{}",
                velox_core::guidance::Problem::ConsentMissing {
                    provider: p.clone(),
                    needed_scope: scope,
                }
                .guidance()
                .render_plain()
            );
            None
        }
        Err(PolicyError::ScopeExceeded { requested, max }) => {
            velox_core::metrics::record_policy_denial("scope_exceeded");
            println!(
                "{}",
                velox_core::guidance::Problem::ScopeTooLow {
                    provider: provider.to_string(),
                    requested,
                    max,
                }
                .guidance()
                .render_plain()
            );
            None
        }
        Err(e) => {
            velox_core::metrics::record_policy_denial(match &e {
                PolicyError::UnknownProvider(_) => "unknown_provider",
                PolicyError::ToolNotAllowed(_) => "tool_not_allowed",
                PolicyError::ProviderCallFailed(_) => "provider_call_failed",
                _ => "other",
            });
            let problem: velox_core::guidance::Problem = (&e).into();
            println!("{}", problem.guidance().render_plain());
            None
        }
    }
}

/// `gated_text`의 조용한 버전 — 거부/실패 모두 `None` (루프에서 미승인 provider 자동 skip용).
/// 화면 출력 없이 정책 게이트만 통과시킨다.
pub async fn gated_text_quiet(
    provider: &str,
    purpose: AgentPurpose,
    scope: ContextScope,
    prompt: String,
) -> Option<String> {
    let req = AgentRequest {
        provider: provider.to_string(),
        purpose,
        prompt,
        scope,
        requested_tools: std::collections::BTreeSet::new(),
    };
    execute_agent(req).await.ok().map(|r| r.text)
}

fn scope_label(scope: ContextScope) -> &'static str {
    match scope {
        ContextScope::Minimal => "minimal",
        ContextScope::System => "system",
        ContextScope::Drivers => "drivers",
    }
}

fn parse_scope(s: &str) -> Option<ContextScope> {
    match s.trim().to_lowercase().as_str() {
        "minimal" => Some(ContextScope::Minimal),
        "system" => Some(ContextScope::System),
        "drivers" => Some(ContextScope::Drivers),
        _ => None,
    }
}

/// provider별 클라우드 호출 동의. `velox chorus consent <provider> [--scope ...]`
pub fn consent(provider: &str, scope: &str) {
    if !velox_core::policy::provider_exists(provider) {
        println!("✗ 알 수 없는 provider: {provider}");
        return;
    }
    let scope = match parse_scope(scope) {
        Some(s) => s,
        None => {
            println!("✗ 알 수 없는 scope: {scope} (minimal / system / drivers)");
            return;
        }
    };
    if velox_core::policy::grant_consent(provider, scope) {
        println!(
            "✓ {provider} 클라우드 호출 동의됨 (scope={}, 부수효과는 여전히 사람 승인 필요)",
            scope_label(scope)
        );
        println!("  철회: velox chorus revoke {provider}");
    } else {
        println!("✗ 동의 저장 실패");
    }
}

/// provider 동의 철회. `velox chorus revoke <provider>`
pub fn revoke(provider: &str) {
    if velox_core::policy::revoke_consent(provider) {
        println!("✓ {provider} 동의 철회됨 → deny-by-default");
    } else {
        println!("✗ 철회 저장 실패");
    }
}

/// 커스텀 provider 추가. `velox chorus add <name> <base_url> <model> [key]`
pub fn add_provider(name: &str, base_url: &str, model: &str, key: &str) {
    let builtin = [
        "claude",
        "gpt",
        "gemini",
        "grok",
        "anthropic",
        "openai",
        "google",
        "xai",
    ];
    if builtin.contains(&name.to_lowercase().as_str()) {
        println!(
            "✗ '{}'는 내장 provider 이름이라 못 써요. 다른 이름으로.",
            name
        );
        return;
    }
    let mut ps = load_providers();
    ps.retain(|x| x.name != name);
    ps.push(ProviderConfig {
        name: name.to_string(),
        base_url: base_url.to_string(),
        model: model.to_string(),
        api_key: key.to_string(),
        local: false,
    });
    if save_providers(&ps) {
        println!(
            "✓ 커스텀 provider 추가됨: {} ({} / {})",
            name, base_url, model
        );
        println!("  사용: velox chorus ask \"...\" --use {}", name);
        println!("  (저장: {})", PROVIDERS_FILE);
    } else {
        println!("✗ 저장 실패");
    }
}

/// 내장 provider의 모델 ID 설정. `velox chorus model set <provider> <model-id>`
pub fn set_model(provider: &str, model_id: &str) {
    match velox_core::ai::set_model(provider, model_id) {
        Ok(id) => {
            println!("✓ {} 모델 설정됨 → {}", provider, id);
            println!("  (저장: {})", MODELS_FILE);
        }
        Err(e) => println!("✗ {}", e),
    }
}

/// 내장 provider의 모델을 기본값으로 초기화. `velox chorus model reset <provider>`
pub fn reset_model(provider: &str) {
    match velox_core::ai::reset_model(provider) {
        Ok(id) => println!("✓ {} 모델 기본값 복원 → {}", provider, id),
        Err(e) => println!("✗ {}", e),
    }
}

/// 사용자가 직접 API 키를 OS 보안 저장소에 저장.
pub fn set_key(provider: &str, key: &str) {
    let var = match env_var_for(provider) {
        Some(v) => v,
        None => {
            println!(
                "✗ 알 수 없는 provider: {} (claude / gpt / gemini / grok)",
                provider
            );
            return;
        }
    };
    match velox_core::credentials::set(provider, key) {
        Ok(()) => println!("✓ {} ({}) 키 저장됨 → OS 보안 저장소", provider, var),
        Err(e) => println!("✗ 키 저장 실패: {}", e),
    }
}

/// 연결된 모든 AI에 실제로 핑을 보내 응답 여부를 검증. `velox chorus test`
pub async fn test_all() {
    println!("=== APEX Chorus — 연결 테스트 (정책 게이트 경유) ===\n");
    for p in ["claude", "gpt", "gemini", "grok"] {
        report_ping(p, "").await;
    }
    for p in load_providers() {
        report_ping(&p.name, " (커스텀)").await;
    }
}

/// 한 provider에 정책 게이트를 통해 핑을 보내고 결과를 사람용으로 출력.
async fn report_ping(provider: &str, tag: &str) {
    let req = AgentRequest {
        provider: provider.to_string(),
        purpose: AgentPurpose::Other,
        prompt: "Reply with exactly: OK".to_string(),
        scope: ContextScope::Minimal,
        requested_tools: std::collections::BTreeSet::new(),
    };
    let t = std::time::Instant::now();
    match execute_agent(req).await {
        Ok(_) => println!(
            "✓ {provider:8}{tag} 응답 정상 ({}ms)",
            t.elapsed().as_millis()
        ),
        Err(PolicyError::CloudNotAllowed(_)) => {
            println!("• {provider:8}{tag} 미동의 — `velox chorus consent {provider}`")
        }
        Err(PolicyError::ProviderCallFailed(_)) => {
            println!("✗ {provider:8}{tag} 응답 실패 — 키/네트워크/모델 확인")
        }
        Err(e) => println!("✗ {provider:8}{tag} {e}"),
    }
}

/// 심판 응답에서 0~10 점수 추출.
fn parse_score(s: &str) -> Option<f64> {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_digit() || c == '.' {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned
        .split_whitespace()
        .next()?
        .parse::<f64>()
        .ok()
        .map(|v| v.clamp(0.0, 1000.0))
}

/// 벤치 문제 세트. hard=true면 변별력 있는 어려운(오염 적은 새) 문제.
fn prompt_set(hard: bool) -> Vec<(&'static str, &'static str)> {
    if hard {
        vec![
            (
                "코딩",
                "Write a Rust function `longest_palindrome(s: &str) -> String` returning the longest palindromic substring. Code only, correct and efficient.",
            ),
            (
                "추론",
                "A bat and a ball cost $1.10 total. The bat costs $1.00 more than the ball. How much is the ball? Show your reasoning.",
            ),
            (
                "수학",
                "What is the remainder when 7^100 is divided by 13? Show the key step.",
            ),
            (
                "지식",
                "In exactly 2 sentences, explain why TCP uses a three-way handshake instead of a two-way one.",
            ),
            (
                "글쓰기",
                "Write one haiku (5-7-5 syllables) about a compiler error. Output only the haiku.",
            ),
        ]
    } else {
        vec![
            (
                "코딩",
                "Write a Rust function `fib(n: u64) -> u64` returning the nth Fibonacci number. Code only.",
            ),
            (
                "추론",
                "A train travels 60 km in 45 minutes. What is its speed in km/h?",
            ),
            (
                "지식",
                "In one sentence, what is the key difference between TCP and UDP?",
            ),
        ]
    }
}

/// AI 모델 벤치 (LLM-as-judge). 연결된 모든 모델에 같은 질문 → judge가 채점 → 리더보드.
/// 측정: 점수(카테고리별), 속도(지연), 처리량(chars/s), 크기(응답 길이).
/// `velox chorus bench [--judge <model>] [--hard]`
pub async fn bench(hard: bool) {
    println!(
        "=== APEX Chorus — AI 모델 벤치 (다중 심판, 0~1000{}) ===",
        if hard { ", HARD" } else { "" }
    );

    let prompts = prompt_set(hard);

    // 심판 패널 = 키 있는 내장 모델. 자기 답은 자기가 채점 안 함 → self-bias 제거.
    let panel: Vec<String> = ["claude", "gpt", "gemini", "grok"]
        .iter()
        .filter(|m| has_key(m))
        .map(|s| s.to_string())
        .collect();
    if panel.is_empty() {
        println!("심판으로 쓸 모델 없음 (키 필요).");
        return;
    }
    println!(
        "심판 패널: {} · 각 답을 자기 외 심판들이 0~1000 채점 → 평균\n",
        panel.join(", ")
    );

    // 대상 = 키 있는 내장 + 커스텀
    let mut models: Vec<String> = Vec::new();
    for m in ["claude", "gpt", "gemini", "grok"] {
        if has_key(m) {
            models.push(m.to_string());
        }
    }
    for p in load_providers() {
        models.push(p.name);
    }
    if models.is_empty() {
        println!("테스트할 모델 없음 (키 설정 필요).");
        return;
    }

    struct Row {
        model: String,
        cat_scores: Vec<f64>,
        lat_ms: u128,
        len: usize,
    }
    let mut rows: Vec<Row> = Vec::new();

    for model in &models {
        println!("[{}] 측정 중...", model);
        let mut cat_scores = Vec::new();
        let (mut lat, mut len) = (0u128, 0usize);
        for (cat, q) in &prompts {
            let t = std::time::Instant::now();
            let answer = gated_text_quiet(
                model,
                AgentPurpose::Other,
                ContextScope::Minimal,
                q.to_string(),
            )
            .await;
            lat += t.elapsed().as_millis();
            let answer = match answer {
                Some(a) => a,
                None => {
                    println!("  {} ✗ 응답 실패", cat);
                    cat_scores.push(0.0);
                    continue;
                }
            };
            len += answer.chars().count();
            let jp = format!(
                "You are an impartial expert evaluator. Rate the answer's correctness and quality on a \
                 precise 0 to 1000 scale (use the full range for fine discrimination). Reply with ONLY the integer.\n\n\
                 Question: {}\nAnswer: {}",
                q, answer
            );
            let mut sum = 0.0;
            let mut cnt = 0u32;
            for j in panel.iter().filter(|j| j.as_str() != model.as_str()) {
                if let Some(sc) =
                    gated_text_quiet(j, AgentPurpose::Review, ContextScope::Minimal, jp.clone())
                        .await
                        .and_then(|s| parse_score(&s))
                {
                    sum += sc;
                    cnt += 1;
                }
            }
            let score = if cnt > 0 { sum / cnt as f64 } else { 0.0 };
            println!("  {} → {:.0}/1000 (심판 {}명)", cat, score, cnt);
            cat_scores.push(score);
        }
        rows.push(Row {
            model: model.clone(),
            cat_scores,
            lat_ms: lat,
            len,
        });
    }

    let np = prompts.len().max(1);
    let avg_score = |r: &Row| r.cat_scores.iter().sum::<f64>() / np as f64;
    rows.sort_by(|a, b| {
        avg_score(b)
            .partial_cmp(&avg_score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("\n=== 리더보드 (다중 심판 평균) ===");
    println!(
        "{:<12}{:>10}{:>9}{:>11}{:>9}",
        "모델", "점수/1000", "속도ms", "처리량c/s", "크기"
    );
    println!("{}", "-".repeat(53));
    for r in &rows {
        let sc = avg_score(r);
        let lat = r.lat_ms / np as u128;
        let size = r.len / np;
        let thru = if r.lat_ms > 0 {
            r.len as f64 / (r.lat_ms as f64 / 1000.0)
        } else {
            0.0
        };
        println!(
            "{:<12}{:>10.0}{:>7}ms{:>11.0}{:>9}",
            r.model, sc, lat, thru, size
        );
    }

    println!("\n=== 카테고리별 점수 (/1000) ===");
    print!("{:<12}", "모델");
    for (cat, _) in &prompts {
        print!("{:>8}", cat);
    }
    println!();
    for r in &rows {
        print!("{:<12}", r.model);
        for s in &r.cat_scores {
            print!("{:>8.0}", s);
        }
        println!();
    }
    println!(
        "\n※ 자기 답은 자기가 채점 안 함 → self-bias 완화. 패널 공통 편향은 남음(상대 참고용)."
    );
}

/// AI 합의 — 같은 질문을 여러 모델에 → 공통점/차이를 종합. `velox chorus consensus "질문"`
pub async fn consensus(question: &str) {
    println!("=== APEX Chorus — AI 합의 (Consensus) ===\n");

    let mut models: Vec<String> = Vec::new();
    for m in ["claude", "gpt", "gemini", "grok"] {
        if has_key(m) {
            models.push(m.to_string());
        }
    }
    for p in load_providers() {
        models.push(p.name);
    }
    if models.len() < 2 {
        println!("합의하려면 응답 가능한 모델이 2개 이상 필요.");
        return;
    }

    println!("질문: {}\n", question);
    let mut answers: Vec<(String, String)> = Vec::new();
    for model in &models {
        print!("[{}] 응답 중...", model);
        std::io::Write::flush(&mut std::io::stdout()).ok();
        match gated_text_quiet(
            model,
            AgentPurpose::Consensus,
            ContextScope::Minimal,
            question.to_string(),
        )
        .await
        {
            Some(a) => {
                println!(" ✓");
                answers.push((model.clone(), a));
            }
            None => println!(" ✗"),
        }
    }
    if answers.len() < 2 {
        println!("\n응답이 2개 미만이라 합의 불가.");
        return;
    }

    println!("\n--- 각 모델 답변(앞부분) ---");
    for (m, a) in &answers {
        let snip: String = a.chars().take(110).collect::<String>().replace('\n', " ");
        let more = if a.chars().count() > 110 { "…" } else { "" };
        println!("[{}] {}{}", m, snip, more);
    }

    let mut combined = String::new();
    for (m, a) in &answers {
        combined.push_str(&format!("[{}]\n{}\n\n", m, a));
    }
    let synth_prompt = format!(
        "아래는 여러 AI가 같은 질문에 답한 것이다. 한국어로 간결하게:\n\
         1) 공통된 핵심 (모두 동의하는 내용)\n\
         2) 의견이 갈리는 지점 (차이/불일치)\n\
         답변을 그대로 반복하지 말고 합의/차이만 정리하라.\n\n\
         질문: {}\n\n{}",
        question, combined
    );
    let synthesizer = if has_key("claude") {
        "claude".to_string()
    } else {
        answers[0].0.clone()
    };
    println!("\n--- 종합 (by {}) ---", synthesizer);
    match gated_text_quiet(
        &synthesizer,
        AgentPurpose::Consensus,
        ContextScope::Minimal,
        synth_prompt,
    )
    .await
    {
        Some(s) => println!("{}", s.trim()),
        None => println!("(종합 실패)"),
    }
}

pub fn show_models() {
    let roles = [
        ("claude", "Code / Architecture"),
        ("gpt", "Strategy / Business"),
        ("gemini", "Docs / Analysis"),
        ("grok", "Search / Latest info"),
    ];

    println!("=== APEX Chorus — Connected Models ===\n");
    println!("[내장]  (모델 변경: chorus model set <provider> <id>)");
    for (name, role) in &roles {
        let key = if has_key(name) {
            "✓ key"
        } else {
            "✗ no-key"
        };
        println!(
            "{:8} model={:<24} [{}]  {}",
            name,
            model_name(name),
            key,
            role
        );
        println!("         policy: {}", fmt_policy(name));
    }
    let custom = load_providers();
    if !custom.is_empty() {
        println!("\n[커스텀 — chorus add 로 추가됨]");
        for p in &custom {
            let key = if has_key(&p.name) {
                "✓ key"
            } else {
                "· inline/none"
            };
            println!(
                "{:8} model={:<24} [{}]  {}",
                p.name, p.model, key, p.base_url
            );
            println!("         policy: {}", fmt_policy(&p.name));
        }
    }
    println!(
        "\n※ Agent Policy: deny-by-default (강제 게이트 = execute_agent). 설정 파일 {}.",
        velox_core::policy::POLICIES_FILE
    );
    println!();
}

/// provider의 유효 정책을 한 줄 요약. (velox_policies.json 없으면 안전 기본값 = deny)
fn fmt_policy(provider: &str) -> String {
    let p = velox_core::policy::policy_for(provider);
    let loc = velox_core::policy::provider_location(provider);
    let tools = if p.allowed_tools.is_empty() {
        "none".to_string()
    } else {
        p.allowed_tools
            .iter()
            .map(|t| format!("{t:?}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "loc={:?} cloud={} scope={:?} tools={} confirm={}",
        loc,
        if p.allow_cloud { "on" } else { "off" },
        p.max_context_scope,
        tools,
        if p.require_confirmation {
            "required"
        } else {
            "off"
        }
    )
}
