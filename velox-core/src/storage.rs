//! velox-core::storage — 저장 대상 등록과 검증된 단방향 전송 (프로그램 3 "PC 연결과 저장").
//!
//! 마스터 플랜 프로그램 3의 첫 조각이다: "이 노트를 집 서버에도 저장해 줘".
//!
//! **범위를 좁게 잡았다.** 이 모듈은 새 네트워크 프로토콜을 만들지 않고, 포트를 열지 않고,
//! 장치 인증을 새로 발명하지 않는다. 운영체제가 이미 닿을 수 있는 폴더(로컬 폴더, 다른 PC 의
//! 공유 폴더 `\\서버\공유`)를 **저장 대상**으로 등록하고, 거기에 파일을 안전하게 보낸다.
//! 누가 그 폴더에 쓸 수 있는지는 Windows 의 공유 권한이 정한다 — 같은 네트워크에 있다는
//! 이유만으로 접근하지 않는다는 플랜의 원칙을 OS 권한으로 지킨다.
//!
//! 설계에서 양보하지 않는 것:
//!
//! 1. **덮어쓰지 않는다.** 같은 이름의 다른 파일이 있으면 새 이름으로 나란히 저장하고
//!    그렇게 했다고 기록한다. 내용이 완전히 같으면 복사하지 않고 "이미 있음"으로 끝낸다.
//! 2. **반쯤 쓰인 파일을 남기지 않는다.** 임시 이름으로 복사하고, 원본과 바이트 단위로
//!    비교해 같을 때만 최종 이름으로 바꾼다. 검증에 실패하면 임시 파일을 지운다.
//! 3. **끊겼으면 기다린다.** 대상에 닿을 수 없으면 실패가 아니라 `Pending` 으로 남기고
//!    `retry` 가 다시 시도한다. 같은 전송을 두 번 해도 파일이 중복되지 않는다(1번 규칙).
//! 4. **단방향 전송만 한다.** 백업·양방향 동기화와는 다른 동작이다. 원본을 지우거나
//!    대상의 파일을 지우는 일은 없다.
//! 5. **비밀 파일은 기본으로 거부한다.** `.env`, 키 파일 이름은 명시적으로 허용해야 보낸다.
//! 6. **기록 파일이 손상됐으면 덮어쓰지 않는다.**

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const STORAGE_FILE: &str = "velox_storage.json";
/// 전송 기록 보관 상한. 넘으면 끝난 것 중 오래된 것부터 버린다(대기 중인 것은 버리지 않는다).
pub const MAX_TRANSFERS: usize = 500;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Destination {
    pub id: String,
    /// 사람이 알아보는 이름("집 서버", "백업 디스크").
    pub name: String,
    /// 저장할 폴더. 로컬 경로 또는 `\\서버\공유\폴더`.
    pub path: String,
    pub added_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    /// 복사했고 원본과 바이트 단위로 같음을 확인했다.
    Completed,
    /// 같은 내용의 파일이 이미 있어 복사하지 않았다.
    AlreadyThere,
    /// 대상에 닿을 수 없어 기다리는 중. 실패가 아니다.
    Pending,
    Failed,
}

