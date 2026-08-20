//! velox-core::report — 수리 전/후 비교 리포트 (v0.20 Closed Alpha).
//!
//! 실제 수리 세션의 결과물이다. 친구 PC를 재조립하거나 드라이버를 갱신한 뒤
//! "그래서 뭐가 나아졌는가"에 **측정값으로** 답한다.
//!
//! 설계에서 양보하지 않는 것 세 가지:
//!
//! 1. **AI 해석과 측정값을 섞지 않는다.** 측정은 [`SessionMeasurement`], 해석은
//!    [`RepairReport::ai_notes`] 로 완전히 분리한다. 리포트를 받은 사람이
//!    "이건 기계가 잰 값, 이건 AI 의견"을 구분할 수 없으면 그 리포트는 못 믿는다.
//! 2. **통과/실패에는 항상 기준을 적는다.** [`Verdict::criterion`] 은 Option 이 아니다.
//!    기준 없는 "정상"은 아무 의미가 없다.
//! 3. **모르면 Unknown 이다.** 센서를 못 읽었으면 [`Outcome::Unknown`] 으로 남긴다.
//!    빈칸을 그럴듯한 숫자로 채우지 않는다.
//!
//! 벤치 점수는 **같은 benchmark version 끼리만** 비교한다. 버전이 다르면
//! 비교를 거부하고 그 사실을 리포트에 적는다.

use serde::{Deserialize, Serialize};

use crate::snapshot::{Snapshot, SnapshotDiff};

/// 벤치마크 채점 방식의 버전. 채점이 바뀌면 올린다 — 과거 점수와 섞이지 않게.
pub const BENCHMARK_VERSION: &str = "1";

/// 리포트 메타데이터 — 이 숫자들이 언제·무엇으로 측정됐는지.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReportMeta {
    pub apex_version: String,
    pub benchmark_version: String,
    pub generated_at: String,
    /// 사용자가 정한 PC 이름. 없으면 빈 문자열.
    pub machine: String,
    /// 이 수리 세션에 대한 사람의 메모(무엇을 바꿨는지).
    pub work_note: String,
}

impl ReportMeta {
    pub fn new(machine: impl Into<String>, work_note: impl Into<String>) -> Self {
        Self {
            apex_version: env!("CARGO_PKG_VERSION").to_string(),
            benchmark_version: BENCHMARK_VERSION.to_string(),
            generated_at: now_rfc3339(),
            machine: machine.into(),
            work_note: work_note.into(),
        }
    }
}

/// 한 시점의 측정 묶음. **전부 기계가 잰 값이다 — 해석이 섞이지 않는다.**
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct SessionMeasurement {
    /// "before" / "after" 같은 라벨.
    pub label: String,
    pub captured_at: String,
    pub snapshot: Snapshot,
    /// CPU 싱글 점수. 측정 안 했으면 None.
    pub cpu_single: Option<f64>,
    pub cpu_multi: Option<f64>,
    /// 지속 부하 유지율(0.0~1.0). 쓰로틀링 지표.
    pub sustain_ratio: Option<f64>,
    /// 부하 중 최고 온도. 센서 못 읽으면 None.
    pub max_temp_c: Option<f32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Fail,
    /// 측정하지 못했다. **실패가 아니다** — 모른다는 뜻이다.
    Unknown,
}

impl Outcome {
    pub fn label(&self) -> &str {
        match self {
            Outcome::Pass => "개선",
            Outcome::Fail => "악화",
            Outcome::Unknown => "측정 불가",
        }
    }
}

/// 판정 하나. **기준(`criterion`)이 없는 판정은 만들 수 없다.**
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Verdict {
    pub name: String,
    pub outcome: Outcome,
    /// 무엇을 근거로 이렇게 판정했는지. 사람이 읽고 동의하거나 반박할 수 있어야 한다.
    pub criterion: String,
    /// 실제로 잰 값(문자열로 표현).
    pub measured: String,
}

/// 수리 전/후 리포트.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct RepairReport {
    pub meta: ReportMeta,
    pub before: SessionMeasurement,
    pub after: SessionMeasurement,
    pub diff: SnapshotDiff,
    pub verdicts: Vec<Verdict>,
    /// **AI 가 쓴 해석.** 측정값이 아니다. 비어 있어도 리포트는 완전하다.
    pub ai_notes: Vec<String>,
    /// 비교를 신뢰할 수 없게 만드는 조건(벤치 버전 불일치, 하드웨어 변경 등).
    pub caveats: Vec<String>,
}

