// APEX Velox — watch  (Velox Watch)
//
// daemon/dashboard가 쓰는 경량 시스템 감시기. sysinfo로 CPU/RAM/Disk/Network를 읽는다.
// (관리자 권한 불필요)

use sysinfo::{Disks, Networks, System};

const CPU_WARN: f32 = 90.0;
const RAM_WARN: f64 = 90.0;
const DISK_FREE_WARN_PCT: f64 = 10.0; // 여유 10% 미만이면 이벤트
const NET_WARN_MBPS: f64 = 50.0;

/// 한 틱의 구조화된 지표 (대시보드/데몬 공용).
pub struct Metrics {
    pub cpu: f32,
    pub ram_pct: f64,
    pub disks: Vec<(String, f64)>, // (mount, free %)
    pub rx_mbps: f64,
    pub tx_mbps: f64,
}

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

    /// 갱신 후 구조화된 지표 반환.
    pub fn read(&mut self, interval_secs: u64) -> Metrics {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.nets.refresh();

        let cpu = self.sys.global_cpu_info().cpu_usage();
        let total = self.sys.total_memory();
        let ram_pct = if total > 0 {
            self.sys.used_memory() as f64 / total as f64 * 100.0
        } else {
            0.0
        };

        let mut disks = Vec::new();
        for d in &Disks::new_with_refreshed_list() {
            let t = d.total_space();
            if t == 0 {
                continue;
            }
            disks.push((
                d.mount_point().to_string_lossy().to_string(),
                d.available_space() as f64 / t as f64 * 100.0,
            ));
        }

        let secs = interval_secs.max(1) as f64;
        let (mut rx, mut tx) = (0u64, 0u64);
        for (_, data) in &self.nets {
            rx += data.received();
            tx += data.transmitted();
        }

        Metrics {
            cpu,
            ram_pct,
            disks,
            rx_mbps: rx as f64 / 1e6 / secs,
            tx_mbps: tx as f64 / 1e6 / secs,
        }
    }

    /// 데몬용: heartbeat 문자열 + 임계 초과 이벤트.
    pub fn tick(&mut self, interval_secs: u64) -> (String, Vec<String>) {
        let m = self.read(interval_secs);
        let mut events = Vec::new();
        if m.cpu >= CPU_WARN {
            events.push(format!("CPU {:.0}% (임계 {:.0}%)", m.cpu, CPU_WARN));
        }
        if m.ram_pct >= RAM_WARN {
            events.push(format!("RAM {:.0}% (임계 {:.0}%)", m.ram_pct, RAM_WARN));
        }
        let mut disk_summ = String::new();
        for (mp, free) in &m.disks {
            disk_summ.push_str(&format!("{}{:.0}%여유 ", mp, free));
            if *free < DISK_FREE_WARN_PCT {
                events.push(format!("Disk {} 여유 {:.0}% (임계 {:.0}%)", mp, free, DISK_FREE_WARN_PCT));
            }
        }
        if m.rx_mbps > NET_WARN_MBPS || m.tx_mbps > NET_WARN_MBPS {
            events.push(format!(
                "NET 높음 ↓{:.0}↑{:.0} MB/s (임계 {:.0})",
                m.rx_mbps, m.tx_mbps, NET_WARN_MBPS
            ));
        }
        let hb = format!(
            "CPU {:.0}% · RAM {:.0}% · {}· NET ↓{:.1}↑{:.1} MB/s",
            m.cpu, m.ram_pct, disk_summ, m.rx_mbps, m.tx_mbps
        );
        (hb, events)
    }
}