impl TransferState {
    pub fn label(self) -> &'static str {
        match self {
            TransferState::Completed => "완료",
            TransferState::AlreadyThere => "이미 있음",
            TransferState::Pending => "대기",
            TransferState::Failed => "실패",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Transfer {
    pub id: String,
    pub source: String,
    pub dest_id: String,
    pub file_name: String,
    pub bytes: u64,
    pub state: TransferState,
    /// 사람이 읽는 설명(왜 대기인지, 왜 실패했는지).
    pub detail: String,
    /// 실제로 저장된 전체 경로. 완료·이미 있음일 때만 있다.
    #[serde(default)]
    pub saved_as: Option<String>,
    /// 이름이 겹쳐 새 이름으로 저장했는가.
    #[serde(default)]
    pub renamed_for_conflict: bool,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub attempts: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct StorageBook {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub destinations: Vec<Destination>,
    #[serde(default)]
    pub transfers: Vec<Transfer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    Invalid(String),
    NotFound(String),
    Corrupt(String),
    Io(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Invalid(m) => write!(f, "{m}"),
            StorageError::NotFound(id) => write!(
                f,
                "그런 저장 대상이 없습니다: {id}\n  다음 행동: `velox storage list` 로 등록된 대상을 확인하세요."
            ),
            StorageError::Corrupt(e) => write!(
                f,
                "저장 기록 파일을 읽을 수 없습니다: {e}\n  다음 행동: 기록을 보호하려고 중단했습니다. {STORAGE_FILE} 을 고치거나 다른 이름으로 옮긴 뒤 다시 시도하세요."
            ),
            StorageError::Io(e) => write!(
                f,
                "저장 기록을 쓰지 못했습니다: {e}\n  다음 행동: 디스크 공간과 권한을 확인하세요."
            ),
        }
    }
}

fn invalid<T>(m: &str) -> Result<T, StorageError> {
    Err(StorageError::Invalid(m.to_string()))
}

// ---------------- 기록 파일 ----------------

fn book_path() -> PathBuf {
    crate::paths::resolve(STORAGE_FILE)
}

pub fn load() -> Result<StorageBook, StorageError> {
    load_from(&book_path())
}

fn load_from(p: &Path) -> Result<StorageBook, StorageError> {
    match std::fs::read_to_string(p) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| StorageError::Corrupt(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(StorageBook::default()),
        Err(e) => Err(StorageError::Io(e.to_string())),
    }
}

fn save_to(p: &Path, book: &StorageBook) -> Result<(), StorageError> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| StorageError::Io(e.to_string()))?;
    }
    let json = serde_json::to_string_pretty(book).map_err(|e| StorageError::Io(e.to_string()))?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| StorageError::Io(e.to_string()))?;
    std::fs::rename(&tmp, p).map_err(|e| StorageError::Io(e.to_string()))
}

fn trim_log(book: &mut StorageBook) {
    while book.transfers.len() > MAX_TRANSFERS {
        // 대기 중인 전송은 버리지 않는다 — 버리면 조용히 사라진 작업이 된다.
        match book
            .transfers
            .iter()
            .position(|t| t.state != TransferState::Pending)
        {
            Some(i) => {
                book.transfers.remove(i);
            }
            None => break,
        }
    }
}

// ---------------- 저장 대상 ----------------

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

/// 저장 대상 등록. 등록 시점에 폴더에 닿을 수 있어야 한다 — 오타를 그 자리에서 잡는다.
pub fn add_destination(name: &str, path: &str) -> Result<Destination, StorageError> {
    add_destination_at(&book_path(), name, path, &crate::util::now_rfc3339())
}

fn add_destination_at(
    book_file: &Path,
    name: &str,
    path: &str,
    now: &str,
) -> Result<Destination, StorageError> {
    let name: String = crate::project::redact_secrets(name.trim())
        .chars()
        .take(60)
        .collect();
    if name.is_empty() {
        return invalid(
            "저장 대상 이름이 비었습니다.\n  다음 행동: velox storage add \"집 서버\" \\\\서버\\공유\\APEX 처럼 이름과 폴더를 주세요.",
        );
    }
    let path = path.trim();
    let p = Path::new(path);
    if !p.is_absolute() {
        return invalid(
            "폴더는 전체 경로여야 합니다.\n  다음 행동: D:\\Backup 또는 \\\\서버\\공유\\폴더 처럼 입력하세요.",
        );
    }
    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return invalid("경로에 `..` 를 쓸 수 없습니다.");
    }
    if !p.is_dir() {
        return invalid(
            "그 폴더에 닿을 수 없습니다.\n  다음 행동: 폴더가 있는지, 다른 PC 라면 공유가 켜져 있고 탐색기에서 열리는지 확인하세요. (등록할 때는 연결돼 있어야 합니다)",
        );
    }

    let mut book = load_from(book_file)?;
    let base = match slugify(&name) {
        // 한글 이름은 영문 id 로 바꿀 수 없다. 한두 글자짜리 id 는 알아볼 수 없으니 기본값을 쓴다.
        s if s.chars().count() < 3 => "dest".to_string(),
        s => s,
    };
    let mut id = base.clone();
    let mut n = 2;
    while book.destinations.iter().any(|d| d.id == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    let dest = Destination {
        id,
        name,
        path: path.to_string(),
        added_at: now.to_string(),
    };
    book.destinations.push(dest.clone());
    book.version = 1;
    save_to(book_file, &book)?;
    Ok(dest)
}