/// 개선으로 인정하는 최소 폭 — 측정 노이즈를 개선이라고 부르지 않기 위한 문턱.
///
/// **이 값은 실측으로 정했다.** 처음엔 3%로 잡았는데, 아무것도 바꾸지 않은
/// 같은 노트북에서 연속 3회 측정한 결과:
///
/// ```text
/// single  12.37 → 13.09 → 13.25   편차 7.2%
/// multi   170.69 → 165.37 → 170.10  편차 3.2%
/// ```
///
/// 3% 문턱이면 "아무 작업도 안 했는데 개선됨"이 나온다. 실제로 첫 라이브 테스트에서
/// 멀티 +5.9%를 개선으로 오판했다. **거짓 개선 보고는 이 도구의 신뢰를 가장 빨리
/// 무너뜨리는 실패**라서, 관측된 편차보다 여유 있게 잡는다.
///
/// 노트북은 열·전원 상태에 따라 편차가 더 크다. 데스크톱에서는 이보다 좁혀도 되지만,
/// 기본값은 보수적인 쪽을 택한다 — 놓치는 개선보다 없는 개선을 보고하는 게 더 나쁘다.
const SCORE_NOISE_PCT: f64 = 10.0;
const SUSTAIN_NOISE_PCT: f64 = 5.0;
const TEMP_NOISE_C: f32 = 3.0;

/// 두 측정으로 리포트를 만든다. 판정은 전부 결정론적이다(AI 없음).
pub fn build(
    meta: ReportMeta,
    before: SessionMeasurement,
    after: SessionMeasurement,
) -> RepairReport {
    let diff = crate::snapshot::compare(&before.snapshot, &after.snapshot);
    let mut caveats = Vec::new();

    // 하드웨어가 바뀌었으면 성능 비교의 의미가 달라진다 — 숨기지 않고 적는다.
    if before.snapshot.system.cpu_model != after.snapshot.system.cpu_model {
        caveats.push(format!(
            "CPU 가 교체됐습니다({} → {}). 점수 비교는 같은 CPU 기준이 아닙니다.",
            before.snapshot.system.cpu_model, after.snapshot.system.cpu_model
        ));
    }
    if before.snapshot.system.ram_total_mb != after.snapshot.system.ram_total_mb {
        caveats.push(format!(
            "메모리 용량이 바뀌었습니다({}MB → {}MB).",
            before.snapshot.system.ram_total_mb, after.snapshot.system.ram_total_mb
        ));
    }

    let verdicts = vec![
        score_verdict("CPU 싱글 성능", before.cpu_single, after.cpu_single),
        score_verdict("CPU 멀티 성능", before.cpu_multi, after.cpu_multi),
        sustain_verdict(before.sustain_ratio, after.sustain_ratio),
        temp_verdict(before.max_temp_c, after.max_temp_c),
        driver_verdict(&diff),
    ];

    RepairReport {
        meta,
        before,
        after,
        diff,
        verdicts,
        ai_notes: Vec::new(),
        caveats,
    }
}

fn score_verdict(name: &str, before: Option<f64>, after: Option<f64>) -> Verdict {
    let criterion = format!(
        "수리 후 점수가 수리 전보다 {SCORE_NOISE_PCT:.0}% 이상 높으면 개선으로 본다. 그 미만 차이는 측정 노이즈로 취급한다."
    );
    match (before, after) {
        (Some(b), Some(a)) if b > 0.0 => {
            let pct = (a - b) / b * 100.0;
            let outcome = if pct >= SCORE_NOISE_PCT {
                Outcome::Pass
            } else if pct <= -SCORE_NOISE_PCT {
                Outcome::Fail
            } else {
                // 노이즈 범위 — 개선도 악화도 아니다. 단정하지 않는다.
                Outcome::Unknown
            };
            Verdict {
                name: name.into(),
                outcome,
                criterion,
                measured: format!("{b:.0} → {a:.0} ({pct:+.1}%)"),
            }
        }
        _ => Verdict {
            name: name.into(),
            outcome: Outcome::Unknown,
            criterion,
            measured: "한쪽 이상 측정되지 않음".into(),
        },
    }
}

