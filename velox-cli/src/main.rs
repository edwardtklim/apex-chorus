mod bench;
mod checkpoint;
mod chorus;
mod compare;
mod daemon;
mod dashboard;
mod diagnose;
mod doctor;
mod drivers;
mod fps;
mod fpscheck;
mod gpu;
mod metrics;
mod plans;
mod project;
mod report;
mod snapshot;
mod system;
mod tempcheck;
mod thermals;
mod timeline;
mod usage;

use clap::{Parser, Subcommand};
use dotenv::dotenv;

#[derive(Parser)]
#[command(name = "velox")]
#[command(version)]
#[command(about = "APEX Velox — Windows 시스템 진단 + 멀티 AI 오케스트레이터 CLI")]
#[command(
    long_about = "APEX Velox — 시스템을 읽고(성능·온도·드라이버·스냅샷) AI가 안전하게 진단/조치하는 Windows CLI.\n\n\
키 없이 동작: info · snapshot · compare · bench · gpu · thermals · drivers · timeline\n\
API 키 필요: doctor · diagnose · chorus  (설정: velox chorus set <provider> <key>)\n\
일부 센서/ETW 기능은 관리자 권한이 필요합니다."
)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// CPU·GPU·배터리·온도 한눈에 (기본 시스템 정보)
    Info,
    /// 온도 실시간 폴링 — --watch면 계속 갱신 (관리자 권장)
    Thermals {
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value_t = 1000)]
        interval: u64,
    },
    /// GPU 사용률·VRAM·온도 (nvidia-smi, 없으면 WMI 폴백)
    Gpu {
        #[command(subcommand)]
        action: GpuCommands,
    },
    /// CPU 벤치(싱글 10000 기준)·stability·thermal(쿨러)·gpu 모니터
    Bench {
        #[command(subcommand)]
        action: BenchCommands,
    },
    /// 프로세스별 프레임 감지 (ETW/DXGI, 관리자 필요)
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
    /// 현재 시스템 스냅샷 — 시스템/GPU/드라이버/전원/온도. --json 기계용 · --out 파일저장
    Snapshot {
        #[arg(long)]
        json: bool,
        /// 스냅샷을 JSON 파일로 저장 (나중에 `velox compare`로 비교)
        #[arg(long)]
        out: Option<String>,
    },
    /// 두 스냅샷(JSON 파일) 비교 — 드라이버/하드웨어 등 구조 변화만 (순간값 무시)
    Compare { old: String, new: String },
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
    /// 멀티 AI — 의미기반 라우팅·키 설정·모델 벤치·합의 (API 키 필요)
    Chorus {
        #[command(subcommand)]
        action: ChorusCommands,
    },
    /// APEX가 호출한 AI 사용량·추정 비용 (로컬 기록 · 구독 청구서 아님)
    Usage {
        #[command(subcommand)]
        action: UsageCommands,
    },
    /// 선택한 프로젝트를 안전하게 스캔하거나 읽기 전용 Council 분석
    Project {
        #[command(subcommand)]
        action: ProjectCommands,
    },
    /// 수리 전/후를 측정하고 비교 리포트를 만든다
    Report {
        #[command(subcommand)]
        action: ReportCommands,
    },
    /// Closed Alpha 지표 — 로컬에만 쌓이며 어디로도 전송되지 않습니다
    Metrics {
        #[command(subcommand)]
        action: MetricsCommands,
    },
    /// 시스템 관리 조회 — 서비스·시작 프로그램·디스크·네트워크 (읽기 전용, 아무것도 바꾸지 않음)
    System {
        #[command(subcommand)]
        action: SystemCommands,
    },
    /// AI 구독·API 계정 기록 — 플랜·금액·갱신일·주 용도·예산 (직접 입력, 결제 실행 안 함)
    Plans {
        #[command(subcommand)]
        action: PlansCommands,
    },
}

