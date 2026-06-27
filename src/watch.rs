// APEX Velox — watch  (Velox Watch MVP)
//
// daemon이 매 틱 호출하는 경량 시스템 감시기. sysinfo로 CPU/RAM/Disk/Network를 읽고
// 임계치를 넘으면 "이벤트"를 만든다. (관리자 권한 불필요)
//
// 기존 daemon 구조는 그대로 두고, Watcher 하나를 붙이는 최소 확장.

use sysinfo::{Disks, Networks, System};

const CPU_WARN: f32 = 90.0;
const RAM_WARN: f64 = 90.0;
const DISK_FREE_WARN_PCT: f64 = 10.0; // 여유 10% 미만이면 이벤트
const NET_WARN_MBPS: f64 = 50.0;

pub struct Watcher {
    sys: System,
    nets: Networks,
}

impl Watcher {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu();
        sys.refresh_memory();
        Watcher {
            sys,
            nets: Networks::new_with_refreshed_list(),
        }
    }

    /// 한 틱 감시. 반환: (heartbeat 문자열, 임계 초과 이벤트 목록)
    pub fn tick(&mut self, interval_secs: u64) -> (String, Vec<String>) {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.nets.refresh();

        let cpu = self.sys.global_cpu_info().cpu_usage();
        let total = self.sys.total_memory();
        let ram = if total > 0 {
            self.sys.used_memory() as f64 / total as f64 * 100.0
        } else {
            0.0
        };

        let mut events: Vec<String> = Vec::new();
        if cpu >= CPU_WARN {
            events.push(format!("CPU {:.0}% (임계 {:.0}%)", cpu, CPU_WARN));
        }
        if ram >= RAM_WARN {
            events.push(format!("RAM {:.0}% (임계 {:.0}%)", ram, RAM_WARN));
        }

        // 디스크 여유
        let disks = Disks::new_with_refreshed_list();
        let mut disk_summ = String::new();
        for d in &disks {
            let t = d.total_space();
            if t == 0 {
                continue;
            }
            let free_pct = d.available_space() as f64 / t as f64 * 100.0;
            let mp = d.mount_point().to_string_lossy();
            disk_summ.push_str(&format!("{}{:.0}%여유 ", mp, free_pct));
            if free_pct < DISK_FREE_WARN_PCT {
                events.push(format!("Disk {} 여유 {:.0}% (임계 {:.0}%)", mp, free_pct, DISK_FREE_WARN_PCT));
            }
        }

        // 네트워크 속도 (지난 refresh 이후 바이트 / 간격)
        let secs = interval_secs.max(1) as f64;
        let (mut rx, mut tx) = (0u64, 0u64);
        for (_, data) in &self.nets {
            rx += data.received();
            tx += data.transmitted();
        }
        let rx_mbps = rx as f64 / 1e6 / secs;
        let tx_mbps = tx as f64 / 1e6 / secs;
        if rx_mbps > NET_WARN_MBPS || tx_mbps > NET_WARN_MBPS {
            events.push(format!(
                "NET 높음 ↓{:.0}↑{:.0} MB/s (임계 {:.0})",
                rx_mbps, tx_mbps, NET_WARN_MBPS
            ));
        }

        let hb = format!(
            "CPU {:.0}% · RAM {:.0}% · {}· NET ↓{:.1}↑{:.1} MB/s",
            cpu, ram, disk_summ, rx_mbps, tx_mbps
        );
        (hb, events)
    }
}
