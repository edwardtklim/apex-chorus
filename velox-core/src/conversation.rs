//! velox-core::conversation — 로컬 대화 저장소 (프로그램 1 AI Chat).
//!
//! 마스터 플랜 프로그램 1의 빠진 조각이다. 지금까지 `chorus ask` 는 한 번 묻고
//! 답을 화면에 뿌리면 끝이었다. 대화가 남지 않으면 다음 질문에서 목적을 다시
//! 설명해야 하고, 다른 모델에게 "이어서 해줘"라고 할 수도 없다.
//!
//! 설계에서 양보하지 않는 것:
//!
//! 1. **키는 저장하지 않는다.** 모든 본문은 [`crate::project::redact_secrets`] 를
//!    통과한 뒤에만 디스크에 닿는다. 사용자가 실수로 키를 붙여넣어도 파일에는 남지 않는다.
//! 2. **저장 위치는 [`crate::paths`] 가 정한다.** 실행 위치(CWD)에 파일을 만들지 않는다.
//! 3. **id 는 경로가 아니다.** 외부(CLI·HTTP)에서 들어온 id 는 문자 집합을 검사해
//!    디렉터리 탈출을 막는다. `../` 같은 값은 파일 접근 전에 거부한다.
//! 4. **원자적 저장.** 임시 파일에 쓰고 rename 한다. 중간에 죽어도 반쯤 쓰인
//!    JSON 이 남지 않는다.
//! 5. **무한히 쌓지 않는다.** 메시지 수와 본문 길이에 상한이 있다.
//!
//! 이 모듈은 AI 를 호출하지 않는다. 호출은 [`crate::policy::execute_agent`] 가
//! 하고, 결과를 여기에 적는 것은 호출자의 몫이다.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 한 대화에 보관하는 최대 메시지 수. 넘으면 오래된 것부터 버린다.
pub const MAX_MESSAGES: usize = 400;
/// 메시지 하나의 최대 길이(문자). 넘으면 잘라내고 잘렸다고 표시한다.
pub const MAX_TEXT_CHARS: usize = 20_000;
/// 제목 최대 길이.
pub const MAX_TITLE_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    pub fn label(&self) -> &str {
        match self {
            Role::User => "나",
            Role::Assistant => "AI",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub at: String,
    /// 본문. 저장 전에 레닥션과 길이 제한을 거친다.
    pub text: String,
    /// 어떤 provider/model 이 만든 답인지. 사용자 메시지면 None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 본문이 길이 제한으로 잘렸는지. 잘린 것을 원문처럼 보여주면 안 된다.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// 목록 화면에 필요한 만큼만. 본문은 들어 있지 않다.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    /// 마지막으로 사용한 provider — 다른 모델로 이어가면 바뀐다.
    pub provider: String,
    pub model: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Conversation {
    #[serde(flatten)]
    pub meta: ConversationMeta,
    #[serde(default)]
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConvError {
    /// id 가 허용 문자 집합을 벗어났다. 경로 탈출 시도일 수 있다.
    InvalidId(String),
    NotFound(String),
    /// 읽었지만 JSON 이 깨졌다. 파일을 지우지 않고 그대로 둔다.
    Corrupt {
        id: String,
        reason: String,
    },
    Io(String),
    EmptyText,
}

impl std::fmt::Display for ConvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvError::InvalidId(id) => write!(
                f,
                "대화 id 가 올바르지 않습니다: {id}\n  다음 행동: 영문 소문자·숫자·하이픈만 쓸 수 있습니다. `velox chorus history` 로 목록을 확인하세요."
            ),
            ConvError::NotFound(id) => write!(
                f,
                "그런 대화가 없습니다: {id}\n  다음 행동: `velox chorus history` 로 목록을 확인하세요."
            ),
            ConvError::Corrupt { id, reason } => write!(
                f,
                "대화 파일이 손상됐습니다({id}): {reason}\n  다음 행동: 파일은 지우지 않았습니다. 새 대화로 계속하세요."
            ),
            ConvError::Io(e) => write!(
                f,
                "대화를 저장하지 못했습니다: {e}\n  다음 행동: 디스크 공간과 권한을 확인하세요."
            ),
            ConvError::EmptyText => write!(f, "빈 메시지는 저장하지 않습니다."),
        }
    }
}

