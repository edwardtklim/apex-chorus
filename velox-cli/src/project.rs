use std::path::Path;
use std::sync::atomic::AtomicBool;

use velox_core::council::{CouncilEvent, CouncilStatus};
use velox_core::privacy::ContextScope;
use velox_core::project::{self, ProjectLimits, ProjectScan};

fn open_and_scan(path: &str) -> Result<(project::ProjectSession, ProjectScan), String> {
    let session =
        project::open(Path::new(path), ProjectLimits::default()).map_err(|e| e.to_string())?;
    let scan = session.scan();
    Ok((session, scan))
}

pub fn scan(path: &str, json: bool) {
    let (session, scan) = match open_and_scan(path) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("프로젝트 스캔 실패: {error}");
            return;
        }
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&scan).unwrap_or_else(|_| "{}".into())
        );
        return;
    }

    println!("=== APEX Project Scan · {} ===", session.name());
    println!("파일              : {}", scan.files.len());
    println!("스캔 크기         : {} bytes", scan.total_bytes);
    println!("제외한 비밀 파일  : {}", scan.skipped_secret_files);
    println!(
        "한도 도달         : {}",
        if scan.truncated { "예" } else { "아니요" }
    );

    if !scan.language_counts.is_empty() {
        println!("\n[언어]");
        for (language, count) in &scan.language_counts {
            println!("  {language:<14} {count}");
        }
    }
    if !scan.manifests.is_empty() {
        println!("\n[매니페스트]");
        for manifest in &scan.manifests {
            println!("  {manifest}");
        }
    }
    if !scan.detected_commands.is_empty() {
        println!("\n[감지한 명령 · 실행하지 않음]");
        for command in &scan.detected_commands {
            println!("  {command}");
        }
    }
    if !scan.todos.is_empty() {
        println!("\n[TODO/FIXME · 최대 {}개]", scan.todos.len());
        for todo in &scan.todos {
            println!("  {}:{}  {}", todo.rel_path, todo.line, todo.text);
        }
    }
    println!("\n읽기 전용 스캔 완료 · 파일 수정/명령 실행/클라우드 전송 없음");
}

pub async fn analyze(path: &str, objective: Option<&str>) {
    let (session, scan) = match open_and_scan(path) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("프로젝트 스캔 실패: {error}");
            return;
        }
    };
    let request = match project::analysis_request(&session, &scan, objective, ContextScope::Minimal)
    {
        Ok(request) => request,
        Err(error) => {
            eprintln!("Project Evidence 생성 실패: {error}");
            return;
        }
    };

    println!("=== APEX Project Analysis · {} ===", session.name());
    println!(
        "전송 범위: minimal · Evidence {}개 · 파일 본문/절대 경로/API 키 제외",
        request.evidence.items.len()
    );
    println!("흐름: Claude 제안 → GPT 검토 → APEX 결정론적 Gate");
    println!("읽기 전용: 파일 수정과 명령 실행은 지원하지 않습니다.\n");

    let on_event = |event: &CouncilEvent| match event {
        CouncilEvent::Evidence { items, .. } => println!("• Evidence 검증: {items}개"),
        CouncilEvent::Proposed { summary, findings } => {
            println!("• Claude 제안: {findings}개 발견 · {summary}")
        }
        CouncilEvent::Reviewed { verdict, reasons } => {
            println!("• GPT 검토: {verdict}");
            for reason in reasons {
                println!("  - {reason}");
            }
        }
        CouncilEvent::Revised { summary, findings } => {
            println!("• Claude 수정안: {findings}개 발견 · {summary}")
        }
        CouncilEvent::Gated { passed, reasons } => {
            println!("• APEX Gate: {}", if *passed { "통과" } else { "거부" });
            for reason in reasons {
                println!("  - {reason}");
            }
        }
    };

    let decision = velox_core::council::run(request, &AtomicBool::new(false), &on_event).await;
    println!("\n=== 결과: {:?} ===", decision.status);
    if let Some(proposal) = decision.proposal {
        println!("{}", proposal.summary);
        for finding in proposal.findings {
            let evidence = finding
                .evidence
                .iter()
                .map(|id| id.0.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            println!("- {} [{}]", finding.statement, evidence);
        }
    }
    if decision.status == CouncilStatus::Inconclusive {
        println!("분석을 완료하지 못했습니다. Provider 동의·키·네트워크를 확인하세요.");
    }
}
