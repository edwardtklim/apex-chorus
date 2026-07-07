//! velox-app — Pulse. 네이티브 데스크탑 앱.
//!
//! 자체 창(WebView2 임베드, 브라우저 아님)에 통합 UI(velox-server의 `/`)를 띄운다.
//! 엔진(velox-server)을 자식 프로세스로 시작하고, 창을 닫으면 엔진도 종료한다.
//! velox-server.exe / velox.exe 가 같은 폴더에 있어야 한다.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const ADDR: &str = "127.0.0.1:7878";
const URL: &str = "http://127.0.0.1:7878";

fn sibling(name: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

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

#[cfg(windows)]
fn spawn_server(path: &Path) -> std::io::Result<Child> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW — 엔진 콘솔 창이 뜨지 않게.
    Command::new(path).creation_flags(0x0800_0000).spawn()
}
#[cfg(not(windows))]
fn spawn_server(path: &Path) -> std::io::Result<Child> {
    Command::new(path).spawn()
}

fn main() {
    // 1) 엔진 시작
    let mut server = match spawn_server(&sibling("velox-server.exe")) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("velox-server.exe를 찾을 수 없습니다 (velox-app와 같은 폴더 필요).");
            return;
        }
    };
    wait_for_server(6);

    // 2) 네이티브 창 + 임베드 웹뷰
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("APEX Velox")
        .with_inner_size(LogicalSize::new(1160.0, 820.0))
        .build(&event_loop)
        .expect("창 생성 실패");
    let _webview = WebViewBuilder::new(&window)
        .with_url(URL)
        .build()
        .expect("웹뷰 생성 실패");

    // 3) 창을 닫으면 엔진도 종료
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            let _ = server.kill();
            *control_flow = ControlFlow::Exit;
        }
    });
}
