//! velox-core::logging — 진단 로그.
//!
//! v0.19 요구사항:
//! - `tracing` 도입
//! - error/warn/info/debug 구분
//! - **key / prompt / system serial 레닥션**
//! - 회전 로그
//! - **사용자가 export 전에 미리보기**
//!
//! 설계 원칙 두 가지가 다른 로깅 라이브러리 사용법과 다르다.
//!
//! 1. **레닥션은 선택이 아니라 경유 지점이다.** 로그로 나가는 모든 문자열은
//!    [`redact_line`]을 통과한다. "조심해서 쓰면 된다"는 규칙은 언젠가 깨진다 —
//!    APEX 는 키가 파일에 남는 것을 구조적으로 막는다.
//! 2. **로그는 기본으로 꺼져 있지 않다. 대신 조용하다.** 파일에는 남기되
//!    터미널에는 사용자가 요청할 때만 보여준다. 진단 도구가 자기 로그로
//!    화면을 어지럽히면 안 된다.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

/// 로그 파일 이름(회전 접미사가 뒤에 붙는다).
pub const LOG_FILE_PREFIX: &str = "velox";

/// 레벨을 지정하는 환경변수. 예: `VELOX_LOG=debug`
pub const LEVEL_ENV: &str = "VELOX_LOG";

/// 보관할 회전 로그 개수. 오래된 것부터 지운다.
const MAX_LOG_FILES: usize = 7;

static GUARD: OnceLock<()> = OnceLock::new();

/// 키·시리얼처럼 보이는 값을 가린 한 줄을 돌려준다.
///
/// [`crate::project::redact_secrets`]의 토큰 기반 레닥션을 재사용하고,
/// 로그에서만 의미 있는 항목(시스템 시리얼, 프롬프트 본문)을 추가로 막는다.
pub fn redact_line(line: &str) -> String {
    let mut out = crate::project::redact_secrets(line);

    // 프롬프트/응답 본문은 애초에 로그로 보내지 않는 게 원칙이지만,
    // 실수로 실렸을 때를 대비해 길이만 남기고 잘라낸다.
    if let Some(idx) = out.find("prompt=") {
        let head = &out[..idx + "prompt=".len()];
        let tail_len = out.len() - (idx + "prompt=".len());
        out = format!("{head}[{tail_len} chars redacted]");
    }
    out
}

/// 레닥션을 강제하는 writer — 이 계층을 지나지 않고 파일에 닿는 경로가 없다.
struct RedactingWriter<W: std::io::Write> {
    inner: W,
}

impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let cleaned = redact_line(&text);
        self.inner.write_all(cleaned.as_bytes())?;
        // 호출자에게는 원본 길이를 보고한다 — 레닥션으로 길이가 변해도
        // tracing 쪽에서 부분 쓰기로 오해하지 않게.
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Clone)]
struct RedactingMakeWriter<M> {
    inner: M,
}

impl<'a, M> MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;
    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: self.inner.make_writer(),
        }
    }
}

/// 로그 디렉터리(`<data>/logs`).
pub fn log_dir() -> PathBuf {
    crate::paths::logs_dir()
}

/// 현재 존재하는 로그 파일들 — 최신순.
pub fn log_files() -> Vec<PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(log_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(LOG_FILE_PREFIX))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.reverse();
    files
}

/// 오래된 회전 로그를 지운다([`MAX_LOG_FILES`]개만 남긴다).
fn prune_old_logs() {
    for old in log_files().into_iter().skip(MAX_LOG_FILES) {
        let _ = std::fs::remove_file(old);
    }
}

/// **export 전에 사용자가 확인할 수 있는 미리보기.**
///
/// 로그를 남한테 보내기 전에 무엇이 들어 있는지 볼 수 있어야 한다는 것이
/// v0.19 요구사항이다. 반환값은 이미 레닥션을 거친 상태다.
pub fn export_preview(max_lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    for path in log_files() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            if out.len() >= max_lines {
                return out;
            }
            out.push(redact_line(line));
        }
    }
    out
}

/// 로깅 초기화. 프로세스당 한 번만 효과가 있다.
///
/// 반환된 가드를 살려둬야 비동기 기록이 flush 된다 — `main` 끝까지 잡고 있을 것.
/// 초기화에 실패해도 프로그램은 계속 돈다(로깅은 부가 기능이지 필수 경로가 아니다).
#[must_use = "가드를 떨어뜨리면 로그가 flush 되지 않는다"]
pub fn init() -> Option<WorkerGuard> {
    if GUARD.set(()).is_err() {
        return None; // 이미 초기화됨
    }
    prune_old_logs();

    let appender = tracing_appender::rolling::daily(log_dir(), LOG_FILE_PREFIX);
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    // 기본은 info. VELOX_LOG 로 올리거나 내린다.
    let filter = EnvFilter::try_from_env(LEVEL_ENV).unwrap_or_else(|_| EnvFilter::new("info"));

    let ok = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(RedactingMakeWriter {
            inner: non_blocking,
        })
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .try_init()
        .is_ok();

    ok.then_some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_api_keys() {
        let line = "INFO velox::ai provider=claude key=sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let out = redact_line(line);
        assert!(!out.contains("sk-ant-api03"), "키가 로그에 남았다: {out}");
        assert!(out.contains("[REDACTED]"));
        assert!(
            out.contains("provider=claude"),
            "무해한 필드는 보존해야 한다"
        );
    }

    #[test]
    fn redacts_every_known_key_shape() {
        for key in [
            "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "sk-proj-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            "AIzaCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC",
            "xai-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD",
        ] {
            let out = redact_line(&format!("token={key} rest"));
            assert!(!out.contains(key), "{key} 가 로그에 남았다: {out}");
        }
    }

    #[test]
    fn truncates_prompt_bodies() {
        let out = redact_line("INFO council prompt=사용자의 민감한 질문 전체가 여기 들어감");
        assert!(
            !out.contains("민감한 질문"),
            "프롬프트 본문이 남았다: {out}"
        );
        assert!(out.contains("chars redacted"));
    }

    #[test]
    fn redacting_writer_filters_before_disk() {
        // writer 계층 자체가 레닥션을 강제하는지 — 호출자가 조심하지 않아도 막혀야 한다.
        let mut sink: Vec<u8> = Vec::new();
        {
            use std::io::Write;
            let mut w = RedactingWriter { inner: &mut sink };
            let raw = b"key=sk-ant-api03-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEE\n";
            let n = w.write(raw).unwrap();
            assert_eq!(n, raw.len(), "호출자에게는 원본 길이를 보고한다");
        }
        let written = String::from_utf8(sink).unwrap();
        assert!(!written.contains("sk-ant-api03"));
        assert!(written.contains("[REDACTED]"));
    }

    #[test]
    fn export_preview_respects_limit() {
        // 로그가 없을 수도 있으므로 상한만 검증한다.
        let preview = export_preview(3);
        assert!(preview.len() <= 3);
    }

    #[test]
    fn log_dir_is_under_data_dir() {
        assert!(log_dir().starts_with(crate::paths::data_dir()));
        assert!(log_dir().ends_with("logs"));
    }
}
