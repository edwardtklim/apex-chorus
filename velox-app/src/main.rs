//! velox-app — Pulse 프로토타입(v0).
//!
//! 로컬 엔진(velox-server)을 띄우고, 통합 UI를 **크롬 없는 데스크탑 창**
//! (Edge/Chrome `--app` = WebView2, Tauri가 쓰는 것과 같은 렌더 엔진)으로 연다.
//! 창을 닫으면 엔진도 함께 종료된다.
//!
//! 다음 단계: 완전 번들 네이티브 앱(Tauri 단일 exe · 자체 아이콘 · 인스톨러).

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7878";
const URL: &str = "http://127.0.0.1:7878";

/// 같은 폴더의 실행 파일 경로.
fn sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

/// 엔진이 뜰 때까지 (최대 secs초) 기다린다.
fn wait_for_server(secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if TcpStream::connect(ADDR).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    false
}

/// Edge/Chrome 후보 경로.
fn browser_candidates() -> Vec<String> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let (pf, pfx, local) = (
        env("ProgramFiles"),
        env("ProgramFiles(x86)"),
        env("LOCALAPPDATA"),
    );
    vec![
        format!("{pfx}\\Microsoft\\Edge\\Application\\msedge.exe"),
        format!("{pf}\\Microsoft\\Edge\\Application\\msedge.exe"),
        format!("{pf}\\Google\\Chrome\\Application\\chrome.exe"),
        format!("{local}\\Google\\Chrome\\Application\\chrome.exe"),
    ]
}

/// UI를 크롬 없는 앱 창으로 연다. (전용 user-data-dir로 새 인스턴스 → 프로세스가 창 수명과 일치)
fn open_app_window() -> Option<Child> {
    let profile = std::env::temp_dir().join("velox-app-profile");
    for path in browser_candidates() {
        if !Path::new(&path).exists() {
            continue;
        }
        if let Ok(child) = Command::new(&path)
            .arg(format!("--app={URL}"))
            .arg("--window-size=1140,800")
            .arg(format!("--user-data-dir={}", profile.display()))
            .spawn()
        {
            return Some(child);
        }
    }
    None
}

fn main() {
    // 1) 엔진 시작
    let mut server = match Command::new(sibling("velox-server.exe")).spawn() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("velox-server.exe를 찾을 수 없습니다. velox-app와 같은 폴더에 있어야 합니다.");
            return;
        }
    };
    if !wait_for_server(6) {
        eprintln!("엔진이 시작되지 않았습니다.");
        let _ = server.kill();
        return;
    }

    // 2) 앱 창 열기 — 브라우저 런처는 창을 넘기고 즉시 반환하므로 wait 하지 않는다.
    if open_app_window().is_none() {
        let _ = Command::new("cmd").args(["/c", "start", "", URL]).spawn();
    }
    println!("✔ APEX Velox 앱 실행 중 → {URL}");
    println!("  앱 창을 닫아도 엔진은 살아있습니다. 완전 종료: 이 터미널에서 Ctrl+C.");

    // 3) 엔진이 사는 동안 유지 — Ctrl+C 하면 이 콘솔의 자식(엔진)도 함께 종료된다.
    let _ = server.wait();
}