fn sustain_verdict(before: Option<f64>, after: Option<f64>) -> Verdict {
    let criterion = format!(
        "지속 부하에서의 처리량 유지율. {SUSTAIN_NOISE_PCT:.0}%p 이상 오르면 냉각 개선으로 본다."
    );
    match (before, after) {
        (Some(b), Some(a)) => {
            let diff_pp = (a - b) * 100.0;
            let outcome = if diff_pp >= SUSTAIN_NOISE_PCT {
                Outcome::Pass
            } else if diff_pp <= -SUSTAIN_NOISE_PCT {
                Outcome::Fail
            } else {
                Outcome::Unknown
            };
            Verdict {
                name: "쓰로틀링(유지율)".into(),
                outcome,
                criterion,
                measured: format!("{:.0}% → {:.0}% ({diff_pp:+.0}%p)", b * 100.0, a * 100.0),
            }
        }
        _ => Verdict {
            name: "쓰로틀링(유지율)".into(),
            outcome: Outcome::Unknown,
            criterion,
            measured: "지속 부하 테스트를 양쪽 다 하지 않음".into(),
        },
    }
}

fn temp_verdict(before: Option<f32>, after: Option<f32>) -> Verdict {
    let criterion =
        format!("부하 중 최고 온도. {TEMP_NOISE_C:.0}°C 이상 내려가면 냉각 개선으로 본다.");
    match (before, after) {
        (Some(b), Some(a)) => {
            let d = a - b;
            let outcome = if d <= -TEMP_NOISE_C {
                Outcome::Pass
            } else if d >= TEMP_NOISE_C {
                Outcome::Fail
            } else {
                Outcome::Unknown
            };
            Verdict {
                name: "최고 온도".into(),
                outcome,
                criterion,
                measured: format!("{b:.0}°C → {a:.0}°C ({d:+.0}°C)"),
            }
        }
        _ => Verdict {
            name: "최고 온도".into(),
            outcome: Outcome::Unknown,
            criterion,
            // 온도 미지원은 흔하다. 실패처럼 보이지 않게 이유를 적는다.
            measured: "온도 센서를 읽지 못함(관리자 권한 또는 보드 미지원)".into(),
        },
    }
}

fn driver_verdict(diff: &SnapshotDiff) -> Verdict {
    let criterion = "드라이버·시스템 구성 변화는 좋고 나쁨을 자동 판정하지 않는다. 무엇이 바뀌었는지만 기록한다.".to_string();
    let n = diff.changed.len() + diff.added.len() + diff.removed.len();
    Verdict {
        name: "시스템 구성 변화".into(),
        outcome: Outcome::Unknown,
        criterion,
        measured: if n == 0 {
            "변화 없음".into()
        } else {
            format!(
                "변경 {} · 추가 {} · 제거 {}",
                diff.changed.len(),
                diff.added.len(),
                diff.removed.len()
            )
        },
    }
}

