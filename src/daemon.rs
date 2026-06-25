// APEX Velox — daemon  (프로세스 → 시스템)
//
// 상시 떠 있으면서 일정 간격으로 시스템을 점검한다.
// 저비용 로컬 스냅샷을 매 틱 찍고, 임계치를 넘을 때만 3단계 AI 파이프라인을 가동한다(API 절약).
//   - 기본       : 감시 + 제안 + 로그
//   - --auto     : Confirmer AI 승인 시 체크포인트 후 자동 실행 (사람 승인 생략)

use std::time::Duration;

pub async fn run(interval_secs: u64, auto: bool) {
    println!("=== APEX Velox — daemon ===");
    println!(
        "간격: {}s · 모드: {} · 중지: Ctrl+C\n",
        interval_secs,
        if auto {
            "AUTO (승인된 조치 자동 실행)"
        } else {
            "감시+제안만"
        }
    );

    let mut tick: u64 = 0;
    loop {
        tick += 1;
        print!("[tick {}] ", tick);
        crate::diagnose::daemon_tick(auto).await;
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}
