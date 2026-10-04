// CLI smoke test — 빌드된 velox 바이너리가 기본 명령에서 정상 종료하는지.
// 엔진/네트워크가 아니라 "CLI가 패닉 없이 실행되고 출력을 낸다"만 검증한다.
// (CARGO_BIN_EXE_velox 는 Cargo가 통합 테스트에 자동 주입하는 바이너리 경로)

use std::process::Command;

fn velox() -> Command {
    Command::new(env!("CARGO_BIN_EXE_velox"))
}

#[test]
fn info_runs_and_prints_header() {
    let out = velox().arg("info").output().expect("velox 실행 실패");
    assert!(out.status.success(), "`velox info` 는 0으로 종료해야 함");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("velox info"),
        "헤더가 출력돼야 함, 실제: {stdout}"
    );
}

#[test]
fn checkpoint_list_runs() {
    let out = velox()
        .args(["checkpoint", "list"])
        .output()
        .expect("velox 실행 실패");
    assert!(
        out.status.success(),
        "`velox checkpoint list` 는 0으로 종료해야 함"
    );
}

#[test]
fn chorus_models_runs() {
    let out = velox()
        .args(["chorus", "models"])
        .output()
        .expect("velox 실행 실패");
    assert!(
        out.status.success(),
        "`velox chorus models` 는 0으로 종료해야 함"
    );
}

/// 회귀 방지 — `--version` 같은 조기 종료가 알파 지표의 crash 로 집계되면 안 된다.
///
/// clap 은 --version/--help/인자 오류에서 프로세스를 바로 끝낸다. 그 경로가
/// `metrics::record_clean_exit()` 를 건너뛰면 세션 표식이 남아 다음 실행이
/// crash 로 세어진다. 2026-09-17 스모크 테스트에서 19회 실행 중 7회가 가짜 crash 였다.
#[test]
fn early_exit_paths_are_not_counted_as_crashes() {
    let dir = std::env::temp_dir().join(format!("velox-metrics-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 데이터 디렉터리 생성 실패");

    let run = |args: &[&str]| {
        velox()
            .env("VELOX_DATA_DIR", &dir)
            .args(args)
            .output()
            .expect("velox 실행 실패")
    };

    assert!(run(&["--version"]).status.success());
    assert!(run(&["--help"]).status.success());
    // 잘못된 인자는 0 이 아닌 코드로 끝나지만, 그래도 crash 가 아니다.
    assert!(!run(&["such-command-does-not-exist"]).status.success());

    let out = run(&["metrics", "summary", "--json"]);
    let json = String::from_utf8_lossy(&out.stdout);
    let crashes = json
        .lines()
        .find(|l| l.contains("\"crashes\""))
        .unwrap_or_else(|| panic!("crashes 필드가 없다: {json}"));
    assert!(
        crashes.contains(": 0"),
        "조기 종료가 crash 로 집계됐다: {crashes}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