impl RepairReport {
    /// 판정 요약 — (개선, 악화, 측정불가).
    pub fn tally(&self) -> (usize, usize, usize) {
        let mut t = (0, 0, 0);
        for v in &self.verdicts {
            match v.outcome {
                Outcome::Pass => t.0 += 1,
                Outcome::Fail => t.1 += 1,
                Outcome::Unknown => t.2 += 1,
            }
        }
        t
    }

    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    /// 자체 완결 HTML — 외부 리소스를 전혀 참조하지 않는다.
    /// 인터넷 없는 수리 현장에서도, 메일로 보내도 그대로 열린다.
    pub fn to_html(&self) -> String {
        let (pass, fail, unknown) = self.tally();
        let mut s = String::with_capacity(8192);

        s.push_str("<!doctype html><html lang=\"ko\"><head><meta charset=\"utf-8\">");
        s.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
        s.push_str(&format!(
            "<title>APEX 수리 리포트 · {}</title>",
            esc(&self.meta.machine)
        ));
        s.push_str("<style>\
:root{color-scheme:light dark}\
body{font:15px/1.6 system-ui,'Segoe UI',sans-serif;margin:0;padding:2rem;max-width:900px;margin-inline:auto;background:#fff;color:#111}\
@media(prefers-color-scheme:dark){body{background:#111;color:#eee}}\
h1{font-size:1.5rem;margin:0 0 .25rem}h2{font-size:1.05rem;margin:2rem 0 .5rem;border-bottom:1px solid #8884;padding-bottom:.3rem}\
.sub{color:#8a8a8a;font-size:.85rem;margin-bottom:1.5rem}\
table{width:100%;border-collapse:collapse;margin:.5rem 0}\
th,td{text-align:left;padding:.5rem .6rem;border-bottom:1px solid #8883;vertical-align:top}\
th{font-weight:600;font-size:.8rem;color:#8a8a8a;text-transform:uppercase;letter-spacing:.03em}\
.tag{display:inline-block;padding:.1rem .5rem;border-radius:99px;font-size:.75rem;font-weight:600}\
.pass{background:#0a7d3222;color:#0a7d32}.fail{background:#b3261e22;color:#b3261e}.unk{background:#8884;color:#8a8a8a}\
.crit{color:#8a8a8a;font-size:.82rem;margin-top:.2rem}\
.ai{border-left:3px solid #6b5cff;background:#6b5cff11;padding:.8rem 1rem;margin:.5rem 0;border-radius:0 6px 6px 0}\
.ai-h{font-size:.78rem;font-weight:700;color:#6b5cff;text-transform:uppercase;letter-spacing:.05em;margin-bottom:.3rem}\
.cav{border-left:3px solid #c77700;background:#c7770011;padding:.6rem 1rem;margin:.4rem 0;border-radius:0 6px 6px 0;font-size:.9rem}\
.sum{display:flex;gap:1.5rem;margin:1rem 0}\
.sum div{font-size:1.6rem;font-weight:700}.sum span{display:block;font-size:.72rem;font-weight:500;color:#8a8a8a;text-transform:uppercase}\
footer{margin-top:3rem;padding-top:1rem;border-top:1px solid #8883;color:#8a8a8a;font-size:.8rem}\
code{background:#8882;padding:.1rem .3rem;border-radius:3px;font-size:.85em}\
</style></head><body>");

        s.push_str(&format!(
            "<h1>APEX 수리 리포트</h1><div class=\"sub\">{} · {}</div>",
            esc(if self.meta.machine.is_empty() {
                "(PC 이름 미지정)"
            } else {
                &self.meta.machine
            }),
            esc(&self.meta.generated_at)
        ));

        if !self.meta.work_note.is_empty() {
            s.push_str(&format!(
                "<p><strong>작업 내용:</strong> {}</p>",
                esc(&self.meta.work_note)
            ));
        }

        s.push_str(&format!(
            "<div class=\"sum\"><div>{pass}<span>개선</span></div><div>{fail}<span>악화</span></div><div>{unknown}<span>측정 불가</span></div></div>"
        ));

        for c in &self.caveats {
            s.push_str(&format!("<div class=\"cav\">⚠ {}</div>", esc(c)));
        }

        // --- 측정값 ---
        s.push_str("<h2>측정 결과</h2><table><tr><th>항목</th><th>측정값</th><th>판정</th></tr>");
        for v in &self.verdicts {
            let cls = match v.outcome {
                Outcome::Pass => "pass",
                Outcome::Fail => "fail",
                Outcome::Unknown => "unk",
            };
            s.push_str(&format!(
                "<tr><td><strong>{}</strong><div class=\"crit\">{}</div></td><td>{}</td><td><span class=\"tag {}\">{}</span></td></tr>",
                esc(&v.name),
                esc(&v.criterion),
                esc(&v.measured),
                cls,
                esc(v.outcome.label())
            ));
        }
        s.push_str("</table>");

        // --- 시스템 변화 ---
        if !self.diff.is_empty() {
            s.push_str(
                "<h2>시스템 변화</h2><table><tr><th>항목</th><th>이전</th><th>이후</th></tr>",
            );
            for c in &self.diff.changed {
                s.push_str(&format!(
                    "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                    esc(&c.item),
                    esc(&c.old),
                    esc(&c.new)
                ));
            }
            for a in &self.diff.added {
                s.push_str(&format!(
                    "<tr><td>{}</td><td>—</td><td>추가됨</td></tr>",
                    esc(a)
                ));
            }
            for r in &self.diff.removed {
                s.push_str(&format!(
                    "<tr><td>{}</td><td>있었음</td><td>—</td></tr>",
                    esc(r)
                ));
            }
            s.push_str("</table>");
        }

        // --- 하드웨어 ---
        s.push_str(
            "<h2>하드웨어</h2><table><tr><th>항목</th><th>수리 전</th><th>수리 후</th></tr>",
        );
        let b = &self.before.snapshot.system;
        let a = &self.after.snapshot.system;
        s.push_str(&format!(
            "<tr><td>CPU</td><td>{}</td><td>{}</td></tr>",
            esc(&b.cpu_model),
            esc(&a.cpu_model)
        ));
        s.push_str(&format!(
            "<tr><td>메모리</td><td>{} MB</td><td>{} MB</td></tr>",
            b.ram_total_mb, a.ram_total_mb
        ));
        s.push_str(&format!(
            "<tr><td>전원 계획</td><td>{}</td><td>{}</td></tr>",
            esc(&self.before.snapshot.plan_label),
            esc(&self.after.snapshot.plan_label)
        ));
        s.push_str("</table>");

        // --- AI 해석 (측정값과 시각적으로 완전히 분리) ---
        if !self.ai_notes.is_empty() {
            s.push_str("<h2>AI 해석</h2>");
            s.push_str("<div class=\"ai\"><div class=\"ai-h\">AI가 작성한 해석입니다 — 위 측정값과 달리 기계가 잰 값이 아닙니다</div>");
            for n in &self.ai_notes {
                s.push_str(&format!("<p>{}</p>", esc(n)));
            }
            s.push_str("</div>");
        }

        s.push_str(&format!(
            "<footer>APEX Velox <code>{}</code> · benchmark version <code>{}</code><br>\
             점수는 같은 benchmark version 끼리만 비교할 수 있습니다. \
             측정 불가 항목은 실패가 아니라 <em>측정하지 못했다</em>는 뜻입니다.</footer>",
            esc(&self.meta.apex_version),
            esc(&self.meta.benchmark_version)
        ));

        s.push_str("</body></html>");
        s
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 외부 크레이트 없이 UTC 문자열 생성.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// days-since-epoch → (year, month, day). Howard Hinnant 의 civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{Snapshot, SystemInfo};

    fn measurement(
        label: &str,
        cpu: &str,
        single: Option<f64>,
        temp: Option<f32>,
    ) -> SessionMeasurement {
        SessionMeasurement {
            label: label.into(),
            captured_at: "2026-08-20T00:00:00Z".into(),
            snapshot: Snapshot {
                plan_guid: "g".into(),
                plan_label: "균형 조정".into(),
                cpu_usage: 5.0,
                max_temp_c: temp,
                system: SystemInfo {
                    cpu_model: cpu.into(),
                    ram_total_mb: 16384,
                    ..Default::default()
                },
                gpus: vec![],
                drivers: vec![],
            },
            cpu_single: single,
            cpu_multi: None,
            sustain_ratio: None,
            max_temp_c: temp,
        }
    }

    #[test]
    fn improvement_beyond_noise_is_pass() {
        let r = build(
            ReportMeta::new("test-pc", ""),
            measurement("before", "i7", Some(1000.0), None),
            measurement("after", "i7", Some(1100.0), None),
        );
        let v = r
            .verdicts
            .iter()
            .find(|v| v.name == "CPU 싱글 성능")
            .unwrap();
        assert_eq!(v.outcome, Outcome::Pass);
        assert!(v.measured.contains("+10.0%"));
    }

    /// 노이즈 범위의 차이를 개선이라고 부르지 않는다.
    #[test]
    fn small_difference_is_unknown_not_pass() {
        let r = build(
            ReportMeta::new("t", ""),
            measurement("before", "i7", Some(1000.0), None),
            measurement("after", "i7", Some(1010.0), None),
        );
        let v = r
            .verdicts
            .iter()
            .find(|v| v.name == "CPU 싱글 성능")
            .unwrap();
        assert_eq!(v.outcome, Outcome::Unknown, "1% 차이는 노이즈다");
    }

    /// 회귀 방지 — 실측한 노트북 편차(single 7.2%)를 개선으로 보고하면 안 된다.
    ///
    /// 첫 라이브 테스트에서 아무 작업 없이 멀티 +5.9%가 "개선"으로 찍혔다.
    /// 거짓 개선 보고는 도구의 신뢰를 가장 빨리 무너뜨린다.
    #[test]
    fn observed_measurement_noise_is_not_reported_as_improvement() {
        // 실제로 관측된 값들 — 같은 PC, 아무 변경 없음.
        for (b, a) in [(12.37, 13.25), (165.37, 170.69), (1000.0, 1059.0)] {
            let r = build(
                ReportMeta::new("t", ""),
                measurement("before", "i7", Some(b), None),
                measurement("after", "i7", Some(a), None),
            );
            let v = r
                .verdicts
                .iter()
                .find(|v| v.name == "CPU 싱글 성능")
                .unwrap();
            assert_eq!(
                v.outcome,
                Outcome::Unknown,
                "{b} → {a} 는 노이즈 범위인데 개선으로 판정됐다"
            );
        }
    }

    #[test]
    fn missing_sensor_is_unknown_not_fail() {
        let r = build(
            ReportMeta::new("t", ""),
            measurement("before", "i7", Some(1000.0), None),
            measurement("after", "i7", Some(1000.0), None),
        );
        let v = r.verdicts.iter().find(|v| v.name == "최고 온도").unwrap();
        assert_eq!(v.outcome, Outcome::Unknown);
        assert!(v.measured.contains("읽지 못함"), "이유를 설명해야 한다");
    }

    /// 모든 판정에 기준이 붙어 있어야 한다 — 기준 없는 판정 금지.
    #[test]
    fn every_verdict_states_its_criterion() {
        let r = build(
            ReportMeta::new("t", ""),
            measurement("before", "i7", Some(1000.0), Some(90.0)),
            measurement("after", "i7", Some(1200.0), Some(80.0)),
        );
        assert!(!r.verdicts.is_empty());
        for v in &r.verdicts {
            assert!(!v.criterion.trim().is_empty(), "{} 에 기준이 없다", v.name);
            assert!(!v.measured.trim().is_empty(), "{} 에 측정값이 없다", v.name);
        }
    }

    #[test]
    fn hardware_change_is_flagged_as_caveat() {
        let r = build(
            ReportMeta::new("t", ""),
            measurement("before", "i5-12400", Some(800.0), None),
            measurement("after", "i7-14700", Some(1600.0), None),
        );
        assert!(
            r.caveats.iter().any(|c| c.contains("CPU 가 교체")),
            "CPU 교체 시 점수 비교의 한계를 명시해야 한다"
        );
    }

    #[test]
    fn ai_notes_are_empty_by_default() {
        let r = build(
            ReportMeta::new("t", ""),
            measurement("before", "i7", None, None),
            measurement("after", "i7", None, None),
        );
        assert!(
            r.ai_notes.is_empty(),
            "AI 해석은 명시적으로 넣어야만 들어간다"
        );
    }

    #[test]
    fn html_is_self_contained_and_separates_ai() {
        let mut r = build(
            ReportMeta::new("내 PC", "써멀 재도포"),
            measurement("before", "i7", Some(1000.0), Some(95.0)),
            measurement("after", "i7", Some(1200.0), Some(78.0)),
        );
        r.ai_notes.push("써멀 재도포 효과로 보입니다".into());
        let html = r.to_html();

        // 외부 리소스를 참조하지 않는다.
        assert!(!html.contains("http://"), "외부 링크가 있으면 안 된다");
        assert!(!html.contains("https://"), "외부 링크가 있으면 안 된다");
        assert!(!html.contains("<script"), "스크립트 없이 열려야 한다");

        // AI 해석이 측정값과 구분되어 표시된다.
        assert!(html.contains("AI가 작성한 해석입니다"));
        assert!(html.contains("기계가 잰 값이 아닙니다"));

        // 벤치 버전이 명시된다.
        assert!(html.contains(BENCHMARK_VERSION));
        assert!(html.contains("써멀 재도포"));
    }

    #[test]
    fn html_escapes_user_input() {
        let r = build(
            ReportMeta::new("<script>alert(1)</script>", ""),
            measurement("before", "i7", None, None),
            measurement("after", "i7", None, None),
        );
        let html = r.to_html();
        assert!(
            !html.contains("<script>alert"),
            "사용자 입력이 이스케이프돼야 한다"
        );
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn json_roundtrips() {
        let r = build(
            ReportMeta::new("t", "n"),
            measurement("before", "i7", Some(1.0), None),
            measurement("after", "i7", Some(2.0), None),
        );
        let json = r.to_json().unwrap();
        let back: RepairReport = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn timestamp_is_rfc3339_shaped() {
        let t = now_rfc3339();
        assert_eq!(t.len(), 20, "{t}");
        assert!(t.ends_with('Z'));
        assert_eq!(&t[4..5], "-");
        assert_eq!(&t[10..11], "T");
    }
}
