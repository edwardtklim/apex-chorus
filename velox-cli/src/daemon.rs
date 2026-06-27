// APEX Velox — daemon  (프로세스 → 시스템)
//
// 상시 떠 있으면서 일정 간격으로 시스템을 점검한다.
// 저비용 로컬 스냅샷을 매 틱 찍고, 임계치를 넘을 때만 3단계 AI 파이프라인을 가동(API 절약).
//   - 기본    : 감시 + 제안 + 로그
//   - --auto  : Confirmer AI 승인 시 체크포인트 후 자동 실행
//
// 안정화(v0.4):
//   - 쿨다운    : 한 번 가동하면 COOLDOWN_SECS 동안 AI 재가동 억제 (비용·플래핑 방지)
//   - 우아한 종료: Ctrl+C 수신 시 루프를 깔끔히 종료
//   - 이벤트 로그: 시작/이상/종료를 파일에 기록

use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const COOLDOWN_SECS: u64 = 300; // 5분: 한 번 조치 후 재가동 억제
const CONSECUTIVE_HOT_REQUIRED: u32 = 3; // 지속성: 연속 N회 과열이어야 발동 (글리치 무시)
const LOG_FILE: &str = "velox_daemon.log";

fn log_event(msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(LOG_FILE) {
        let _ = writeln!(f, "{} {}", ts, msg);
    }
}

pub async fn run(interval_secs: u64, auto: bool) {
    println!("=== APEX Velox — daemon ===");
    println!(
        "간격: {}s · 지속성: {}회 · 쿨다운: {}s · 모드: {} · 중지: Ctrl+C\n",
        interval_secs,
        CONSECUTIVE_HOT_REQUIRED,
        COOLDOWN_SECS,
        if auto {
            "AUTO (승인된 조치 자동 실행)"
        } else {
            "감시+제안만"
        }
    );
    println!("감시: 온도 · CPU · RAM · Disk · Network (임계 초과 시 EVENT)\n");
    log_event(&format!(
        "daemon start interval={} auto={}",
        interval_secs, auto
    ));

    let cooldown = Duration::from_secs(COOLDOWN_SECS);
    let mut last_fire: Option<Instant> = None;
    let mut hot_streak: u32 = 0;
    let mut tick: u64 = 0;
    let mut watcher = velox_core::watch::Watcher::new(); // Velox Watch: CPU/RAM/Disk/Net

    loop {
        tick += 1;

        // 1) 매 틱 가벼운 상태 읽기 (저비용)
        let (heartbeat, hot) = crate::diagnose::quick_status();
        if hot {
            hot_streak += 1;
        } else {
            hot_streak = 0;
        }

        print!("[tick {}] {}", tick, heartbeat);
        if hot {
            print!("  ⚠ 이상 {}/{}", hot_streak, CONSECUTIVE_HOT_REQUIRED);
        }
        println!();

        // Velox Watch: CPU/RAM/Disk/Network 감시 + 임계 이벤트 (기존 온도 흐름과 독립)
        let (whb, wevents) = watcher.tick(interval_secs);
        println!("         {}", whb);
        for e in &wevents {
            println!("         ⚠ EVENT: {}", e);
            log_event(&format!("watch event: {}", e));
        }

        // 2) 지속성 + 쿨다운 둘 다 통과해야 무거운 AI 반응 가동
        let cooldown_ok = last_fire.map_or(true, |t| t.elapsed() >= cooldown);
        if hot && hot_streak >= CONSECUTIVE_HOT_REQUIRED && cooldown_ok {
            println!("  → 지속 확인됨(글리치 아님) → 3단계 AI 파이프라인 가동");
            log_event("anomaly persisted -> react");
            crate::diagnose::react(auto).await;
            last_fire = Some(Instant::now());
            hot_streak = 0;
        } else if hot && !cooldown_ok {
            println!("  → 쿨다운 중 — 대기");
        }
        // hot이지만 아직 지속 횟수 미달 → 계속 카운트만 (글리치 한 번엔 반응 안 함)

        // 인터벌 대기 중 Ctrl+C 들어오면 즉시 우아하게 종료
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {}
            _ = tokio::signal::ctrl_c() => {
                println!("\n종료 신호 수신 — daemon 정지.");
                log_event("daemon stop (ctrl-c)");
                break;
            }
        }
    }
}
