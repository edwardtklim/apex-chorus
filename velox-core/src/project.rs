//! velox-core::project — 읽기 전용 프로젝트 세션 (v0.18).
//!
//! 사용자가 고른 프로젝트 폴더를 **읽기만** 해서 typed Evidence로 만든다. 파일을 고치거나
//! 명령을 실행하지 않는다(쓰기·실행은 이후 버전에서 별도 안전 게이트와 함께 검토).
//!
//! 보안 불변조건(이 모듈의 존재 이유):
//! - **경계 탈출 불가**: 모든 경로는 root 기준으로 canonicalize 후 root 안인지 재확인한다.
//!   `..`·절대경로·symlink/junction으로 밖을 가리키면 거부한다.
//! - **비밀 파일 제외**: `.env`, 키/인증서, 자격증명 파일은 이름 기준으로 스캔에서 제외한다.
//! - **절대 경로 미유출**: Evidence에는 **root 기준 상대 경로만** 넣는다. 사용자 홈 디렉터리나
//!   PC 이름이 AI에게 새지 않는다.
//! - **내용 레닥션**: 코드 조각을 꺼낼 때 키처럼 생긴 문자열은 `[REDACTED]`로 가린다.
//! - **한도**: 파일 수·파일 크기·총 바이트 상한을 둬서 큰 저장소를 통째로 올리지 않는다.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::evidence::{
    EvidenceBundle, EvidenceData, EvidenceError, EvidenceId, EvidenceItem, EvidenceSource,
};
use crate::privacy::ContextScope;

/// 기본 한도 — 저장소 전체를 무분별하게 읽지 않기 위한 상한.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectLimits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    /// Evidence로 내보낼 최대 TODO/FIXME 수.
    pub max_todos: usize,
}

impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            max_files: 2_000,
            max_file_bytes: 512 * 1024,
            max_total_bytes: 16 * 1024 * 1024,
            max_todos: 50,
        }
    }
}

/// 프로젝트 작업 중 발생하는 오류. 전부 **거부**로 이어진다(fail-closed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectError {
    NotADirectory(String),
    /// 경로가 프로젝트 root 밖을 가리킨다(`..`/절대경로/symlink 탈출 포함).
    OutsideRoot(String),
    /// 비밀로 취급되는 파일은 열지 않는다.
    SecretExcluded(String),
    /// 파일이 한도를 넘는다.
    TooLarge(String),
    Io(String),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::NotADirectory(p) => write!(f, "폴더가 아닙니다: {p}"),
            ProjectError::OutsideRoot(p) => write!(f, "프로젝트 밖 경로입니다: {p}"),
            ProjectError::SecretExcluded(p) => write!(f, "비밀 파일이라 제외됩니다: {p}"),
            ProjectError::TooLarge(p) => write!(f, "파일이 한도를 초과합니다: {p}"),
            ProjectError::Io(e) => write!(f, "읽기 실패: {e}"),
        }
    }
}

impl std::error::Error for ProjectError {}

// ---------------- 비밀 파일 / 레닥션 ----------------

/// 파일 이름이 비밀(키·자격증명)로 보이는가 — 스캔·읽기에서 제외한다.
pub fn is_secret_filename(name: &str) -> bool {
    let n = name.to_lowercase();
    if n == ".env" || n.starts_with(".env.") || n.ends_with(".env") {
        return true;
    }
    const EXACT: [&str; 8] = [
        "id_rsa",
        "id_ed25519",
        "credentials",
        "credentials.json",
        ".npmrc",
        ".pypirc",
        ".netrc",
        "secrets.json",
    ];
    if EXACT.contains(&n.as_str()) {
        return true;
    }
    const SUFFIX: [&str; 7] = [".pem", ".key", ".pfx", ".p12", ".keystore", ".jks", ".ppk"];
    if SUFFIX.iter().any(|s| n.ends_with(s)) {
        return true;
    }
    n.contains("secret")
        || n.contains("credential")
        || n.contains("apikey")
        || n.contains("api_key")
}

/// 키처럼 보이는 문자열을 가린다 — 코드 조각을 AI에 보내기 전에 항상 통과시킨다.
pub fn redact_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for token in text.split_inclusive(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
        let trimmed = token.trim_matches(|c: char| {
            c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';'
        });
        if looks_like_secret(trimmed) {
            // 원래 토큰의 구분자는 보존하고 값만 가린다.
            let tail: String = token
                .chars()
                .rev()
                .take_while(|c| c.is_whitespace() || *c == '"' || *c == '\'')
                .collect();
            out.push_str("[REDACTED]");
            out.extend(tail.chars().rev());
        } else {
            out.push_str(token);
        }
    }
    out
}