fn dir() -> PathBuf {
    crate::paths::resolve("conversations")
}

/// 외부에서 들어온 id 검사 — 파일 시스템에 닿기 **전에** 막는다.
fn validate_id(id: &str) -> Result<(), ConvError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(ConvError::InvalidId(id.to_string()))
    }
}

fn path_of(id: &str) -> Result<PathBuf, ConvError> {
    validate_id(id)?;
    Ok(dir().join(format!("{id}.json")))
}

/// 저장 전 본문 정리 — 레닥션 후 길이 제한. (정리된 본문, 잘렸는지)
fn sanitize(text: &str) -> Result<(String, bool), ConvError> {
    let redacted = crate::project::redact_secrets(text.trim());
    if redacted.trim().is_empty() {
        return Err(ConvError::EmptyText);
    }
    let mut out: String = redacted.chars().take(MAX_TEXT_CHARS).collect();
    let truncated = out.chars().count() < redacted.chars().count();
    if truncated {
        out.push_str("\n…[길이 제한으로 잘림]");
    }
    Ok((out, truncated))
}

/// 같은 초에 여러 대화를 만들어도 id 가 겹치지 않게 하는 프로세스 내 카운터.
/// (초 단위 시각 + pid 만으로는 충돌한다 — 테스트가 실제로 서로의 파일을 덮어썼다.)
static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn new_id() -> String {
    let now = crate::util::now_rfc3339();
    let digits: String = now.chars().filter(|c| c.is_ascii_digit()).collect();
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{digits}-{:x}-{seq:x}", std::process::id())
}

fn save(c: &Conversation) -> Result<(), ConvError> {
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| ConvError::Io(e.to_string()))?;
    let json = serde_json::to_string_pretty(c).map_err(|e| ConvError::Io(e.to_string()))?;
    // 원자적 저장: tmp → rename. 중간에 죽어도 반쯤 쓰인 파일이 남지 않는다.
    let final_path = path_of(&c.meta.id)?;
    let tmp = d.join(format!("{}.json.tmp", c.meta.id));
    std::fs::write(&tmp, json).map_err(|e| ConvError::Io(e.to_string()))?;
    std::fs::rename(&tmp, &final_path).map_err(|e| ConvError::Io(e.to_string()))?;
    Ok(())
}

/// 새 대화를 만든다. 제목이 비면 시각으로 대신한다.
pub fn create(title: &str, provider: &str, model: &str) -> Result<Conversation, ConvError> {
    let now = crate::util::now_rfc3339();
    let title = crate::project::redact_secrets(title.trim());
    let title: String = if title.is_empty() {
        format!("대화 {now}")
    } else {
        title.chars().take(MAX_TITLE_CHARS).collect()
    };
    let c = Conversation {
        meta: ConversationMeta {
            id: new_id(),
            title,
            provider: provider.to_string(),
            model: model.to_string(),
            created_at: now.clone(),
            updated_at: now,
            message_count: 0,
        },
        messages: Vec::new(),
    };
    save(&c)?;
    Ok(c)
}

pub fn load(id: &str) -> Result<Conversation, ConvError> {
    let p = path_of(id)?;
    let raw = match std::fs::read_to_string(&p) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(ConvError::NotFound(id.to_string()));
        }
        Err(e) => return Err(ConvError::Io(e.to_string())),
    };
    serde_json::from_str(&raw).map_err(|e| ConvError::Corrupt {
        id: id.to_string(),
        reason: e.to_string(),
    })
}

