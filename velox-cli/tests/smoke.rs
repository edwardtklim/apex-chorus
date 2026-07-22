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
