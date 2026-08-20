//! velox-core::paths — 상태 파일 위치 결정.
//!
//! **원칙: 실행 위치(CWD)에 상태를 저장하지 않는다.**
//!
//! v0.18까지는 모든 상태 파일이 맨 파일명(`velox_policies.json` 등)이라
//! 프로세스의 현재 작업 디렉터리에 저장됐다. 사용자가 앱을 다른 폴더에서 켜면
//! 이미 동의한 정책을 찾지 못해 deny-by-default 로 떨어졌다 — 사용자 입장에선
//! "동의했는데 왜 또 물어보나"가 된다. 이 모듈이 그 문제를 없앤다.
//!
//! 해석 우선순위:
//! 1. `VELOX_DATA_DIR` 환경변수 — 명시적 지정(테스트·포터블 실행용). 마이그레이션 안 함.
//! 2. Windows `%LOCALAPPDATA%\APEX\Velox`
//! 3. Unix `$XDG_DATA_HOME/apex/velox` 또는 `$HOME/.local/share/apex/velox`
//! 4. 전부 실패하면 CWD — 기능을 멈추지 않기 위한 최후 폴백.
//!
//! 외부 크레이트를 쓰지 않는다(공유 매니페스트 변경 최소화).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// 데이터 디렉터리를 명시적으로 지정하는 환경변수.
pub const DATA_DIR_ENV: &str = "VELOX_DATA_DIR";

/// CWD 에서 데이터 디렉터리로 옮겨야 하는 v0.18 이전 상태 파일들.
///
/// 로그·CSV 등 CLI 소유 파일은 여기 넣지 않는다(해당 크레이트가 옮긴다).
const LEGACY_FILES: &[&str] = &[
    "velox_policies.json",
    "velox_models.json",
    "velox_providers.json",
    "velox_ledger.json",
    "velox_pricing.json",
    "velox_checkpoints.txt",
];

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static MIGRATED: OnceLock<Vec<String>> = OnceLock::new();

/// 환경변수/OS 규약으로 데이터 디렉터리 후보를 고른다. 디렉터리 생성은 하지 않는다.
fn resolve_dir() -> (PathBuf, bool) {
    // (경로, 마이그레이션 대상 여부)
    if let Some(dir) = non_empty_env(DATA_DIR_ENV) {
        // 호출자가 위치를 직접 통제하는 경우 — 레거시 이전을 하지 않는다.
        return (PathBuf::from(dir), false);
    }
    if let Some(base) = non_empty_env("LOCALAPPDATA") {
        return (PathBuf::from(base).join("APEX").join("Velox"), true);
    }
    if let Some(base) = non_empty_env("XDG_DATA_HOME") {
        return (PathBuf::from(base).join("apex").join("velox"), true);
    }
    if let Some(home) = non_empty_env("HOME") {
        return (
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("apex")
                .join("velox"),
            true,
        );
    }
    // 최후 폴백 — 예전과 동일하게 CWD. 여기서는 옮길 것이 없다.
    (PathBuf::from("."), false)
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// 상태 파일을 담는 디렉터리. 없으면 만든다.
///
/// 최초 호출 시 CWD 에 남아 있는 v0.18 이전 파일을 한 번 옮긴다([`migrated_files`]).
pub fn data_dir() -> &'static Path {
    DATA_DIR.get_or_init(|| {
        let (dir, migrate) = resolve_dir();
        // 생성 실패해도 계속 진행 — 이후 파일 I/O 가 각자 실패를 처리한다.
        let _ = std::fs::create_dir_all(&dir);
        let moved = if migrate {
            migrate_legacy(&dir)
        } else {
            Vec::new()
        };
        let _ = MIGRATED.set(moved);
        dir
    })
}

/// 상태 파일의 전체 경로.
pub fn resolve(name: &str) -> PathBuf {
    data_dir().join(name)
}

/// 이번 프로세스에서 CWD → 데이터 디렉터리로 옮겨진 파일 목록.
///
/// [`data_dir`] 이 아직 불리지 않았다면 먼저 호출해 초기화한다.
pub fn migrated_files() -> &'static [String] {
    let _ = data_dir();
    MIGRATED.get().map(Vec::as_slice).unwrap_or(&[])
}

/// CWD 에 남은 레거시 파일을 데이터 디렉터리로 이동한다.
///
/// 규칙:
/// - 대상에 이미 파일이 있으면 **건드리지 않는다**(새 위치가 항상 우선).
/// - rename 이 실패하면(다른 볼륨 등) 복사 후 원본 삭제로 폴백한다.
/// - 어떤 실패도 조용히 넘긴다 — 이전은 편의 기능이지 필수 경로가 아니다.
fn migrate_legacy(dir: &Path) -> Vec<String> {
    // CWD 가 곧 대상 디렉터리면 옮길 것이 없다.
    if std::env::current_dir().map(|c| c == dir).unwrap_or(false) {
        return Vec::new();
    }
    let mut moved = Vec::new();
    for name in LEGACY_FILES {
        let src = Path::new(name);
        if !src.is_file() {
            continue;
        }
        let dst = dir.join(name);
        if dst.exists() {
            continue; // 새 위치 우선 — 덮어쓰지 않는다.
        }
        let ok = std::fs::rename(src, &dst).is_ok()
            || (std::fs::copy(src, &dst).is_ok() && std::fs::remove_file(src).is_ok());
        if ok {
            moved.push((*name).to_string());
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_joins_data_dir() {
        let p = resolve("velox_policies.json");
        assert!(p.ends_with("velox_policies.json"));
        assert_eq!(p.parent(), Some(data_dir()));
    }

    #[test]
    fn data_dir_is_stable_across_calls() {
        assert_eq!(data_dir(), data_dir());
    }

    #[test]
    fn data_dir_is_not_bare_cwd_when_localappdata_present() {
        // 개발/CI 환경에는 LOCALAPPDATA 또는 HOME 이 있으므로
        // 상태 파일이 실행 위치에 그대로 떨어지면 안 된다.
        if non_empty_env("LOCALAPPDATA").is_some() || non_empty_env("HOME").is_some() {
            assert_ne!(data_dir(), Path::new("."));
        }
    }

    #[test]
    fn explicit_env_dir_skips_migration() {
        // resolve_dir 은 순수 함수라 OnceLock 과 무관하게 검사할 수 있다.
        let prev = std::env::var(DATA_DIR_ENV).ok();
        unsafe { std::env::set_var(DATA_DIR_ENV, "velox-test-data-dir") };
        let (dir, migrate) = resolve_dir();
        assert_eq!(dir, PathBuf::from("velox-test-data-dir"));
        assert!(!migrate, "명시 지정 시 레거시 이전을 하지 않는다");
        match prev {
            Some(v) => unsafe { std::env::set_var(DATA_DIR_ENV, v) },
            None => unsafe { std::env::remove_var(DATA_DIR_ENV) },
        }
    }

    #[test]
    fn blank_env_is_ignored() {
        let prev = std::env::var(DATA_DIR_ENV).ok();
        unsafe { std::env::set_var(DATA_DIR_ENV, "   ") };
        assert!(non_empty_env(DATA_DIR_ENV).is_none());
        match prev {
            Some(v) => unsafe { std::env::set_var(DATA_DIR_ENV, v) },
            None => unsafe { std::env::remove_var(DATA_DIR_ENV) },
        }
    }
}
