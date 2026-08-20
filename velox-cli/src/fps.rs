use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ferrisetw::EventRecord;
use ferrisetw::provider::Provider;
use ferrisetw::schema_locator::SchemaLocator;
use ferrisetw::trace::UserTrace;

use sysinfo::System;

// Microsoft-Windows-DXGI provider.
const DXGI_PROVIDER_GUID: &str = "CA11C036-0102-4A2D-A6AD-F03CFED5D3C9";
// DXGIPresent_Start — fired once per Present() call (i.e. once per submitted frame).
const DXGI_PRESENT_START: u16 = 42;

pub fn run(seconds: u64) {
    println!("=== APEX Velox — fps ===");
    println!(
        "Counting DXGI present events per process for {}s...",
        seconds
    );
    println!("(Requires Administrator. Launch a game/3D app to see its FPS.)\n");

    let counts: Arc<Mutex<HashMap<u32, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    let counts_cb = counts.clone();

    let provider = Provider::by_guid(DXGI_PROVIDER_GUID)
        .add_callback(move |record: &EventRecord, _schema: &SchemaLocator| {
            if record.event_id() == DXGI_PRESENT_START {
                let pid = record.process_id();
                *counts_cb.lock().unwrap().entry(pid).or_insert(0) += 1;
            }
        })
        .build();

    let trace = match UserTrace::new()
        .named(String::from("velox-fps"))
        .enable(provider)
        .start_and_process()
    {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to start ETW trace: {e:?}");
            eprintln!(
                "{}",
                velox_core::guidance::Problem::AdminRequired {
                    what: "FPS 측정(ETW 트레이스)".into()
                }
                .guidance()
                .render_plain()
            );
            return;
        }
    };

    std::thread::sleep(Duration::from_secs(seconds));
    trace.stop().ok();

    let mut sys = System::new();
    sys.refresh_processes();

    let map = counts.lock().unwrap();
    let mut rows: Vec<(u32, u64)> = map.iter().map(|(&k, &v)| (k, v)).collect();
    rows.sort_by_key(|row| std::cmp::Reverse(row.1));

    if rows.is_empty() {
        println!("No present events captured.");
        println!("Either no GPU app was presenting, or the trace lacked permission.");
        return;
    }

    println!("{:<8} {:<26} {:>10}", "PID", "Process", "FPS");
    println!("{}", "-".repeat(46));
    for (pid, count) in rows.iter().take(15) {
        let name = sys
            .process(sysinfo::Pid::from_u32(*pid))
            .map(|p| p.name().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let fps = *count as f64 / seconds as f64;
        println!("{:<8} {:<26} {:>10.1}", pid, name, fps);
    }
    println!("\nNote: dwm.exe is the Windows desktop compositor (presents at refresh rate).");
    println!("Your game is the high-FPS entry that is not dwm.exe.");
}