/// 최근에 쓴 것부터. 손상된 파일은 목록에서 건너뛴다(지우지 않는다).
pub fn list() -> Vec<ConversationMeta> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    let mut out: Vec<ConversationMeta> = entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|raw| serde_json::from_str::<Conversation>(&raw).ok())
        .map(|c| c.meta)
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// 메시지 추가. provider/model 은 그 메시지를 만든 주체(사용자 메시지면 None).
pub fn append(
    id: &str,
    role: Role,
    text: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> Result<ConversationMeta, ConvError> {
    let (text, truncated) = sanitize(text)?;
    let mut c = load(id)?;
    c.messages.push(Message {
        role,
        at: crate::util::now_rfc3339(),
        text,
        provider: provider.map(str::to_string),
        model: model.map(str::to_string),
        truncated,
    });
    if c.messages.len() > MAX_MESSAGES {
        let cut = c.messages.len() - MAX_MESSAGES;
        c.messages.drain(0..cut);
    }
    // 마지막으로 쓴 모델을 대화의 현재 모델로 둔다 — 다른 모델로 이어가면 바뀐다.
    if let Some(p) = provider {
        c.meta.provider = p.to_string();
    }
    if let Some(m) = model {
        c.meta.model = m.to_string();
    }
    c.meta.updated_at = crate::util::now_rfc3339();
    c.meta.message_count = c.messages.len();
    save(&c)?;
    Ok(c.meta)
}

pub fn delete(id: &str) -> Result<(), ConvError> {
    let p = path_of(id)?;
    match std::fs::remove_file(&p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(ConvError::NotFound(id.to_string()))
        }
        Err(e) => Err(ConvError::Io(e.to_string())),
    }
}

