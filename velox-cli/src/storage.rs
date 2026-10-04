//! `velox storage` — 저장 대상 등록과 검증된 단방향 전송 (프로그램 3).
//!
//! 판단·검증·복사는 전부 [`velox_core::storage`] 가 한다. 여기서는 표시만 한다.

use velox_core::storage::{self, Transfer, TransferState};

fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.2} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

fn print_transfer(t: &Transfer) {
    let mark = match t.state {
        TransferState::Completed | TransferState::AlreadyThere => "✓",
        TransferState::Pending => "…",
        TransferState::Failed => "✗",
    };
    println!(
        "{mark} [{}] {} ({}) → {}",
        t.state.label(),
        t.file_name,
        size(t.bytes),
        t.dest_id
    );
    println!("    {}", t.detail);
    if let Some(p) = &t.saved_as {
        println!("    저장 위치: {p}");
    }
}

/// `velox storage add <이름> <폴더>`
pub fn add(name: &str, path: &str) {
    match storage::add_destination(name, path) {
        Ok(d) => {
            println!("✓ 저장 대상 등록: {} [{}]", d.name, d.id);
            println!("    폴더: {}", d.path);
            println!("    보내기: velox storage send <파일> --to {}", d.id);
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox storage list`
pub fn list(json: bool) {
    let book = match storage::load() {
        Ok(b) => b,
        Err(e) => {
            println!("✗ {e}");
            return;
        }
    };
    if json {
        match serde_json::to_string_pretty(&book.destinations) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        return;
    }
    println!("=== 저장 대상 {}개 ===\n", book.destinations.len());
    if book.destinations.is_empty() {
        println!("등록된 대상이 없습니다.");
        println!("  다음 행동: velox storage add \"집 서버\" \\\\서버\\공유\\APEX");
        println!("  (다른 PC 의 폴더는 먼저 Windows 공유로 열려 있어야 합니다)");
        return;
    }
    for d in &book.destinations {
        // 지금 닿는지를 함께 보여준다 — 등록돼 있다고 연결돼 있는 것은 아니다.
        println!(
            "  [{}] {}  [{}]",
            if storage::is_reachable(d) {
                "연결"
            } else {
                "끊김"
            },
            d.name,
            d.id
        );
        println!("      {}", d.path);
    }
    let pending = book
        .transfers
        .iter()
        .filter(|t| t.state == TransferState::Pending)
        .count();
    if pending > 0 {
        println!("\n  대기 중인 전송 {pending}건 → velox storage retry");
    }
}

/// `velox storage remove <id>` — 등록만 지운다. 폴더의 파일은 건드리지 않는다.
pub fn remove(id: &str) {
    match storage::remove_destination(id) {
        Ok(d) => {
            println!("✓ 등록을 지웠습니다: {} [{}]", d.name, d.id);
            println!("    {} 안의 파일은 그대로 있습니다.", d.path);
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox storage send <파일> --to <id>`
pub fn send(file: &str, to: &str, allow_secret: bool) {
    let started = std::time::Instant::now();
    match storage::send(file, to, allow_secret) {
        Ok(t) => {
            print_transfer(&t);
            if t.state == TransferState::Pending {
                println!("    원본은 그대로 두세요 — 옮기거나 지우면 다시 시도할 수 없습니다.");
            }
            velox_core::metrics::record_operation(
                "storage_send",
                match t.state {
                    TransferState::Failed => velox_core::metrics::OperationOutcome::Failed,
                    _ => velox_core::metrics::OperationOutcome::Completed,
                },
                started.elapsed().as_millis() as u64,
            );
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox storage retry` — 대기 중인 전송을 다시 시도한다.
pub fn retry() {
    match storage::retry_pending() {
        Ok(tried) if tried.is_empty() => println!("대기 중인 전송이 없습니다."),
        Ok(tried) => {
            println!("=== 다시 시도 {}건 ===\n", tried.len());
            for t in &tried {
                print_transfer(t);
            }
        }
        Err(e) => println!("✗ {e}"),
    }
}

/// `velox storage log [--limit N]` — 최근 전송 기록.
pub fn log(limit: usize, json: bool) {
    let book = match storage::load() {
        Ok(b) => b,
        Err(e) => {
            println!("✗ {e}");
            return;
        }
    };
    let recent: Vec<&Transfer> = book.transfers.iter().rev().take(limit).collect();
    if json {
        match serde_json::to_string_pretty(&recent) {
            Ok(t) => println!("{t}"),
            Err(e) => eprintln!("✗ 직렬화 실패: {e}"),
        }
        return;
    }
    println!("=== 전송 기록 (최근 {}건) ===\n", recent.len());
    if recent.is_empty() {
        println!("기록이 없습니다.");
    }
    for t in recent {
        println!("{}", t.created_at);
        print_transfer(t);
    }
}