/// 토큰 하나가 알려진 비밀 형태인가.
fn looks_like_secret(t: &str) -> bool {
    if t.len() < 20 {
        return false;
    }
    const PREFIXES: [&str; 7] = ["sk-ant-", "sk-proj-", "sk-", "AIza", "xai-", "AKIA", "ghp_"];
    if PREFIXES.iter().any(|p| t.starts_with(p)) {
        return true;
    }
    t.contains("BEGIN") && t.contains("PRIVATE")
}

// ---------------- 무시 규칙 ----------------

/// 항상 건너뛰는 디렉터리(빌드 산출물·의존성·VCS).
const ALWAYS_SKIP_DIRS: [&str; 12] = [
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".idea",
    ".vscode",
    "vendor",
];

/// `.gitignore` **부분 구현** — 흔한 패턴만 지원한다(주석·부정(`!`)·디렉터리(`/`)·`*` 글롭).
/// 완전한 gitignore 의미론이 아니며, 모르는 패턴은 **무시하지 않고 포함**시키기보다
/// 보수적으로 처리하기 위해 항상 기본 스킵 목록과 비밀 파일 규칙을 함께 적용한다.
#[derive(Clone, Debug, Default)]
pub struct IgnoreRules {
    patterns: Vec<(String, bool)>, // (pattern, is_negation)
}

impl IgnoreRules {
    pub fn from_gitignore(text: &str) -> Self {
        let mut patterns = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (neg, pat) = match line.strip_prefix('!') {
                Some(rest) => (true, rest.trim()),
                None => (false, line),
            };
            let pat = pat.trim_start_matches('/').trim_end_matches('/');
            if !pat.is_empty() {
                patterns.push((pat.to_string(), neg));
            }
        }
        Self { patterns }
    }

    /// 상대 경로가 무시 대상인가.
    pub fn is_ignored(&self, rel_path: &str) -> bool {
        let mut ignored = false;
        for (pat, neg) in &self.patterns {
            if path_matches(rel_path, pat) {
                ignored = !neg;
            }
        }
        ignored
    }
}

/// 아주 단순한 글롭: `*`는 경로 구분자를 제외한 임의 문자열.
fn glob_matches(name: &str, pat: &str) -> bool {
    if !pat.contains('*') {
        return name == pat;
    }
    let parts: Vec<&str> = pat.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match name[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 {
                    return false;
                }
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    if let Some(last) = parts.last()
        && !last.is_empty()
        && !name.ends_with(last)
    {
        return false;
    }
    true
}

/// 패턴이 경로 전체 또는 어떤 구성요소와 맞는가.
fn path_matches(rel_path: &str, pat: &str) -> bool {
    if glob_matches(rel_path, pat) {
        return true;
    }
    rel_path.split('/').any(|seg| glob_matches(seg, pat))
}

// ---------------- 프로젝트 세션 ----------------

/// 사용자가 선택한 프로젝트. `root`는 canonicalize된 절대 경로이며 **외부에 노출하지 않는다**.
#[derive(Clone, Debug)]
pub struct ProjectSession {
    root: PathBuf,
    pub created_at: u64,
    pub limits: ProjectLimits,
    ignore: IgnoreRules,
}

/// 프로젝트 폴더를 연다. 폴더가 아니면 거부.
pub fn open(root: &Path, limits: ProjectLimits) -> Result<ProjectSession, ProjectError> {
    let canonical = root
        .canonicalize()
        .map_err(|e| ProjectError::Io(e.to_string()))?;
    if !canonical.is_dir() {
        return Err(ProjectError::NotADirectory(root.display().to_string()));
    }
    let ignore = std::fs::read_to_string(canonical.join(".gitignore"))
        .map(|t| IgnoreRules::from_gitignore(&t))
        .unwrap_or_default();
    Ok(ProjectSession {
        root: canonical,
        created_at: crate::ledger::now_unix(),
        limits,
        ignore,
    })
}

/// 파일 하나의 요약(상대 경로만 보관).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FileEntry {
    pub rel_path: String,
    pub language: String,
    pub bytes: u64,
}

/// TODO/FIXME 한 건 (내용은 레닥션 후 잘라서 보관).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct TodoItem {
    pub rel_path: String,
    pub line: u32,
    pub text: String,
}

