mod chorus;
mod thermals;
mod gpu;
mod bench;
mod fps;
mod diagnose;
mod checkpoint;
mod daemon;
mod dashboard;
mod drivers;
mod doctor;
mod tempcheck;
mod fpscheck;
mod timeline;

use dotenv::dotenv;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "velox")]
#[command(about = "APEX Velox — system monitor and AI orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Info,
    Thermals {
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value_t = 1000)]
        interval: u64,
    },
    Gpu {
        #[command(subcommand)]
        action: GpuCommands,
    },
    Bench {
        #[command(subcommand)]
        action: BenchCommands,
    },
    Fps {
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    /// AI가 시스템 상태를 진단하고, 승인 시 안전·가역 조치를 실행 (3단계 AI)
    Diagnose {
        #[arg(long)]
        fix: bool,
        /// 발열/관리자 없이 임계 초과(95°C)를 주입해 전체 루프를 실증
        #[arg(long = "simulate-hot")]
        simulate_hot: bool,
    },
    /// APEX Doctor — "왜 느려?" 한 명령으로 전체 스캔 + AI 종합 진단 (읽기 전용)
    Doctor,
    /// 온도를 N초 모니터링 → 최고/최저/평균·지속시간 → AI 발열 조언 (읽기 전용, 실행 X)
    Tempcheck {
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// FPS를 N초 측정 → 평균/최저/1% low → AI 게임 성능 조언 (읽기 전용, 실행 X)
    Fpscheck {
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// 성능 추세 기록·비교 — 개인 최고(100%) 대비 "언제부터 느려졌나" 추적
    Timeline {
        #[command(subcommand)]
        action: TimelineCommands,
    },
    /// 드라이버/장치 상태 읽기 — 문제 장치 탐지. --analyze면 AI가 known-issue/업데이트 조언
    Drivers {
        #[arg(long)]
        analyze: bool,
    },
    /// 정상 상태 저장/복원 (블루스크린·AI 오판 후 되돌리기)
    Checkpoint {
        #[command(subcommand)]
        action: CheckpointCommands,
    },
    /// 실시간 터미널 대시보드 — CPU/RAM/GPU/온도/디스크/네트워크를 한 화면에 (1초 갱신)
    Dashboard,
    /// 상시 감시 데몬 — 일정 간격으로 점검, 임계치 시 AI 파이프라인 가동
    Daemon {
        #[arg(long, default_value_t = 30)]
        interval: u64,
        #[arg(long)]
        auto: bool,
    },
    Chorus {
        #[command(subcommand)]
        action: ChorusCommands,
    },
}

#[derive(Subcommand)]
enum GpuCommands {
    Status,
}

#[derive(Subcommand)]
enum BenchCommands {
    /// CPU single + multi-thread benchmark (prime sieve + matrix multiply)
    Cpu,
    /// GPU monitor — samples nvidia-smi while you run a real load
    Gpu {
        #[arg(long, default_value_t = 10)]
        seconds: u64,
    },
    /// Everyday workload — file compression + image resize/encode
    Everyday,
    /// CPU 지속 성능(쓰로틀링) — 싱글·멀티를 N초간 돌려 유지율 측정
    Stability {
        #[arg(long, default_value_t = 15)]
        seconds: u64,
    },
    /// Run all benchmarks in sequence
    All {
        #[arg(long, default_value_t = 10)]
        gpu_seconds: u64,
    },
}

#[derive(Subcommand)]
enum TimelineCommands {
    /// 지금 성능을 측정해 기록
    Record,
    /// 기록된 추세 + 개인 최고 대비 % 보기
    Show,
}

#[derive(Subcommand)]
enum CheckpointCommands {
    /// 현재 정상 상태 저장
    Save,
    /// 저장된 체크포인트 목록
    List,
    /// 마지막 정상 상태로 복원
    Restore,
}

#[derive(Subcommand)]
enum ChorusCommands {
    Ask {
        prompt: String,
        #[arg(long = "use")]
        use_model: Option<String>,
        #[arg(long = "no-context")]
        no_context: bool,
    },
    /// 연결된 AI 목록 + 키 상태
    Models,
    /// API 키 직접 입력·저장 (.env): chorus set <provider> <key>
    Set {
        provider: String,
        key: String,
    },
    /// 커스텀 AI provider 추가 (OpenAI 호환): chorus add <name> <base_url> <model> [key]
    Add {
        name: String,
        base_url: String,
        model: String,
        #[arg(default_value = "none")]
        key: String,
    },
    /// 연결된 모든 AI에 핑을 보내 응답 검증
    Test,
    /// AI 모델 벤치 — 다중 심판이 0~1000 채점 → 리더보드 (자기 답 제외). --hard로 변별력↑
    Bench {
        #[arg(long)]
        hard: bool,
    },
    /// AI 합의 — 같은 질문을 여러 모델에 → 공통점/차이 정리: chorus consensus "질문"
    Consensus {
        question: String,
    },
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Commands::Info => run_info(),
        Commands::Thermals { watch, interval } => {
            if watch {
                thermals::run_watch(interval);
            } else {
                thermals::run_once();
            }
        }
        Commands::Gpu { action } => match action {
            GpuCommands::Status => gpu::run_status(),
        },
        Commands::Bench { action } => match action {
            BenchCommands::Cpu => bench::run_cpu(),
            BenchCommands::Gpu { seconds } => bench::run_gpu_monitor(seconds),
            BenchCommands::Everyday => bench::run_everyday(),
            BenchCommands::Stability { seconds } => bench::run_stability(seconds),
            BenchCommands::All { gpu_seconds } => {
                bench::run_cpu();
                println!();
                bench::run_everyday();
                println!();
                bench::run_gpu_monitor(gpu_seconds);
            }
        },
        Commands::Fps { seconds } => fps::run(seconds),
        Commands::Diagnose { fix, simulate_hot } => diagnose::run(fix, simulate_hot).await,
        Commands::Doctor => doctor::run().await,
        Commands::Tempcheck { seconds } => tempcheck::run(seconds).await,
        Commands::Fpscheck { seconds } => fpscheck::run(seconds).await,
        Commands::Timeline { action } => match action {
            TimelineCommands::Record => timeline::record(),
            TimelineCommands::Show => timeline::show(),
        },
        Commands::Drivers { analyze } => {
            if analyze {
                drivers::run_analyze().await
            } else {
                drivers::run()
            }
        }
        Commands::Checkpoint { action } => match action {
            CheckpointCommands::Save => checkpoint::save(),
            CheckpointCommands::List => checkpoint::list(),
            CheckpointCommands::Restore => checkpoint::restore_latest(),
        },
        Commands::Dashboard => dashboard::run().await,
        Commands::Daemon { interval, auto } => daemon::run(interval, auto).await,
        Commands::Chorus { action } => match action {
            ChorusCommands::Ask { prompt, use_model, no_context } => {
                let (model, auto) = match use_model {
                    Some(m) => (m, false),
                    None => (velox_core::ai::route_semantic(&prompt).await, true),
                };
                if auto {
                    println!("→ Auto-routed to: {} (semantic)\n", model);
                } else {
                    println!("→ Using: {}\n", model);
                }
                chorus::ask(&prompt, &model, no_context).await;
            }
            ChorusCommands::Models => {
                chorus::show_models();
            }
            ChorusCommands::Set { provider, key } => chorus::set_key(&provider, &key),
            ChorusCommands::Add { name, base_url, model, key } => {
                chorus::add_provider(&name, &base_url, &model, &key)
            }
            ChorusCommands::Test => chorus::test_all().await,
            ChorusCommands::Bench { hard } => chorus::bench(hard).await,
            ChorusCommands::Consensus { question } => chorus::consensus(&question).await,
        },
    }
}

fn run_info() {
    use serde::Deserialize;
    use wmi::{COMLibrary, WMIConnection};

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_Processor")]
    struct Processor {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "NumberOfCores")]
        number_of_cores: u32,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_VideoController")]
    struct GPU {
        #[serde(rename = "Name")]
        name: String,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_Battery")]
    struct Battery {
        #[serde(rename = "EstimatedChargeRemaining")]
        charge: u32,
    }

    #[derive(Deserialize, Debug)]
    #[serde(rename = "MSAcpi_ThermalZoneTemperature")]
    struct ThermalZone {
        #[serde(rename = "CurrentTemperature")]
        current_temperature: u32,
        #[serde(rename = "InstanceName")]
        instance_name: String,
    }

    println!("=== APEX Velox — velox info ===\n");

    let com = COMLibrary::new().expect("COM init failed");
    let wmi = WMIConnection::new(com.clone()).expect("WMI connection failed");

    let cpus: Vec<Processor> = wmi.query().expect("CPU query failed");
    for cpu in &cpus {
        println!("CPU:    {}", cpu.name);
        println!("Cores:  {}", cpu.number_of_cores);
    }

    println!();

    let gpus: Vec<GPU> = wmi.query().expect("GPU query failed");
    for gpu in &gpus {
        println!("GPU:    {}", gpu.name);
    }

    println!();

    let batteries: Vec<Battery> = wmi.query().expect("Battery query failed");
    if batteries.is_empty() {
        println!("Battery: No battery detected");
    } else {
        for bat in &batteries {
            println!("Battery: {}%", bat.charge);
        }
    }

    println!();
    println!("--- Temperatures ---");
    let wmi2 = WMIConnection::with_namespace_path("ROOT\\WMI", com)
        .expect("WMI ROOT\\WMI failed");

    let temps: Vec<ThermalZone> = wmi2.query().unwrap_or_default();
    if temps.is_empty() {
        println!("Temperature: Run as Administrator for sensor data");
    } else {
        for t in &temps {
            let celsius = (t.current_temperature as f32 / 10.0) - 273.15;
            println!("{}: {:.1}°C", t.instance_name, celsius);
        }
    }

    println!("\n=== velox info complete ===");
}