// APEX Velox — fpscheck  (읽기 전용 · 조언만, 실행 없음)
//
// ETW(DXGI present)로 N초간 프레임을 측정해 게임 프로세스의 평균/최저/최고/1% low fps를
// 계산하고, AI가 "체감 부드러움·스터터 여부·그래픽 설정"을 조언만 한다. (시스템 변경 없음)

use ferrisetw::EventRecord;
use ferrisetw::provider::Provider;
use ferrisetw::schema_locator::SchemaLocator;
use ferrisetw::trace::UserTrace;
use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::System;

const DXGI_PROVIDER_GUID: &str = "CA11C036-0102-4A2D-A6AD-F03CFED5D3C9";
const DXGI_PRESENT_START: u16 = 42;

pub async fn run(seconds: u64) {
    println!(
        "=== APEX Velox — fpscheck ({}초 · 읽기 전용 · 조언만) ===",
        seconds
    );
    println!("(관리자 권한 필요. 게임/3D 앱을 켜고 실행하세요.)\n");

    // (pid, 측정 시작 이후 경과 초) 기록 → 프레임 간격으로 순간 fps 산출
    let events: Arc<Mutex<Vec<(u32, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let ev_cb = events.clone();
    let start = Instant::now();

    let provider = Provider::by_guid(DXGI_PROVIDER_GUID)
        .add_callback(move |record: &EventRecord, _s: &SchemaLocator| {
            if record.event_id() == DXGI_PRESENT_START {
                ev_cb
                    .lock()
                    .unwrap()
                    .push((record.process_id(), start.elapsed().as_secs_f64()));
            }
        })
        .build();

    let trace = match UserTrace::new()
        .named(String::from("velox-fpscheck"))
        .enable(provider)
        .start_and_process()
    {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ETW 시작 실패: {e:?}");
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

    for i in 0..seconds {
        print!("\r모니터링 {}/{}s ...", i + 1, seconds);
        std::io::stdout().flush().ok();
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!();
    trace.stop().ok();

    let evs = events.lock().unwrap().clone();
    if evs.is_empty() {
        println!("present 이벤트 없음 — 게임이 실행 중인지 / 관리자 권한인지 확인.");
        return;
    }

    // pid별 카운트
    let mut counts: HashMap<u32, u64> = HashMap::new();
    for (pid, _) in evs.iter() {
        *counts.entry(*pid).or_insert(0) += 1;
    }

    let mut sys = System::new();
    sys.refresh_processes();
    let name_of = |pid: u32| {
        sys.process(sysinfo::Pid::from_u32(pid))
            .map(|p| p.name().to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    };

    // 게임 = dwm.exe 아닌 최다 present 프로세스
    let mut ranked: Vec<(u32, u64)> = counts.iter().map(|(&k, &v)| (k, v)).collect();
    ranked.sort_by_key(|row| std::cmp::Reverse(row.1));
    let game = ranked
        .iter()
        .find(|(pid, _)| {
            let n = name_of(*pid).to_lowercase();
            n != "dwm.exe" && n != "<unknown>"
        })
        .or_else(|| ranked.first())
        .cloned();
    let (gpid, gcount) = match game {
        Some(g) => g,
        None => {
            println!("게임 식별 실패.");
            return;
        }
    };
    let gname = name_of(gpid);

    // 게임 pid의 타임스탬프 → 프레임 간격 → 순간 fps 분포
    let mut ts: Vec<f64> = evs
        .iter()
        .filter(|(p, _)| *p == gpid)
        .map(|(_, t)| *t)
        .collect();
    ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let avg_fps = gcount as f64 / seconds as f64;
    let mut inst: Vec<f64> = ts
        .windows(2)
        .filter_map(|w| {
            let dt = w[1] - w[0];
            (dt > 0.0).then(|| 1.0 / dt)
        })
        .collect();
    inst.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let min_fps = inst.first().cloned().unwrap_or(0.0);
    let max_fps = inst.last().cloned().unwrap_or(0.0);
    // 1% low = 가장 낮은 1% 순간 fps의 평균 (스터터 지표)
    let n1 = (((inst.len() as f64) * 0.01).ceil() as usize)
        .max(1)
        .min(inst.len());
    let low1 = if inst.is_empty() {
        0.0
    } else {
        inst[..n1].iter().sum::<f64>() / n1 as f64
    };

    println!("\n🎮 게임: {} (PID {})", gname, gpid);
    println!(
        "📊 평균 {:.0} fps · 1% low {:.0} fps · 최저 {:.0} · 최고 {:.0}",
        avg_fps, low1, min_fps, max_fps
    );

    let prompt = format!(
        "너는 게임 성능 진단 도우미다. 게임 '{gname}' {seconds}초 측정 결과:\n\
         - 평균 {avg_fps:.0} fps\n- 1% low {low1:.0} fps\n- 최저 {min_fps:.0} / 최고 {max_fps:.0} fps\n\n\
         한국어로 간단히 답하라:\n\
         1) 체감 부드러움 평가\n\
         2) 1% low 기준 끊김(스터터)이 있는지\n\
         3) 그래픽 설정/해상도 조정 권장\n\
         ※ 조언만 한다. 시스템을 직접 바꾸지 않는다."
    );
    println!("\n[AI 게임 성능 진단 — 조언만]");
    match crate::chorus::gated_text(
        "claude",
        velox_core::policy::AgentPurpose::Diagnose,
        velox_core::privacy::ContextScope::System,
        prompt,
    )
    .await
    {
        Some(t) => println!("{}", t.trim()),
        None => println!("(AI 게임 성능 진단 생략 — 위 이유 참고. 측정값은 위에 표시됨.)"),
    }
}
