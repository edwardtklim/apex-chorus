// APEX Velox — tempcheck  (읽기 전용 · 조언만, 실행 없음)
//
// 일정 시간 온도를 모니터링해서 최고/최저/평균 + 85°C 이상 지속시간을 집계하고,
// AI가 "쿨러 교체나 서비스(AS)가 필요한 수준인지" 조언만 한다. (절대 시스템을 바꾸지 않음)

use serde::Deserialize;
use std::io::Write;
use std::time::Duration;
use wmi::{COMLibrary, WMIConnection};

const WARN_C: f32 = 85.0;

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
struct ThermalZone {
    #[serde(rename = "CurrentTemperature")]
    current_temperature: u32,
}

fn read_max_temp() -> Option<f32> {
    let com = COMLibrary::new().ok()?;
    let wmi = WMIConnection::with_namespace_path("ROOT\\WMI", com).ok()?;
    let temps: Vec<ThermalZone> = wmi.query().unwrap_or_default();
    temps
        .iter()
        .map(|t| (t.current_temperature as f32 / 10.0) - 273.15)
        .fold(None, |acc, c| Some(acc.map_or(c, |m: f32| m.max(c))))
}

pub async fn run(seconds: u64) {
    println!(
        "=== APEX Velox — tempcheck ({}초 모니터링 · 읽기 전용) ===\n",
        seconds
    );

    let mut samples: Vec<f32> = Vec::new();
    let mut above = 0u32; // 85°C 이상 샘플 수 (≈ 지속 초)

    for i in 0..seconds {
        match read_max_temp() {
            Some(c) => {
                samples.push(c);
                if c >= WARN_C {
                    above += 1;
                }
                print!(
                    "\r[{}/{}s] {:.1}°C   (85°C 이상 누적: {}s)   ",
                    i + 1,
                    seconds,
                    c,
                    above
                );
                std::io::stdout().flush().ok();
            }
            None => {
                println!("온도 센서 읽기 실패 — 관리자 권한으로 실행해야 합니다.");
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!();

    if samples.is_empty() {
        println!("샘플 없음.");
        return;
    }

    let max = samples.iter().cloned().fold(f32::MIN, f32::max);
    let min = samples.iter().cloned().fold(f32::MAX, f32::min);
    let avg = samples.iter().sum::<f32>() / samples.len() as f32;

    println!(
        "\n📊 결과: 최고 {:.1}°C · 최저 {:.1}°C · 평균 {:.1}°C · 85°C 이상 {}초 / {}초",
        max, min, avg, above, seconds
    );

    // AI 조언 (실행 없음 — 조언만)
    let prompt = format!(
        "너는 노트북 발열 진단 도우미다. {seconds}초 모니터링 결과:\n\
         - 최고 온도: {max:.1}°C\n- 최저 온도: {min:.1}°C\n- 평균 온도: {avg:.1}°C\n\
         - 85°C 이상 지속: {above}초 / {seconds}초\n\n\
         한국어로 간단히 답하라:\n\
         1) 발열 상태 평가 (정상/주의/위험)\n\
         2) 쿨러 교체나 서비스(AS)가 필요한 수준인지\n\
         3) 권장 조치 (서멀구리스, 청소, 환기 등)\n\
         ※ 너는 조언만 한다. 시스템을 직접 바꾸지 않는다."
    );

    println!("\n[AI 발열 진단 — 조언만]");
    match crate::chorus::gated_text(
        "claude",
        velox_core::policy::AgentPurpose::Diagnose,
        velox_core::privacy::ContextScope::Minimal,
        prompt,
    )
    .await
    {
        Some(t) => println!("{}", t.trim()),
        None => println!("(AI 발열 진단 생략 — 위 이유 참고. 측정값은 위에 표시됨.)"),
    }
}