#[derive(Subcommand)]
enum PlansCommands {
    /// 전체 기록 + 월 고정 지출(구독만) + API 예산 대비 추정 사용액
    List {
        #[arg(long)]
        json: bool,
    },
    /// 기록 추가(같은 id 면 교체). 구독과 API 는 따로 등록합니다
    Add {
        /// 제공자 이름 (claude, chatgpt, gemini ...)
        #[arg(long)]
        provider: String,
        /// subscription(정액 구독) 또는 api(사용량 과금)
        #[arg(long)]
        kind: String,
        /// 플랜 이름 (Plus, Max ...)
        #[arg(long, default_value = "")]
        plan: String,
        /// 주기당 금액. 모르면 생략 — 0 으로 채우지 않습니다
        #[arg(long)]
        amount: Option<f64>,
        /// 통화 코드 (USD, KRW ...)
        #[arg(long, default_value = "USD")]
        currency: String,
        /// monthly / yearly / none
        #[arg(long, default_value = "none")]
        cycle: String,
        /// 다음 갱신일 YYYY-MM-DD
        #[arg(long)]
        renews: Option<String>,
        /// 이 AI 를 주로 무엇에 쓰는지
        #[arg(long, default_value = "")]
        purpose: String,
        /// (API 전용) 월 예산 — 넘으면 알려줍니다. 결제를 막지는 않습니다
        #[arg(long)]
        budget: Option<f64>,
        #[arg(long, default_value = "")]
        note: String,
        /// 같은 제공자·종류를 여러 개 등록할 때 직접 지정
        #[arg(long)]
        id: Option<String>,
    },
    /// 기록 삭제
    Remove { id: String },
    /// 값이 아직 맞다고 확인 (갱신일을 함께 고칠 수 있음)
    Confirm {
        id: String,
        #[arg(long)]
        renews: Option<String>,
    },
    /// N일 안에 갱신되는 구독 (기본 14일)
    Upcoming {
        #[arg(long, default_value_t = 14)]
        days: i64,
    },
}

