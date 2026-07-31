//! v0.17 Usage CLI integration tests.
//!
//! Each command runs in an isolated temporary working directory so the tests
//! never read, modify, or delete a developer's real ledger or pricing files.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "apex-usage-test-{}-{label}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temporary test directory creation");
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_velox"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("velox command should start")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn usage_summary_is_honest_when_nothing_is_configured() {
    let dir = TestDir::new("summary");
    let output = dir.run(&["usage", "summary"]);
    let text = stdout(&output);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("Estimated API cost"));
    assert!(text.contains("unknown"));
    assert!(text.contains("Not subscription billing or provider balance"));
}

#[test]
fn usage_export_has_no_sensitive_content_fields() {
    let dir = TestDir::new("export");
    let output = dir.run(&["usage", "export", "--format", "json"]);
    let text = stdout(&output);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(text.trim(), "[]");
    for forbidden in [
        "prompt",
        "response",
        "evidence",
        "api_key",
        "authorization",
        "secret",
    ] {
        assert!(
            !text.to_ascii_lowercase().contains(forbidden),
            "usage export unexpectedly contains {forbidden}"
        );
    }
}

#[test]
fn pricing_show_does_not_invent_default_prices() {
    let dir = TestDir::new("pricing");
    let output = dir.run(&["usage", "pricing", "show"]);
    let text = stdout(&output);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("단가 미설정"));
    assert!(text.contains("unknown"));
}

#[test]
fn read_only_usage_commands_do_not_create_state_files() {
    let dir = TestDir::new("read-only");
    for args in [
        &["usage", "summary"][..],
        &["usage", "sessions"][..],
        &["usage", "pricing", "show"][..],
    ] {
        let output = dir.run(args);
        assert!(output.status.success(), "command failed: {args:?}");
    }

    assert!(!dir.0.join("velox_ledger.json").exists());
    assert!(!dir.0.join("velox_pricing.json").exists());
}