/// 다음 질문(또는 **다른 모델**)에 붙일 대화 맥락을 만든다.
///
/// 전체 대화를 그대로 보내지 않는다 — 최근 `max_messages` 개, 총 `max_chars` 자
/// 안에서 뒤에서부터 채운다. 마스터 플랜의 "AI → APEX 공통 맥락 → 다음 AI" 흐름에서
/// APEX 가 고르는 부분이 이 함수다.
pub fn context_text(c: &Conversation, max_messages: usize, max_chars: usize) -> String {
    let mut picked: Vec<&Message> = Vec::new();
    let mut used = 0usize;
    for m in c.messages.iter().rev().take(max_messages) {
        let cost = m.text.chars().count() + 8;
        if used + cost > max_chars && !picked.is_empty() {
            break;
        }
        used += cost;
        picked.push(m);
    }
    picked.reverse();
    let body = picked
        .iter()
        .map(|m| format!("{}: {}", m.role.label(), m.text))
        .collect::<Vec<_>>()
        .join("\n");
    if body.is_empty() {
        String::new()
    } else {
        format!("[이전 대화 — 제목: {}]\n{}", c.meta.title, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트는 서로의 데이터 디렉터리를 건드리지 않아야 한다.
    /// `paths` 는 프로세스당 한 번만 해석되므로, 디렉터리 자체는 공유하되
    /// 각 테스트가 자기가 만든 대화 id 만 검사한다.
    fn unique_title(tag: &str) -> String {
        format!("{tag}-{}", new_id())
    }

    #[test]
    fn invalid_ids_are_rejected_before_touching_disk() {
        for bad in [
            "../secret",
            "..",
            "a/b",
            "a\\b",
            "UPPER",
            "has space",
            "",
            "sym*bol",
        ] {
            assert_eq!(
                validate_id(bad),
                Err(ConvError::InvalidId(bad.to_string())),
                "{bad} 는 거부돼야 한다"
            );
            assert!(load(bad).is_err());
            assert!(delete(bad).is_err());
        }
    }

    #[test]
    fn generated_ids_are_valid() {
        for _ in 0..5 {
            assert!(validate_id(&new_id()).is_ok());
        }
    }

    #[test]
    fn roundtrip_create_append_load() {
        let c = create(&unique_title("roundtrip"), "claude", "claude-x").unwrap();
        let meta = append(&c.meta.id, Role::User, "CPU 왜 느려?", None, None).unwrap();
        assert_eq!(meta.message_count, 1);
        let meta = append(
            &c.meta.id,
            Role::Assistant,
            "확인할 것은…",
            Some("gpt"),
            Some("gpt-x"),
        )
        .unwrap();
        assert_eq!(meta.message_count, 2);
        // 마지막으로 답한 모델이 대화의 현재 모델이 된다.
        assert_eq!(meta.provider, "gpt");

        let loaded = load(&c.meta.id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, Role::User);
        assert_eq!(loaded.messages[1].provider.as_deref(), Some("gpt"));
        assert!(list().iter().any(|m| m.id == c.meta.id));

        delete(&c.meta.id).unwrap();
        assert!(matches!(load(&c.meta.id), Err(ConvError::NotFound(_))));
    }

    /// 사용자가 키를 붙여넣어도 **파일에는 남지 않는다.**
    #[test]
    fn secrets_never_reach_disk() {
        let c = create(&unique_title("secret"), "claude", "claude-x").unwrap();
        append(
            &c.meta.id,
            Role::User,
            "내 키는 sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA 야",
            None,
            None,
        )
        .unwrap();

        let raw = std::fs::read_to_string(path_of(&c.meta.id).unwrap()).unwrap();
        assert!(
            !raw.contains("sk-ant-api03-AAAA"),
            "키가 평문으로 저장됐다: {raw}"
        );
        let loaded = load(&c.meta.id).unwrap();
        assert!(loaded.messages[0].text.contains("내 키는"));
        delete(&c.meta.id).unwrap();
    }

    #[test]
    fn empty_message_is_refused() {
        let c = create(&unique_title("empty"), "claude", "claude-x").unwrap();
        assert_eq!(
            append(&c.meta.id, Role::User, "   \n ", None, None),
            Err(ConvError::EmptyText)
        );
        delete(&c.meta.id).unwrap();
    }

    #[test]
    fn long_text_is_truncated_and_marked() {
        let c = create(&unique_title("long"), "claude", "claude-x").unwrap();
        let long = "가".repeat(MAX_TEXT_CHARS + 500);
        append(&c.meta.id, Role::User, &long, None, None).unwrap();
        let loaded = load(&c.meta.id).unwrap();
        assert!(loaded.messages[0].truncated);
        assert!(loaded.messages[0].text.contains("잘림"));
        delete(&c.meta.id).unwrap();
    }

    #[test]
    fn message_count_is_capped() {
        let mut c = Conversation::default();
        c.meta.id = "cap-test".into();
        for i in 0..(MAX_MESSAGES + 10) {
            c.messages.push(Message {
                role: Role::User,
                at: "t".into(),
                text: format!("m{i}"),
                provider: None,
                model: None,
                truncated: false,
            });
        }
        if c.messages.len() > MAX_MESSAGES {
            let cut = c.messages.len() - MAX_MESSAGES;
            c.messages.drain(0..cut);
        }
        assert_eq!(c.messages.len(), MAX_MESSAGES);
        assert_eq!(c.messages[0].text, "m10", "오래된 것부터 버린다");
    }

    #[test]
    fn context_text_respects_limits_and_keeps_order() {
        let mut c = Conversation::default();
        c.meta.title = "제목".into();
        for i in 0..10 {
            c.messages.push(Message {
                role: if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                at: "t".into(),
                text: format!("message-{i}"),
                provider: None,
                model: None,
                truncated: false,
            });
        }
        let ctx = context_text(&c, 3, 10_000);
        assert!(ctx.contains("message-9"));
        assert!(!ctx.contains("message-6"), "최근 3개만 담아야 한다");
        let i7 = ctx.find("message-7").unwrap();
        let i8 = ctx.find("message-8").unwrap();
        assert!(i7 < i8, "시간 순서를 유지해야 한다");

        // 글자 상한이 작으면 최소 한 개는 담되 그 이상 넘지 않는다.
        let tight = context_text(&c, 10, 20);
        assert!(tight.contains("message-9"));
        assert!(!tight.contains("message-8"));
    }

    #[test]
    fn empty_conversation_has_no_context() {
        let c = Conversation::default();
        assert!(context_text(&c, 5, 1000).is_empty());
    }
}