#[derive(Subcommand)]
enum SystemCommands {
    /// 요약 — 수집 시각·확인할 항목·디스크·네트워크·서비스 개수
    Status {
        #[arg(long)]
        json: bool,
    },
    /// 서비스 목록 (--stopped 면 자동 시작인데 멈춘 것만)
    Services {
        #[arg(long)]
        stopped: bool,
        #[arg(long)]
        json: bool,
    },
    /// 시작 프로그램 목록
    Startup {
        #[arg(long)]
        json: bool,
    },
    /// 지금 상태를 기준점으로 저장 (나중에 changes 로 비교)
    Save,
    /// 기준점 이후 달라진 것 — 시작 프로그램·서비스·디스크·네트워크
    Changes {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum MetricsCommands {
    /// 지금까지 쌓인 지표 요약
    Summary {
        #[arg(long)]
        json: bool,
    },
    /// 지표를 파일로 내보내기 (개발자에게 보낼 때)
    Export {
        #[arg(long)]
        out: Option<String>,
    },
    /// 지표 전부 삭제
    Clear,
    /// 방금 본 경고가 틀렸다고 표시 (사람이 판단하는 지표)
    FalseWarning,
}

#[derive(Subcommand)]
enum ReportCommands {
    /// 현재 상태를 측정해 파일로 저장 (수리 전/후에 각각 한 번씩)
    Capture {
        /// 라벨 (before / after 등)
        #[arg(long, default_value = "before")]
        label: String,
        /// 저장할 파일 경로
        #[arg(long)]
        out: String,
        /// CPU 점수도 함께 측정 (몇 초 더 걸림)
        #[arg(long)]
        bench: bool,
    },
    /// 두 측정을 비교해 수리 리포트 생성
    Repair {
        #[arg(long)]
        before: String,
        #[arg(long)]
        after: String,
        /// HTML 리포트를 저장할 경로
        #[arg(long)]
        out: Option<String>,
        /// PC 이름 (리포트에 표시)
        #[arg(long, default_value = "")]
        machine: String,
        /// 무엇을 작업했는지 메모
        #[arg(long, default_value = "")]
        note: String,
        /// 사람이 읽는 요약 대신 JSON 출력
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// 프로젝트 구조·언어·TODO를 로컬에서 스캔 (파일 수정/명령 실행 없음)
    Scan {
        path: String,
        #[arg(long)]
        json: bool,
    },
    /// 승인된 최소 Evidence만 Claude→GPT Council로 분석 (읽기 전용)
    Analyze {
        path: String,
        #[arg(long)]
        objective: Option<String>,
    },
}

#[derive(Subcommand)]
enum UsageCommands {
    /// 기간 요약 — 추정 비용·호출·토큰
    Summary {
        #[arg(long, default_value = "month")]
        period: String,
    },
    /// provider·모델별 분해
    Providers {
        #[arg(long, default_value = "month")]
        period: String,
    },
    /// 최근 세션 목록 (메타데이터만)
    Sessions {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// 기록 내보내기: usage export --format json|csv [--out FILE]
    Export {
        #[arg(long, default_value = "json")]
        format: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// 모든 세션 기록 삭제
    Clear,
    /// 기록 켜기/끄기: usage recording on|off
    Recording { state: String },
    /// 보존 기간(일) 설정, 0=무기한
    Retention { days: u32 },
    /// 단가표 — 공개 단가를 직접 입력해야 비용이 계산됩니다
    Pricing {
        #[command(subcommand)]
        action: PricingCommands,
    },
}

#[derive(Subcommand)]
enum PricingCommands {
    /// 현재 단가표 보기
    Show,
    /// 모델 단가 설정 (100만 토큰당, provider 콘솔의 공개 단가)
    Set {
        model: String,
        #[arg(long)]
        input: f64,
        #[arg(long)]
        output: f64,
        #[arg(long)]
        cache: Option<f64>,
        /// 단가를 확인한 날짜 YYYY-MM-DD
        #[arg(long)]
        date: String,
        /// 출처(콘솔 URL 등)
        #[arg(long)]
        source: Option<String>,
    },
    /// 모델 단가 삭제
    Remove { model: String },
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
    /// 쿨러 부하 테스트 — N초간 전코어 부하 → 온도가 임계(°C)를 넘는지 판정 (기본 5분/85°C)
    Thermal {
        #[arg(long, default_value_t = 300)]
        seconds: u64,
        #[arg(long, default_value_t = 85.0)]
        limit: f32,
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
        /// 이 대화를 이어간다 (id는 `chorus chat list`). 다른 모델로 이어가도 맥락이 전달됩니다
        #[arg(long)]
        conversation: Option<String>,
    },
    /// 저장된 대화 — 새로 만들기·목록·보기·삭제 (전부 이 PC 안에만 저장)
    Chat {
        #[command(subcommand)]
        action: ChatCommands,
    },
    /// 연결된 AI 목록 + 모델/키/정책 상태
    Models,
    /// 모델 설정 — 내장 provider가 쓸 모델 ID 지정/초기화: chorus model set <provider> <id>
    Model {
        #[command(subcommand)]
        action: ModelCommands,
    },
    /// 클라우드 호출 동의 — provider별 명시적 consent: chorus consent <provider> [--scope minimal|system|drivers]
    Consent {
        provider: String,
        /// 허용할 최대 데이터 범위 (기본 minimal)
        #[arg(long, default_value = "minimal")]
        scope: String,
    },
    /// 동의 철회 — provider를 deny-by-default로: chorus revoke <provider>
    Revoke { provider: String },
    /// API 키 직접 입력·저장 (.env): chorus set <provider> <key>
    Set { provider: String, key: String },
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
    Consensus { question: String },
}

#[derive(Subcommand)]
enum ChatCommands {
    /// 새 대화 시작: chat new "제목" [--use <provider>]
    New {
        title: String,
        #[arg(long = "use", default_value = "claude")]
        use_model: String,
    },
    /// 대화 목록 (최근 순)
    List,
    /// 대화 전체 내용 보기
    Show { id: String },
    /// 대화 삭제 (되돌릴 수 없음)
    Delete { id: String },
    /// 대화를 Markdown 파일로 내보내기 (노트 앱으로 옮길 때). 기존 파일은 덮어쓰지 않음
    Export {
        id: String,
        /// 저장할 경로. 생략하면 APEX 리포트 폴더
        #[arg(long)]
        out: Option<String>,
        /// 이미 있는 파일을 덮어쓴다
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ModelCommands {
    /// provider의 모델 ID 설정: chorus model set <provider> <model-id>
    Set { provider: String, model_id: String },
    /// provider의 모델을 기본값으로 초기화: chorus model reset <provider>
    Reset { provider: String },
}

#[tokio::main]
async fn main() {
    // 로그는 파일에만 남는다(터미널은 조용히). 가드를 main 끝까지 살려 flush 보장.
    let _log_guard = velox_core::logging::init();
    tracing::debug!(target: "velox::cli", "cli start");
    velox_core::metrics::record_start();
    dotenv().ok();

    // `Cli::parse()` 는 --version/--help/인자 오류에서 clap 이 곧바로 프로세스를 끝낸다.
    // 그러면 record_clean_exit 가 호출되지 않아 **정상 종료가 crash 로 집계된다**
    // (알파 지표의 crash 수가 부풀던 원인). 직접 받아서 정리한 뒤 종료한다.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            velox_core::metrics::record_clean_exit();
            // 도움말/버전은 stdout, 오류는 stderr + 비정상 종료 코드 — clap 의 기본 동작을 유지한다.
            e.print().ok();
            std::process::exit(e.exit_code());
        }
    };

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
            BenchCommands::Thermal { seconds, limit } => bench::run_thermal(seconds, limit),
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
        Commands::Snapshot { json, out } => snapshot::run(json, out),
        Commands::Compare { old, new } => compare::run(old, new),
        Commands::Checkpoint { action } => match action {
            CheckpointCommands::Save => checkpoint::save(),
            CheckpointCommands::List => checkpoint::list(),
            CheckpointCommands::Restore => checkpoint::restore_latest(),
        },
        Commands::Dashboard => dashboard::run().await,
        Commands::Daemon { interval, auto } => daemon::run(interval, auto).await,
        Commands::Chorus { action } => match action {
            ChorusCommands::Ask {
                prompt,
                use_model,
                no_context,
                conversation,
            } => {
                let (model, auto) = match use_model {
                    Some(m) => (m, false),
                    None => (velox_core::ai::route_semantic(&prompt).await, true),
                };
                if auto {
                    println!("→ Auto-routed to: {} (semantic)\n", model);
                } else {
                    println!("→ Using: {}\n", model);
                }
                chorus::ask_in(&prompt, &model, no_context, conversation.as_deref()).await;
            }
            ChorusCommands::Chat { action } => match action {
                ChatCommands::New { title, use_model } => chorus::chat_new(&title, &use_model),
                ChatCommands::List => chorus::chat_list(),
                ChatCommands::Show { id } => chorus::chat_show(&id),
                ChatCommands::Delete { id } => chorus::chat_delete(&id),
                ChatCommands::Export { id, out, force } => {
                    chorus::chat_export(&id, out.as_deref(), force)
                }
            },
            ChorusCommands::Models => {
                chorus::show_models();
            }
            ChorusCommands::Model { action } => match action {
                ModelCommands::Set { provider, model_id } => {
                    chorus::set_model(&provider, &model_id)
                }
                ModelCommands::Reset { provider } => chorus::reset_model(&provider),
            },
            ChorusCommands::Consent { provider, scope } => chorus::consent(&provider, &scope),
            ChorusCommands::Revoke { provider } => chorus::revoke(&provider),
            ChorusCommands::Set { provider, key } => chorus::set_key(&provider, &key),
            ChorusCommands::Add {
                name,
                base_url,
                model,
                key,
            } => chorus::add_provider(&name, &base_url, &model, &key),
            ChorusCommands::Test => chorus::test_all().await,
            ChorusCommands::Bench { hard } => chorus::bench(hard).await,
            ChorusCommands::Consensus { question } => chorus::consensus(&question).await,
        },
        Commands::Usage { action } => match action {
            UsageCommands::Summary { period } => usage::summary(&period),
            UsageCommands::Providers { period } => usage::providers(&period),
            UsageCommands::Sessions { limit } => usage::sessions(limit),
            UsageCommands::Export { format, out } => usage::export(&format, out.as_deref()),
            UsageCommands::Clear => usage::clear(),
            UsageCommands::Recording { state } => match state.trim().to_lowercase().as_str() {
                "on" | "true" => usage::recording(true),
                "off" | "false" => usage::recording(false),
                other => println!("✗ 알 수 없는 값: {other} (on / off)"),
            },
            UsageCommands::Retention { days } => usage::retention(days),
            UsageCommands::Pricing { action } => match action {
                PricingCommands::Show => usage::pricing_show(),
                PricingCommands::Set {
                    model,
                    input,
                    output,
                    cache,
                    date,
                    source,
                } => usage::pricing_set(&model, input, output, cache, &date, source.as_deref()),
                PricingCommands::Remove { model } => usage::pricing_remove(&model),
            },
        },
        Commands::Project { action } => match action {
            ProjectCommands::Scan { path, json } => project::scan(&path, json),
            ProjectCommands::Analyze { path, objective } => {
                project::analyze(&path, objective.as_deref()).await
            }
        },
        Commands::Report { action } => match action {
            ReportCommands::Capture { label, out, bench } => report::capture(&label, &out, bench),
            ReportCommands::Repair {
                before,
                after,
                out,
                machine,
                note,
                json,
            } => report::repair(&before, &after, out.as_deref(), &machine, &note, json),
        },
        Commands::Plans { action } => match action {
            PlansCommands::List { json } => plans::list(json),
            PlansCommands::Add {
                provider,
                kind,
                plan,
                amount,
                currency,
                cycle,
                renews,
                purpose,
                budget,
                note,
                id,
            } => plans::add(plans::AddArgs {
                provider,
                kind,
                plan,
                amount,
                currency,
                cycle,
                renews,
                purpose,
                budget,
                note,
                id,
            }),
            PlansCommands::Remove { id } => plans::remove(&id),
            PlansCommands::Confirm { id, renews } => plans::confirm(&id, renews.as_deref()),
            PlansCommands::Upcoming { days } => plans::upcoming(days),
        },
        Commands::System { action } => match action {
            SystemCommands::Status { json } => system::status(json),
            SystemCommands::Services { stopped, json } => system::services(stopped, json),
            SystemCommands::Startup { json } => system::startup(json),
            SystemCommands::Save => system::save_baseline(),
            SystemCommands::Changes { json } => system::changes(json),
        },
        Commands::Metrics { action } => match action {
            MetricsCommands::Summary { json } => metrics::summary(json),
            MetricsCommands::Export { out } => metrics::export(out.as_deref()),
            MetricsCommands::Clear => metrics::clear(),
            MetricsCommands::FalseWarning => metrics::false_warning(),
        },
    }

    // 여기까지 왔으면 정상 종료다. 표식을 지워 다음 실행이 crash 로 세지 않게 한다.
    velox_core::metrics::record_clean_exit();
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
    struct Gpu {
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

    let Ok(com) = COMLibrary::new() else {
        println!("시스템 정보 조회 실패: COM 초기화 불가.");
        return;
    };
    let Ok(wmi) = WMIConnection::new(com) else {
        println!("시스템 정보 조회 실패: WMI 연결 불가.");
        return;
    };

    let cpus: Vec<Processor> = wmi.query().unwrap_or_default();
    if cpus.is_empty() {
        println!("CPU:    조회 실패");
    }
    for cpu in &cpus {
        println!("CPU:    {}", cpu.name);
        println!("Cores:  {}", cpu.number_of_cores);
    }

    println!();

    let gpus: Vec<Gpu> = wmi.query().unwrap_or_default();
    if gpus.is_empty() {
        println!("GPU:    조회 실패 또는 없음");
    }
    for gpu in &gpus {
        println!("GPU:    {}", gpu.name);
    }

    println!();

    let batteries: Vec<Battery> = wmi.query().unwrap_or_default();
    if batteries.is_empty() {
        println!("Battery: No battery detected");
    } else {
        for bat in &batteries {
            println!("Battery: {}%", bat.charge);
        }
    }

    println!();
    println!("--- Temperatures ---");
    match WMIConnection::with_namespace_path("ROOT\\WMI", com) {
        Ok(wmi2) => {
            let temps: Vec<ThermalZone> = wmi2.query().unwrap_or_default();
            if temps.is_empty() {
                println!("Temperature: Run as Administrator for sensor data");
            } else {
                for t in &temps {
                    let celsius = (t.current_temperature as f32 / 10.0) - 273.15;
                    println!("{}: {:.1}°C", t.instance_name, celsius);
                }
            }
        }
        Err(_) => println!("Temperature: 센서 접근 불가 (관리자 권한 필요)"),
    }

    println!("\n=== velox info complete ===");
}