/// 스캔 결과 — **상대 경로와 집계만** 담는다.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProjectScan {
    pub files: Vec<FileEntry>,
    pub language_counts: BTreeMap<String, usize>,
    pub total_bytes: u64,
    pub manifests: Vec<String>,
    /// 매니페스트에서 **탐지된**(실행하지 않는) 빌드·테스트 명령.
    pub detected_commands: Vec<String>,
    pub todos: Vec<TodoItem>,
    /// 비밀로 판단해 건너뛴 파일 수.
    pub skipped_secret_files: usize,
    /// 한도에 걸려 일부만 스캔했는가.
    pub truncated: bool,
}

fn language_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_lowercase();
    Some(match ext.as_str() {
        "rs" => "Rust",
        "c" | "h" => "C",
        "cc" | "cpp" | "cxx" | "hpp" => "C++",
        "py" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "go" => "Go",
        "java" => "Java",
        "cs" => "C#",
        "toml" | "json" | "yaml" | "yml" => "Config",
        "md" => "Docs",
        _ => return None,
    })
}

const MANIFESTS: [&str; 8] = [
    "Cargo.toml",
    "package.json",
    "pyproject.toml",
    "requirements.txt",
    "CMakeLists.txt",
    "Makefile",
    "go.mod",
    "pom.xml",
];

/// 매니페스트 이름 → 관례적인 빌드/테스트 명령(탐지만, 실행하지 않음).
fn commands_for(manifest: &str) -> Vec<&'static str> {
    match manifest {
        "Cargo.toml" => vec!["cargo build", "cargo test", "cargo clippy"],
        "package.json" => vec!["npm install", "npm test"],
        "pyproject.toml" | "requirements.txt" => vec!["pytest"],
        "CMakeLists.txt" => vec!["cmake --build"],
        "Makefile" => vec!["make"],
        "go.mod" => vec!["go build", "go test ./..."],
        "pom.xml" => vec!["mvn test"],
        _ => vec![],
    }
}