/// 등록을 지운다. **대상 폴더의 파일은 건드리지 않는다.**
pub fn remove_destination(id: &str) -> Result<Destination, StorageError> {
    remove_destination_at(&book_path(), id)
}

fn remove_destination_at(book_file: &Path, id: &str) -> Result<Destination, StorageError> {
    let mut book = load_from(book_file)?;
    let Some(i) = book.destinations.iter().position(|d| matches_dest(d, id)) else {
        return Err(StorageError::NotFound(id.to_string()));
    };
    let gone = book.destinations.remove(i);
    save_to(book_file, &book)?;
    Ok(gone)
}

/// id 또는 사람이 붙인 이름으로 대상을 찾는다. 이름이 한글이면 id 를 외울 필요가 없다.
fn matches_dest(d: &Destination, key: &str) -> bool {
    let key = key.trim();
    d.id == key || d.name.eq_ignore_ascii_case(key)
}

/// 지금 그 폴더에 닿을 수 있는가.
pub fn is_reachable(dest: &Destination) -> bool {
    Path::new(&dest.path).is_dir()
}

// ---------------- 전송 ----------------

/// 두 파일이 바이트 단위로 같은가. 크기부터 보고, 같으면 내용을 끝까지 비교한다.
fn files_equal(a: &Path, b: &Path) -> std::io::Result<bool> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut fa, mut fb) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = read_full(&mut fa, &mut ba)?;
        let m = read_full(&mut fb, &mut bb)?;
        if n != m || ba[..n] != bb[..m] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match f.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// 이름이 겹칠 때 쓸 새 이름: `보고서 (APEX 20261005-053000).md`.
