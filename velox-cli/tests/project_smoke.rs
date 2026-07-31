use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestProject(PathBuf);

impl TestProject {
    fn new() -> Self {
        let suffix = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "apex-project-cli-test-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("src")).expect("create project");
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n",
        )
        .expect("write manifest");
        std::fs::write(root.join("src/main.rs"), "fn main() { /* TODO: test */ }\n")
            .expect("write source");
        std::fs::write(root.join(".env"), "OPENAI_API_KEY=forbidden-secret\n")
            .expect("write excluded secret");
        Self(root)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_velox"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("velox command should start")
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn project_scan_json_is_read_only_and_does_not_leak_secrets_or_root() {
    let project = TestProject::new();
    let manifest_before = std::fs::read(project.0.join("Cargo.toml")).unwrap();
    let source_before = std::fs::read(project.0.join("src/main.rs")).unwrap();
    let root_text = project.0.to_string_lossy().into_owned();

    let output = project.run(&["project", "scan", ".", "--json"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8(output.stdout).expect("UTF-8 JSON");
    let json: serde_json::Value = serde_json::from_str(&text).expect("structured scan JSON");
    assert!(
        json["files"]
            .as_array()
            .is_some_and(|files| !files.is_empty())
    );
    assert!(text.contains("src/main.rs"));
    assert!(!text.contains(&root_text));
    assert!(!text.contains(".env"));
    assert!(!text.contains("forbidden-secret"));

    assert_eq!(
        std::fs::read(project.0.join("Cargo.toml")).unwrap(),
        manifest_before
    );
    assert_eq!(
        std::fs::read(project.0.join("src/main.rs")).unwrap(),
        source_before
    );
    assert_eq!(
        std::fs::read_dir(&project.0).unwrap().count(),
        3,
        "scan must not create project-local state"
    );
}