impl ProjectSession {
    /// 프로젝트 이름(root의 마지막 구성요소만) — 전체 경로는 노출하지 않는다.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into())
    }

    /// root 경로(내부/표시용). Evidence에는 절대 넣지 않는다.
    pub fn root_display(&self) -> String {
        self.root.display().to_string()
    }

    /// 상대 경로를 안전하게 해석한다. **경계 밖이면 거부**(`..`·절대경로·symlink 탈출 포함).
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, ProjectError> {
        let candidate = Path::new(rel);
        if candidate.is_absolute() {
            return Err(ProjectError::OutsideRoot(rel.to_string()));
        }
        for comp in candidate.components() {
            match comp {
                Component::Normal(_) | Component::CurDir => {}
                // `..`, 루트, 드라이브 접두사는 전부 거부.
                _ => return Err(ProjectError::OutsideRoot(rel.to_string())),
            }
        }
        let joined = self.root.join(candidate);
        // canonicalize가 symlink/junction을 실제 위치로 풀어준다 → 탈출 탐지.
        let real = joined
            .canonicalize()
            .map_err(|e| ProjectError::Io(e.to_string()))?;
        if !real.starts_with(&self.root) {
            return Err(ProjectError::OutsideRoot(rel.to_string()));
        }
        if let Some(name) = real.file_name().map(|s| s.to_string_lossy().to_string())
            && is_secret_filename(&name)
        {
            return Err(ProjectError::SecretExcluded(rel.to_string()));
        }
        Ok(real)
    }

    /// 사용자가 고른 코드 조각을 읽는다 — 한도 검사 + **레닥션**을 거친다.
    pub fn read_snippet(
        &self,
        rel: &str,
        start_line: u32,
        line_count: u32,
    ) -> Result<Vec<String>, ProjectError> {
        let path = self.resolve(rel)?;
        let meta = std::fs::metadata(&path).map_err(|e| ProjectError::Io(e.to_string()))?;
        if meta.len() > self.limits.max_file_bytes {
            return Err(ProjectError::TooLarge(rel.to_string()));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| ProjectError::Io(e.to_string()))?;
        let start = start_line.saturating_sub(1) as usize;
        Ok(text
            .lines()
            .skip(start)
            .take(line_count.max(1) as usize)
            .map(redact_secrets)
            .collect())
    }

    /// 프로젝트를 읽기 전용으로 스캔한다. 한도를 넘으면 잘라내고 `truncated`로 표시한다.
    pub fn scan(&self) -> ProjectScan {
        let mut scan = ProjectScan::default();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let Ok(rel) = path.strip_prefix(&self.root) else {
                    continue;
                };
                let rel_path = rel.to_string_lossy().replace('\\', "/");

                let Ok(ft) = entry.file_type() else { continue };
                // symlink는 따라가지 않는다(경계 탈출 방지).
                if ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    if ALWAYS_SKIP_DIRS.contains(&name.as_str())
                        || self.ignore.is_ignored(&rel_path)
                    {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                if is_secret_filename(&name) {
                    scan.skipped_secret_files += 1;
                    continue;
                }
                if self.ignore.is_ignored(&rel_path) {
                    continue;
                }
                if scan.files.len() >= self.limits.max_files
                    || scan.total_bytes >= self.limits.max_total_bytes
                {
                    scan.truncated = true;
                    continue;
                }
                let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if MANIFESTS.contains(&name.as_str()) && !scan.manifests.contains(&name) {
                    scan.manifests.push(name.clone());
                    for c in commands_for(&name) {
                        let c = c.to_string();
                        if !scan.detected_commands.contains(&c) {
                            scan.detected_commands.push(c);
                        }
                    }
                }
                let Some(lang) = language_of(&name) else {
                    continue;
                };
                scan.total_bytes += bytes;
                *scan.language_counts.entry(lang.to_string()).or_insert(0) += 1;
                if bytes <= self.limits.max_file_bytes && scan.todos.len() < self.limits.max_todos {
                    collect_todos(&path, &rel_path, self.limits.max_todos, &mut scan.todos);
                }
                scan.files.push(FileEntry {
                    rel_path,
                    language: lang.to_string(),
                    bytes,
                });
            }
        }
        scan.files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        scan.manifests.sort();
        scan
    }

    /// 스캔 결과를 **typed Evidence**로 만든다. 상대 경로와 집계만 들어간다.
    pub fn evidence(
        &self,
        scan: &ProjectScan,
        approved_scope: ContextScope,
    ) -> Result<EvidenceBundle, EvidenceError> {
        let mut items = Vec::new();
        let mut push = |id: &str, data: EvidenceData| {
            items.push(EvidenceItem {
                id: EvidenceId(id.to_string()),
                source: EvidenceSource::Project,
                // 프로젝트 집계·상대경로에는 시스템 신원 정보가 없다.
                sensitivity: ContextScope::Minimal,
                data,
            });
        };
        push(
            "project.name",
            EvidenceData::Fact {
                name: "프로젝트".into(),
                value: self.name(),
            },
        );
        push(
            "project.files",
            EvidenceData::Metric {
                name: "파일 수".into(),
                value: scan.files.len() as f64,
                unit: "".into(),
            },
        );
        push(
            "project.bytes",
            EvidenceData::Metric {
                name: "코드 크기".into(),
                value: scan.total_bytes as f64,
                unit: "B".into(),
            },
        );
        for (lang, count) in &scan.language_counts {
            push(
                &format!(
                    "project.lang.{}",
                    lang.to_lowercase().replace(['+', '#'], "p")
                ),
                EvidenceData::Metric {
                    name: format!("{lang} 파일"),
                    value: *count as f64,
                    unit: "".into(),
                },
            );
        }
        for (i, m) in scan.manifests.iter().enumerate() {
            push(
                &format!("project.manifest.{i}"),
                EvidenceData::Fact {
                    name: "매니페스트".into(),
                    value: m.clone(),
                },
            );
        }
        for (i, c) in scan.detected_commands.iter().enumerate() {
            push(
                &format!("project.command.{i}"),
                EvidenceData::Fact {
                    name: "탐지된 명령(실행 안 함)".into(),
                    value: c.clone(),
                },
            );
        }
        for (i, t) in scan.todos.iter().enumerate() {
            push(
                &format!("project.todo.{i}"),
                EvidenceData::CodeFinding {
                    path: t.rel_path.clone(),
                    line: Some(t.line),
                    message: t.text.clone(),
                },
            );
        }
        if scan.skipped_secret_files > 0 {
            push(
                "project.secrets_skipped",
                EvidenceData::Finding {
                    code: "project.secrets_skipped".into(),
                    message: format!(
                        "비밀로 판단된 파일 {}개를 제외했습니다",
                        scan.skipped_secret_files
                    ),
                },
            );
        }
        if scan.truncated {
            push(
                "project.truncated",
                EvidenceData::Finding {
                    code: "project.truncated".into(),
                    message: "한도에 걸려 일부만 스캔했습니다".into(),
                },
            );
        }
        EvidenceBundle::new(approved_scope, items)
    }
}