fn conflict_name(dir: &Path, file_name: &str, stamp: &str) -> PathBuf {
    let p = Path::new(file_name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut n = 1;
    loop {
        let suffix = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        let candidate = dir.join(format!("{stem} (APEX {stamp}{suffix}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

enum CopyOutcome {
    Completed { saved_as: PathBuf, renamed: bool },
    AlreadyThere { saved_as: PathBuf },
}

/// 임시 이름으로 복사 → 원본과 비교 → 최종 이름으로 변경.
fn copy_verified(source: &Path, dir: &Path, id: &str, stamp: &str) -> std::io::Result<CopyOutcome> {
    let file_name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut target = dir.join(&file_name);
    let mut renamed = false;
    if target.exists() {
        if target.is_file() && files_equal(source, &target)? {
            return Ok(CopyOutcome::AlreadyThere { saved_as: target });
        }
        // 같은 이름의 다른 파일 — 덮어쓰지 않고 나란히 둔다.
        target = conflict_name(dir, &file_name, stamp);
        renamed = true;
    }

    let tmp = dir.join(format!(".apex-part-{id}"));
    let result = (|| {
        std::fs::copy(source, &tmp)?;
        if !files_equal(source, &tmp)? {
            return Err(std::io::Error::other(
                "복사한 파일이 원본과 다릅니다(검증 실패)",
            ));
        }
        if target.exists() {
            // 복사하는 사이 누가 같은 이름을 만들었다 — 그래도 덮어쓰지 않는다.
            target = conflict_name(dir, &file_name, stamp);
            renamed = true;
        }
        std::fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        // 반쯤 쓰인 파일을 남기지 않는다.
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| CopyOutcome::Completed {
        saved_as: target,
        renamed,
    })
}

fn new_transfer_id(book: &StorageBook, now: &str) -> String {
    let digits: String = now.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut n = book.transfers.len();
    loop {
        let id = format!("t{digits}-{n}");
        if !book.transfers.iter().any(|t| t.id == id) {
            return id;
        }
        n += 1;
    }
}

fn stamp_of(now: &str) -> String {
    // 2026-10-05T05:30:00Z → 20261005-053000
    let d: String = now.chars().filter(|c| c.is_ascii_digit()).collect();
    if d.len() >= 14 {
        format!("{}-{}", &d[..8], &d[8..14])
    } else {
        d
    }
}

/// 한 번의 시도. `t` 를 결과로 갱신한다.
fn attempt(t: &mut Transfer, dest: Option<&Destination>, now: &str) {
    t.attempts += 1;
    let fail = |t: &mut Transfer, why: String| {
        t.state = TransferState::Failed;
        t.detail = why;
        t.finished_at = Some(now.to_string());
    };

    let Some(dest) = dest else {
        return fail(t, "저장 대상 등록이 삭제됐습니다".into());
    };
    let source = PathBuf::from(&t.source);
    match std::fs::symlink_metadata(&source) {
        Ok(m) if m.file_type().is_symlink() => {
            return fail(t, "원본이 링크입니다 — 실제 파일만 보냅니다".into());
        }
        Ok(m) if !m.is_file() => return fail(t, "원본이 파일이 아닙니다".into()),
        Ok(m) => t.bytes = m.len(),
        Err(_) => {
            return fail(t, "원본 파일이 없습니다(옮겨졌거나 지워졌습니다)".into());
        }
    }

    let dir = Path::new(&dest.path);
    if !dir.is_dir() {
        t.state = TransferState::Pending;
        t.detail = format!(
            "\"{}\" 에 닿을 수 없습니다 — 연결되면 `velox storage retry` 로 이어집니다",
            dest.name
        );
        return;
    }

    match copy_verified(&source, dir, &t.id, &stamp_of(now)) {
        Ok(CopyOutcome::Completed { saved_as, renamed }) => {
            t.state = TransferState::Completed;
            t.renamed_for_conflict = renamed;
            t.detail = if renamed {
                "같은 이름의 다른 파일이 있어 새 이름으로 저장했습니다(기존 파일은 그대로)".into()
            } else {
                "복사 후 원본과 바이트 단위로 같음을 확인했습니다".into()
            };
            t.saved_as = Some(saved_as.to_string_lossy().into_owned());
            t.finished_at = Some(now.to_string());
        }
        Ok(CopyOutcome::AlreadyThere { saved_as }) => {
            t.state = TransferState::AlreadyThere;
            t.detail = "같은 내용의 파일이 이미 있어 복사하지 않았습니다".into();
            t.saved_as = Some(saved_as.to_string_lossy().into_owned());
            t.finished_at = Some(now.to_string());
        }
        Err(e) => {
            if dir.is_dir() {
                fail(t, format!("복사하지 못했습니다: {e}"));
            } else {
                // 복사 도중 끊겼다 — 실패가 아니라 대기로 돌린다.
                t.state = TransferState::Pending;
                t.detail = format!(
                    "전송 중 \"{}\" 연결이 끊겼습니다 — 다시 시도됩니다",
                    dest.name
                );
            }
        }
    }
}

/// 파일 하나를 저장 대상으로 보낸다.
///
/// `allow_secret` 이 false 면 `.env`·키 파일 같은 이름은 거부한다.
pub fn send(source: &str, dest_id: &str, allow_secret: bool) -> Result<Transfer, StorageError> {
    send_at(
        &book_path(),
        source,
        dest_id,
        allow_secret,
        &crate::util::now_rfc3339(),
    )
}

fn send_at(
    book_file: &Path,
    source: &str,
    dest_id: &str,
    allow_secret: bool,
    now: &str,
) -> Result<Transfer, StorageError> {
    let mut book = load_from(book_file)?;
    let Some(dest) = book
        .destinations
        .iter()
        .find(|d| matches_dest(d, dest_id))
        .cloned()
    else {
        return Err(StorageError::NotFound(dest_id.to_string()));
    };

    let src = Path::new(source.trim());
    let meta = match std::fs::symlink_metadata(src) {
        Ok(m) => m,
        Err(_) => {
            return invalid(
                "보낼 파일을 찾을 수 없습니다.\n  다음 행동: 경로를 확인하세요. 폴더가 아니라 파일 하나를 지정해야 합니다.",
            );
        }
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return invalid(
            "파일 하나만 보낼 수 있습니다(폴더·링크는 안 됩니다).\n  다음 행동: 보낼 파일의 경로를 지정하세요.",
        );
    }
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !allow_secret && crate::project::is_secret_filename(&file_name) {
        return invalid(
            "비밀값이 든 파일로 보입니다(.env·키 파일).\n  다음 행동: 정말 보내려면 --allow-secret 을 붙이세요. 공유 폴더에 키를 두는 것은 권하지 않습니다.",
        );
    }
    let absolute = std::fs::canonicalize(src)
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .unwrap_or_else(|_| source.trim().to_string());

    let mut t = Transfer {
        id: new_transfer_id(&book, now),
        source: absolute,
        dest_id: dest.id.clone(),
        file_name,
        bytes: meta.len(),
        state: TransferState::Pending,
        detail: String::new(),
        saved_as: None,
        renamed_for_conflict: false,
        created_at: now.to_string(),
        finished_at: None,
        attempts: 0,
    };
    attempt(&mut t, Some(&dest), now);
    book.transfers.push(t.clone());
    book.version = 1;
    trim_log(&mut book);
    save_to(book_file, &book)?;
    Ok(t)
}

/// 대기 중인 전송을 다시 시도한다. 시도한 전송들을 돌려준다.
pub fn retry_pending() -> Result<Vec<Transfer>, StorageError> {
    retry_pending_at(&book_path(), &crate::util::now_rfc3339())
}

fn retry_pending_at(book_file: &Path, now: &str) -> Result<Vec<Transfer>, StorageError> {
    let mut book = load_from(book_file)?;
    let dests = book.destinations.clone();
    let mut tried = Vec::new();
    for t in book
        .transfers
        .iter_mut()
        .filter(|t| t.state == TransferState::Pending)
    {
        attempt(t, dests.iter().find(|d| d.id == t.dest_id), now);
        tried.push(t.clone());
    }
    if !tried.is_empty() {
        save_to(book_file, &book)?;
    }
    Ok(tried)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Env {
        root: PathBuf,
        book: PathBuf,
        src: PathBuf,
        dest: PathBuf,
    }

    fn env(tag: &str) -> Env {
        let root = std::env::temp_dir().join(format!(
            "velox-storage-{tag}-{}-{}",
            std::process::id(),
            crate::ledger::now_unix()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (src, dest) = (root.join("src"), root.join("dest"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        Env {
            book: root.join(STORAGE_FILE),
            root,
            src,
            dest,
        }
    }

    fn file(dir: &Path, name: &str, body: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p.to_string_lossy().into_owned()
    }

    fn register(e: &Env) -> Destination {
        add_destination_at(&e.book, "집 서버", &e.dest.to_string_lossy(), "t0").unwrap()
    }

    const NOW: &str = "2026-10-05T05:30:00Z";

    #[test]
    fn send_copies_and_verifies() {
        let e = env("send");
        let d = register(&e);
        assert_eq!(d.id, "dest", "한글 이름은 기본 id 로 떨어진다");
        let f = file(&e.src, "노트.md", "CPU 연구");

        let t = send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        assert_eq!(t.state, TransferState::Completed);
        assert!(!t.renamed_for_conflict);
        assert_eq!(t.bytes, "CPU 연구".len() as u64);
        assert_eq!(
            std::fs::read_to_string(e.dest.join("노트.md")).unwrap(),
            "CPU 연구"
        );
        // 임시 파일이 남지 않는다.
        assert!(
            !std::fs::read_dir(&e.dest)
                .unwrap()
                .flatten()
                .any(|x| x.file_name().to_string_lossy().starts_with(".apex-part-"))
        );
        // 원본은 그대로다(단방향 전송은 원본을 지우지 않는다).
        assert!(Path::new(&f).exists());
        let _ = std::fs::remove_dir_all(&e.root);
    }

    /// 같은 전송을 두 번 해도 파일이 중복되지 않는다.
    #[test]
    fn identical_file_is_not_copied_again() {
        let e = env("same");
        let d = register(&e);
        let f = file(&e.src, "a.txt", "같은 내용");
        send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        let again = send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        assert_eq!(again.state, TransferState::AlreadyThere);
        assert_eq!(std::fs::read_dir(&e.dest).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&e.root);
    }

    /// **덮어쓰지 않는다** — 이름이 같고 내용이 다르면 둘 다 남는다.
    #[test]
    fn different_file_with_same_name_is_kept_alongside() {
        let e = env("conflict");
        let d = register(&e);
        std::fs::write(e.dest.join("a.txt"), "서버에 있던 것").unwrap();
        let f = file(&e.src, "a.txt", "새로 보내는 것");

        let t = send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        assert_eq!(t.state, TransferState::Completed);
        assert!(t.renamed_for_conflict);
        assert_eq!(
            std::fs::read_to_string(e.dest.join("a.txt")).unwrap(),
            "서버에 있던 것",
            "기존 파일은 그대로여야 한다"
        );
        let saved = t.saved_as.unwrap();
        assert!(saved.contains("a (APEX 20261005-053000).txt"), "{saved}");
        assert_eq!(std::fs::read_to_string(&saved).unwrap(), "새로 보내는 것");

        // 같은 충돌이 또 나도 서로 덮어쓰지 않는다.
        let f2 = file(&e.src, "a.txt", "세 번째 내용");
        let t2 = send_at(&e.book, &f2, &d.id, false, NOW).unwrap();
        assert!(t2.saved_as.unwrap().contains("053000-2"));
        assert_eq!(std::fs::read_dir(&e.dest).unwrap().count(), 3);
        let _ = std::fs::remove_dir_all(&e.root);
    }

    /// 끊겼으면 실패가 아니라 대기. 연결되면 retry 가 끝낸다.
    #[test]
    fn unreachable_destination_queues_and_retry_completes() {
        let e = env("queue");
        let d = register(&e);
        let f = file(&e.src, "q.txt", "대기열");
        std::fs::remove_dir_all(&e.dest).unwrap(); // 연결 끊김

        let t = send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        assert_eq!(t.state, TransferState::Pending);
        assert!(t.detail.contains("집 서버"));

        // 아직 끊겨 있으면 계속 대기.
        let tried = retry_pending_at(&e.book, NOW).unwrap();
        assert_eq!(tried[0].state, TransferState::Pending);
        assert_eq!(tried[0].attempts, 2);

        std::fs::create_dir_all(&e.dest).unwrap(); // 복귀
        let tried = retry_pending_at(&e.book, NOW).unwrap();
        assert_eq!(tried[0].state, TransferState::Completed);
        assert_eq!(
            std::fs::read_to_string(e.dest.join("q.txt")).unwrap(),
            "대기열"
        );

        // 더 시도할 것이 없다.
        assert!(retry_pending_at(&e.book, NOW).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn pending_transfer_fails_clearly_if_the_source_disappears() {
        let e = env("gone");
        let d = register(&e);
        let f = file(&e.src, "g.txt", "x");
        std::fs::remove_dir_all(&e.dest).unwrap();
        send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        std::fs::remove_file(&f).unwrap();
        std::fs::create_dir_all(&e.dest).unwrap();

        let tried = retry_pending_at(&e.book, NOW).unwrap();
        assert_eq!(tried[0].state, TransferState::Failed);
        assert!(tried[0].detail.contains("원본 파일이 없습니다"));
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn secret_files_and_folders_are_refused() {
        let e = env("secret");
        let d = register(&e);
        let secret = file(&e.src, ".env", "KEY=1");
        assert!(
            matches!(send_at(&e.book, &secret, &d.id, false, NOW), Err(StorageError::Invalid(m)) if m.contains("--allow-secret"))
        );
        assert!(!e.dest.join(".env").exists());
        // 명시적으로 허용하면 보낸다.
        assert_eq!(
            send_at(&e.book, &secret, &d.id, true, NOW).unwrap().state,
            TransferState::Completed
        );

        // 폴더·없는 파일·없는 대상.
        assert!(send_at(&e.book, &e.src.to_string_lossy(), &d.id, false, NOW).is_err());
        assert!(send_at(&e.book, "Z:\\없는\\파일.txt", &d.id, false, NOW).is_err());
        let ok = file(&e.src, "ok.txt", "x");
        assert_eq!(
            send_at(&e.book, &ok, "nope", false, NOW),
            Err(StorageError::NotFound("nope".into()))
        );
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn destination_validation_and_unique_ids() {
        let e = env("dest");
        let p = e.dest.to_string_lossy().into_owned();
        assert!(add_destination_at(&e.book, "", &p, "t").is_err());
        assert!(add_destination_at(&e.book, "상대", "relative\\path", "t").is_err());
        assert!(add_destination_at(&e.book, "없음", &format!("{p}-missing"), "t").is_err());
        assert!(add_destination_at(&e.book, "위로", &format!("{p}\\..\\x"), "t").is_err());

        let a = add_destination_at(&e.book, "Home Server", &p, "t").unwrap();
        let b = add_destination_at(&e.book, "Home Server", &p, "t").unwrap();
        assert_eq!(a.id, "home-server");
        assert_eq!(b.id, "home-server-2");

        // 이름으로도 찾는다 — 한글 이름이면 id 를 외울 필요가 없다.
        let named = add_destination_at(&e.book, "집 서버", &p, "t").unwrap();
        assert_eq!(named.id, "dest");
        let f = file(&e.src, "by-name.txt", "x");
        assert_eq!(
            send_at(&e.book, &f, "집 서버", false, NOW).unwrap().dest_id,
            "dest"
        );
        // 한 글자짜리 id 는 만들지 않는다.
        assert_eq!(
            add_destination_at(&e.book, "D 드라이브", &p, "t")
                .unwrap()
                .id,
            "dest-2"
        );

        // 등록 삭제는 폴더의 파일을 건드리지 않는다.
        std::fs::write(e.dest.join("keep.txt"), "x").unwrap();
        remove_destination_at(&e.book, "home-server").unwrap();
        assert!(e.dest.join("keep.txt").exists());
        assert!(remove_destination_at(&e.book, "home-server").is_err());
        let _ = std::fs::remove_dir_all(&e.root);
    }

    /// 대상 등록이 지워진 뒤의 대기 전송은 조용히 사라지지 않고 실패로 남는다.
    #[test]
    fn pending_transfer_to_a_removed_destination_fails_visibly() {
        let e = env("removed");
        let d = register(&e);
        let f = file(&e.src, "r.txt", "x");
        std::fs::remove_dir_all(&e.dest).unwrap();
        send_at(&e.book, &f, &d.id, false, NOW).unwrap();
        remove_destination_at(&e.book, &d.id).unwrap();

        let tried = retry_pending_at(&e.book, NOW).unwrap();
        assert_eq!(tried[0].state, TransferState::Failed);
        assert!(tried[0].detail.contains("삭제"));
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn corrupt_book_is_never_overwritten() {
        let e = env("corrupt");
        std::fs::write(&e.book, "{ broken").unwrap();
        let p = e.dest.to_string_lossy().into_owned();
        assert!(matches!(
            add_destination_at(&e.book, "x", &p, "t"),
            Err(StorageError::Corrupt(_))
        ));
        assert_eq!(std::fs::read_to_string(&e.book).unwrap(), "{ broken");
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn byte_comparison_detects_same_size_different_content() {
        let e = env("bytes");
        let a = PathBuf::from(file(&e.src, "a", "abcdef"));
        let b = PathBuf::from(file(&e.src, "b", "abcdeX"));
        let c = PathBuf::from(file(&e.src, "c", "abcdef"));
        assert!(!files_equal(&a, &b).unwrap());
        assert!(files_equal(&a, &c).unwrap());
        // 버퍼(64KB)보다 큰 파일의 끝부분 차이도 잡는다.
        let big1 = "x".repeat(200_000);
        let mut big2 = big1.clone();
        big2.replace_range(199_999..200_000, "y");
        let p1 = PathBuf::from(file(&e.src, "big1", &big1));
        let p2 = PathBuf::from(file(&e.src, "big2", &big2));
        assert!(!files_equal(&p1, &p2).unwrap());
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn log_trimming_never_drops_pending_transfers() {
        let mut book = StorageBook::default();
        let mk = |i: usize, state| Transfer {
            id: format!("t{i}"),
            source: String::new(),
            dest_id: String::new(),
            file_name: String::new(),
            bytes: 0,
            state,
            detail: String::new(),
            saved_as: None,
            renamed_for_conflict: false,
            created_at: String::new(),
            finished_at: None,
            attempts: 0,
        };
        book.transfers.push(mk(0, TransferState::Pending));
        for i in 1..(MAX_TRANSFERS + 20) {
            book.transfers.push(mk(i, TransferState::Completed));
        }
        trim_log(&mut book);
        assert_eq!(book.transfers.len(), MAX_TRANSFERS);
        assert_eq!(
            book.transfers[0].id, "t0",
            "가장 오래됐어도 대기 중이면 남는다"
        );
    }
}
