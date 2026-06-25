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
        "간격: {}s · 쿨다운: {}s · 모드: {} · 중지: Ctrl+C\n",
        interval_secs,
        COOLDOWN_SECS,
        if auto {
            "AUTO (승인된 조치 자동 실행)"
        } else {
            "감시+제안만"
        }
    );
    log_event(&format!(
        "daemon start interval={} auto={}",
        interval_secs, auto
    ));

    let cooldown = Duration::from_secs(COOLDOWN_SECS);
    let mut last_fire: Option<Instant> = None;
    let mut tick: u64 = 0;

    loop {
        tick += 1;
        print!("[tick {}] ", tick);

        // 쿨다운 만료 여부 → AI 가동 허용?
        let allow_ai = last_fire.map_or(true, |t| t.elapsed() >= cooldown);
        let fired = crate::diagnose::daemon_tick(auto, allow_ai).await;
        if fired {
            last_fire = Some(Instant::now());
            log_event("anomaly: AI pipeline fired");
        }

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