/// 기본 분석 목표 — 사용자가 따로 안 적으면 이걸 쓴다.
pub const DEFAULT_OBJECTIVE: &str =
    "이 프로젝트의 구조·위험·기술부채를 Evidence만 근거로 분석하고, 각 발견에 근거 ID를 붙여라";

/// 프로젝트 분석용 Council 요청을 만든다.
///
/// Council은 **읽기 전용**이라 파일을 고치지 않는다. 여기서는 Evidence만 준비하고,
/// 실행(취소·진행 표시 포함)은 호출자(CLI/서버)가 [`crate::council::run`]으로 한다.
pub fn analysis_request(
    session: &ProjectSession,
    scan: &ProjectScan,
    objective: Option<&str>,
    approved_scope: ContextScope,
) -> Result<crate::council::CouncilRequest, EvidenceError> {
    let evidence = session.evidence(scan, approved_scope)?;
    Ok(crate::council::CouncilRequest {
        objective: objective
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_OBJECTIVE)
            .to_string(),
        evidence,
        approved_scope,
    })
}

/// 파일에서 TODO/FIXME를 찾아 레닥션 후 담는다.
fn collect_todos(path: &Path, rel_path: &str, max: usize, out: &mut Vec<TodoItem>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for (i, line) in text.lines().enumerate() {
        if out.len() >= max {
            return;
        }
        if line.contains("TODO") || line.contains("FIXME") {
            let cleaned = redact_secrets(line.trim());
            let text: String = cleaned.chars().take(120).collect();
            out.push(TodoItem {
                rel_path: rel_path.to_string(),
                line: (i + 1) as u32,
                text,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 격리된 임시 프로젝트를 만든다.
    fn temp_project(tag: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "apex-proj-{tag}-{}-{}",
            std::process::id(),
            crate::ledger::now_unix()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        fs::write(
            base.join("src/main.rs"),
            "fn main(){}\n// TODO: 리팩터 필요\n",
        )
        .unwrap();
        fs::write(
            base.join(".env"),
            "OPENAI_API_KEY=sk-proj-abcdefghijklmnop\n",
        )
        .unwrap();
        fs::write(base.join("key.pem"), "-----BEGIN PRIVATE KEY-----\n").unwrap();
        fs::write(base.join(".gitignore"), "ignored_dir\n*.log\n").unwrap();
        fs::create_dir_all(base.join("ignored_dir")).unwrap();
        fs::write(base.join("ignored_dir/x.rs"), "fn ignored(){}").unwrap();
        fs::write(base.join("noisy.log"), "log").unwrap();
        base
    }

    #[test]
    fn secret_filenames_are_detected() {
        for n in [
            ".env",
            ".env.local",
            "id_rsa",
            "server.pem",
            "my.key",
            "credentials.json",
            "app_secret.txt",
            "APIKEY.txt",
        ] {
            assert!(is_secret_filename(n), "{n} 는 비밀로 취급돼야 함");
        }
        for n in ["main.rs", "README.md", "Cargo.toml"] {
            assert!(!is_secret_filename(n), "{n} 는 일반 파일");
        }
    }

    #[test]
    fn redaction_hides_key_like_tokens() {
        let out = redact_secrets("let k = \"sk-proj-abcdefghijklmnopqrs\"; // ok");
        assert!(!out.contains("sk-proj-abcdefghijklmnopqrs"));
        assert!(out.contains("[REDACTED]"));
        // 짧은 평범한 토큰은 건드리지 않는다.
        let plain = redact_secrets("let x = 42; // fine");
        assert_eq!(plain, "let x = 42; // fine");
    }

    #[test]
    fn path_escape_attempts_are_rejected() {
        let root = temp_project("escape");
        let p = open(&root, ProjectLimits::default()).unwrap();
        for bad in ["../outside.txt", "..", "src/../../etc", "/etc/passwd"] {
            assert!(
                matches!(p.resolve(bad), Err(ProjectError::OutsideRoot(_))),
                "{bad} 는 거부돼야 함"
            );
        }
        // 정상 경로는 허용.
        assert!(p.resolve("src/main.rs").is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn secret_file_cannot_be_opened_even_if_path_valid() {
        let root = temp_project("secretopen");
        let p = open(&root, ProjectLimits::default()).unwrap();
        assert!(matches!(
            p.resolve(".env"),
            Err(ProjectError::SecretExcluded(_))
        ));
        assert!(matches!(
            p.read_snippet("key.pem", 1, 5),
            Err(ProjectError::SecretExcluded(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_excludes_secrets_and_respects_gitignore() {
        let root = temp_project("scan");
        let p = open(&root, ProjectLimits::default()).unwrap();
        let scan = p.scan();
        let paths: Vec<&str> = scan.files.iter().map(|f| f.rel_path.as_str()).collect();
        assert!(paths.contains(&"src/main.rs"));
        assert!(!paths.iter().any(|p| p.contains(".env")));
        assert!(!paths.iter().any(|p| p.ends_with(".pem")));
        assert!(!paths.iter().any(|p| p.contains("ignored_dir")));
        assert!(!paths.iter().any(|p| p.ends_with(".log")));
        assert!(scan.skipped_secret_files >= 2);
        assert!(scan.manifests.contains(&"Cargo.toml".to_string()));
        assert!(scan.detected_commands.iter().any(|c| c == "cargo test"));
        assert!(scan.todos.iter().any(|t| t.rel_path == "src/main.rs"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn evidence_contains_no_absolute_paths() {
        let root = temp_project("eviabs");
        let p = open(&root, ProjectLimits::default()).unwrap();
        let scan = p.scan();
        let bundle = p.evidence(&scan, ContextScope::Minimal).unwrap();
        let prompt = bundle.to_prompt();
        // 사용자 홈/임시 경로가 새면 안 된다.
        assert!(!prompt.contains(&p.root_display()));
        assert!(!prompt.to_lowercase().contains("users"));
        assert!(!prompt.contains(":\\"));
        // 상대 경로와 집계는 들어 있다.
        assert!(prompt.contains("project.files"));
        assert!(prompt.contains("Cargo.toml"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn limits_truncate_large_projects() {
        let root = temp_project("limits");
        let limits = ProjectLimits {
            max_files: 1,
            ..Default::default()
        };
        let p = open(&root, limits).unwrap();
        let scan = p.scan();
        assert!(scan.files.len() <= 1);
        assert!(scan.truncated);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn snippet_is_redacted() {
        let root = temp_project("snippet");
        fs::write(
            root.join("src/leaky.rs"),
            "let k = \"sk-ant-api03-abcdefghijklmnopqrstuvwx\";\n",
        )
        .unwrap();
        let p = open(&root, ProjectLimits::default()).unwrap();
        let lines = p.read_snippet("src/leaky.rs", 1, 1).unwrap();
        assert!(lines[0].contains("[REDACTED]"));
        assert!(!lines[0].contains("sk-ant-api03-abcdefghijklmnopqrstuvwx"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gitignore_subset_matches_common_patterns() {
        let rules = IgnoreRules::from_gitignore("# comment\n/target\n*.log\nbuild/\n!keep.log\n");
        assert!(rules.is_ignored("target"));
        assert!(rules.is_ignored("a/b.log"));
        assert!(rules.is_ignored("build"));
        assert!(!rules.is_ignored("keep.log")); // 부정 패턴
        assert!(!rules.is_ignored("src/main.rs"));
    }

    #[test]
    fn analysis_request_carries_only_project_evidence() {
        let root = temp_project("analysis");
        let p = open(&root, ProjectLimits::default()).unwrap();
        let scan = p.scan();
        let req = analysis_request(&p, &scan, None, ContextScope::Minimal).unwrap();
        assert_eq!(req.objective, DEFAULT_OBJECTIVE);
        assert_eq!(req.approved_scope, ContextScope::Minimal);
        // 모든 Evidence가 프로젝트 출처이고 승인 범위 이내.
        assert!(
            req.evidence
                .items
                .iter()
                .all(|i| i.source == EvidenceSource::Project
                    && i.sensitivity <= ContextScope::Minimal)
        );
        // 사용자 목표를 주면 그대로 쓴다.
        let custom =
            analysis_request(&p, &scan, Some("버그만 찾아줘"), ContextScope::Minimal).unwrap();
        assert_eq!(custom.objective, "버그만 찾아줘");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn opening_a_file_instead_of_directory_is_rejected() {
        let root = temp_project("notdir");
        let file = root.join("Cargo.toml");
        assert!(matches!(
            open(&file, ProjectLimits::default()),
            Err(ProjectError::NotADirectory(_))
        ));
        let _ = fs::remove_dir_all(&root);
    }
}
