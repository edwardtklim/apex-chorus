//! velox-core::evidence — typed Evidence (v0.15 Step C.6.3).
//!
//! AI에게 보내는 **실제 payload는 오직 `EvidenceBundle`에서만** 만들어진다. 호출자가 임의
//! 문자열 prompt를 만들고 `scope=Minimal`이라 라벨만 붙이는 방식을 막기 위해, 데이터는
//! typed `EvidenceItem`으로 수집되고, 각 item의 민감도(sensitivity)가 사용자가 승인한
//! 범위(approved_scope)를 넘으면 **거부**된다. 임의 `serde_json::Value` payload는 타입상 불가.
//!
//! 원칙(불변조건 2.3): API key·secret·전체 사용자 경로·장치 serial은 Evidence로 만들지 않는다
//! (빌더 책임). AI prompt는 [`EvidenceBundle::to_prompt`] 하나만 생성한다.

use serde::{Deserialize, Serialize};

use crate::benchmark::CpuBenchmarkReport;
use crate::health::HealthReport;
use crate::privacy::ContextScope;
use crate::snapshot::{Snapshot, SnapshotDiff};

/// Evidence ID 허용 최대 길이.
pub const MAX_EVIDENCE_ID_LEN: usize = 64;

/// Evidence 항목을 가리키는 안정적 식별자. Council이 인용(cite)할 때 쓴다.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceId(pub String);

/// Evidence의 출처.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Health,
    Snapshot,
    SnapshotCompare,
    Benchmark,
    DriverScan,
    Project,
    User,
}

/// Evidence의 실제 값 — **typed 전용**. 임의 payload를 담을 수 없다.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EvidenceData {
    Metric {
        name: String,
        value: f64,
        unit: String,
    },
    Fact {
        name: String,
        value: String,
    },
    Finding {
        code: String,
        message: String,
    },
    Change {
        item: String,
        old: String,
        new: String,
    },
    CodeFinding {
        path: String,
        line: Option<u32>,
        message: String,
    },
}

/// 하나의 Evidence 항목.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EvidenceItem {
    pub id: EvidenceId,
    pub source: EvidenceSource,
    /// 이 항목의 데이터 민감도 — approved_scope를 넘으면 bundle이 거부된다.
    pub sensitivity: ContextScope,
    pub data: EvidenceData,
}

/// 사용자가 승인한 범위 안에서 검증된 Evidence 묶음.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EvidenceBundle {
    pub approved_scope: ContextScope,
    pub items: Vec<EvidenceItem>,
}

/// Evidence 검증 실패 — 모두 fail-closed(거부)로 이어진다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceError {
    /// 빈 bundle.
    Empty,
    /// 중복 EvidenceId.
    DuplicateId(String),
    /// 빈/과길이/허용 안 되는 문자를 가진 ID.
    InvalidId(String),
    /// item 민감도가 승인 범위를 초과.
    SensitivityExceedsScope {
        id: String,
        sensitivity: ContextScope,
        approved: ContextScope,
    },
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvidenceError::Empty => write!(f, "빈 Evidence bundle"),
            EvidenceError::DuplicateId(id) => write!(f, "중복 EvidenceId: {id}"),
            EvidenceError::InvalidId(id) => write!(f, "잘못된 EvidenceId: {id}"),
            EvidenceError::SensitivityExceedsScope {
                id,
                sensitivity,
                approved,
            } => write!(
                f,
                "Evidence {id}의 민감도 {sensitivity:?}가 승인 범위 {approved:?}를 초과"
            ),
        }
    }
}

impl std::error::Error for EvidenceError {}

/// EvidenceId가 유효한지 — 비어있지 않고, 최대 길이 이내, `[A-Za-z0-9._-]`만.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_EVIDENCE_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl EvidenceBundle {
    /// 검증하며 bundle을 만든다. 빈 bundle·중복/잘못된 ID·범위 초과는 거부(fail-closed).
    pub fn new(
        approved_scope: ContextScope,
        items: Vec<EvidenceItem>,
    ) -> Result<Self, EvidenceError> {
        if items.is_empty() {
            return Err(EvidenceError::Empty);
        }
        let mut seen = std::collections::BTreeSet::new();
        for it in &items {
            if !valid_id(&it.id.0) {
                return Err(EvidenceError::InvalidId(it.id.0.clone()));
            }
            if !seen.insert(it.id.0.clone()) {
                return Err(EvidenceError::DuplicateId(it.id.0.clone()));
            }
            if it.sensitivity > approved_scope {
                return Err(EvidenceError::SensitivityExceedsScope {
                    id: it.id.0.clone(),
                    sensitivity: it.sensitivity,
                    approved: approved_scope,
                });
            }
        }
        Ok(Self {
            approved_scope,
            items,
        })
    }

    /// 주어진 EvidenceId가 이 bundle에 존재하는지 (Council의 인용 검증용).
    pub fn contains(&self, id: &EvidenceId) -> bool {
        self.items.iter().any(|it| &it.id == id)
    }

    /// **AI prompt를 만드는 유일한 경로.** typed Evidence만 텍스트로 직렬화한다.
    pub fn to_prompt(&self) -> String {
        let mut s = format!(
            "[APEX Evidence · approved_scope={:?} · {} items]\n",
            self.approved_scope,
            self.items.len()
        );
        for it in &self.items {
            let body = match &it.data {
                EvidenceData::Metric { name, value, unit } => {
                    format!("{}: {}{}", name, fmt_num(*value), unit)
                }
                EvidenceData::Fact { name, value } => format!("{name}: {value}"),
                EvidenceData::Finding { code, message } => format!("[{code}] {message}"),
                EvidenceData::Change { item, old, new } => format!("{item}: {old} → {new}"),
                EvidenceData::CodeFinding {
                    path,
                    line,
                    message,
                } => match line {
                    Some(l) => format!("{path}:{l} {message}"),
                    None => format!("{path} {message}"),
                },
            };
            s.push_str(&format!("({}) {}\n", it.id.0, body));
        }
        s
    }
}

/// f64를 사람용으로 — 정수면 소수점 없이, 아니면 소수 첫째자리.
fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.1}")
    }
}

/// sensitivity가 scope 이내일 때만 item을 추가하는 헬퍼(데이터 최소화).
fn add(
    out: &mut Vec<EvidenceItem>,
    scope: ContextScope,
    id: &str,
    source: EvidenceSource,
    sensitivity: ContextScope,
    data: EvidenceData,
) {
    if sensitivity <= scope {
        out.push(EvidenceItem {
            id: EvidenceId(id.to_string()),
            source,
            sensitivity,
            data,
        });
    }
}

/// Snapshot에서 범위 안의 item만 모은다. (여러 빌더가 공유)
fn snapshot_items(
    s: &Snapshot,
    source: EvidenceSource,
    scope: ContextScope,
    out: &mut Vec<EvidenceItem>,
) {
    use ContextScope::{Drivers, Minimal, System};
    use EvidenceData::{Fact, Metric};

    add(
        out,
        scope,
        "snapshot.power_plan",
        source,
        Minimal,
        Fact {
            name: "전원 계획".into(),
            value: s.plan_label.clone(),
        },
    );
    add(
        out,
        scope,
        "snapshot.cpu_usage",
        source,
        Minimal,
        Metric {
            name: "CPU 사용률".into(),
            value: s.cpu_usage as f64,
            unit: "%".into(),
        },
    );
    if let Some(t) = s.max_temp_c {
        add(
            out,
            scope,
            "snapshot.max_temp_c",
            source,
            Minimal,
            Metric {
                name: "최고 온도".into(),
                value: t as f64,
                unit: "°C".into(),
            },
        );
    }
    // System 범위: CPU 모델·코어·RAM·OS·GPU 이름
    add(
        out,
        scope,
        "snapshot.cpu_model",
        source,
        System,
        Fact {
            name: "CPU".into(),
            value: s.system.cpu_model.clone(),
        },
    );
    add(
        out,
        scope,
        "snapshot.logical_cores",
        source,
        System,
        Metric {
            name: "논리 코어".into(),
            value: s.system.logical_cores as f64,
            unit: "".into(),
        },
    );
    add(
        out,
        scope,
        "snapshot.ram_total_mb",
        source,
        System,
        Metric {
            name: "RAM".into(),
            value: s.system.ram_total_mb as f64,
            unit: "MB".into(),
        },
    );
    add(
        out,
        scope,
        "snapshot.os",
        source,
        System,
        Fact {
            name: "OS".into(),
            value: s.system.os.clone(),
        },
    );
    for (i, g) in s.gpus.iter().enumerate() {
        add(
            out,
            scope,
            &format!("snapshot.gpu.{i}.name"),
            source,
            System,
            Fact {
                name: "GPU".into(),
                value: g.name.clone(),
            },
        );
    }
    // Drivers 범위: 장치명 + 버전
    for (i, d) in s.drivers.iter().enumerate() {
        add(
            out,
            scope,
            &format!("snapshot.driver.{i}"),
            source,
            Drivers,
            Fact {
                name: d.device.clone(),
                value: d.version.clone(),
            },
        );
    }
}

impl EvidenceBundle {
    /// `Snapshot` → 승인 범위 안의 Evidence.
    pub fn from_snapshot(
        s: &Snapshot,
        approved_scope: ContextScope,
    ) -> Result<Self, EvidenceError> {
        let mut items = Vec::new();
        snapshot_items(s, EvidenceSource::Snapshot, approved_scope, &mut items);
        Self::new(approved_scope, items)
    }

    /// `HealthReport` → 결정론적 findings + 근거 snapshot 지표.
    pub fn from_health_report(
        r: &HealthReport,
        approved_scope: ContextScope,
    ) -> Result<Self, EvidenceError> {
        let mut items = Vec::new();
        snapshot_items(
            &r.snapshot,
            EvidenceSource::Snapshot,
            approved_scope,
            &mut items,
        );
        for (i, f) in r.findings.iter().enumerate() {
            // findings는 부하/온도에 관한 요약(Minimal 수준).
            add(
                &mut items,
                approved_scope,
                &format!("health.finding.{i}"),
                EvidenceSource::Health,
                ContextScope::Minimal,
                EvidenceData::Finding {
                    code: f.code.clone(),
                    message: f.message.clone(),
                },
            );
        }
        Self::new(approved_scope, items)
    }

    /// `SnapshotDiff` → 하드웨어/드라이버/전원 변화. (수리 전후 비교)
    pub fn from_snapshot_diff(
        d: &SnapshotDiff,
        approved_scope: ContextScope,
    ) -> Result<Self, EvidenceError> {
        let mut items = Vec::new();
        for (i, c) in d.changed.iter().enumerate() {
            add(
                &mut items,
                approved_scope,
                &format!("diff.changed.{i}"),
                EvidenceSource::SnapshotCompare,
                ContextScope::System,
                EvidenceData::Change {
                    item: c.item.clone(),
                    old: c.old.clone(),
                    new: c.new.clone(),
                },
            );
        }
        for (i, a) in d.added.iter().enumerate() {
            add(
                &mut items,
                approved_scope,
                &format!("diff.added.{i}"),
                EvidenceSource::SnapshotCompare,
                ContextScope::Drivers,
                EvidenceData::Fact {
                    name: "추가된 장치".into(),
                    value: a.clone(),
                },
            );
        }
        for (i, r) in d.removed.iter().enumerate() {
            add(
                &mut items,
                approved_scope,
                &format!("diff.removed.{i}"),
                EvidenceSource::SnapshotCompare,
                ContextScope::Drivers,
                EvidenceData::Fact {
                    name: "제거된 장치".into(),
                    value: r.clone(),
                },
            );
        }
        Self::new(approved_scope, items)
    }

    /// `CpuBenchmarkReport` → 성능 지표(대부분 Minimal, 코어 수는 System).
    pub fn from_cpu_benchmark(
        b: &CpuBenchmarkReport,
        approved_scope: ContextScope,
    ) -> Result<Self, EvidenceError> {
        use ContextScope::{Minimal, System};
        use EvidenceData::Metric;
        let mut items = Vec::new();
        let m = |name: &str, value: f64, unit: &str| Metric {
            name: name.into(),
            value,
            unit: unit.into(),
        };
        add(
            &mut items,
            approved_scope,
            "bench.single_score",
            EvidenceSource::Benchmark,
            Minimal,
            m("싱글 점수", b.single_score, ""),
        );
        add(
            &mut items,
            approved_scope,
            "bench.multi_score",
            EvidenceSource::Benchmark,
            Minimal,
            m("멀티 점수", b.multi_score, ""),
        );
        add(
            &mut items,
            approved_scope,
            "bench.logical_cores",
            EvidenceSource::Benchmark,
            System,
            m("논리 코어", b.logical_cores as f64, ""),
        );
        Self::new(approved_scope, items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{Change, DriverInfo, GpuInfo, SnapshotDiff, SystemInfo};

    fn item(id: &str, sens: ContextScope) -> EvidenceItem {
        EvidenceItem {
            id: EvidenceId(id.into()),
            source: EvidenceSource::Snapshot,
            sensitivity: sens,
            data: EvidenceData::Fact {
                name: "n".into(),
                value: "v".into(),
            },
        }
    }

    fn rich_snapshot() -> Snapshot {
        Snapshot {
            cpu_usage: 42.0,
            max_temp_c: Some(71.0),
            system: SystemInfo {
                cpu_model: "TestCPU".into(),
                logical_cores: 8,
                ram_total_mb: 16000,
                os: "Windows".into(),
                kernel: "nt".into(),
            },
            gpus: vec![GpuInfo {
                name: "TestGPU".into(),
                driver_version: "1.0".into(),
                driver_date: "2026".into(),
            }],
            drivers: vec![DriverInfo {
                device: "TestDevice".into(),
                version: "2.0".into(),
            }],
            ..Snapshot::default()
        }
    }

    #[test]
    fn empty_bundle_rejected() {
        assert_eq!(
            EvidenceBundle::new(ContextScope::Minimal, vec![]).unwrap_err(),
            EvidenceError::Empty
        );
    }

    #[test]
    fn duplicate_id_rejected() {
        let items = vec![
            item("dup", ContextScope::Minimal),
            item("dup", ContextScope::Minimal),
        ];
        assert_eq!(
            EvidenceBundle::new(ContextScope::Minimal, items).unwrap_err(),
            EvidenceError::DuplicateId("dup".into())
        );
    }

    #[test]
    fn invalid_id_rejected() {
        // 허용 안 되는 문자
        assert_eq!(
            EvidenceBundle::new(
                ContextScope::Minimal,
                vec![item("bad id!", ContextScope::Minimal)]
            )
            .unwrap_err(),
            EvidenceError::InvalidId("bad id!".into())
        );
        // 과길이
        let long = "x".repeat(MAX_EVIDENCE_ID_LEN + 1);
        assert!(matches!(
            EvidenceBundle::new(
                ContextScope::Minimal,
                vec![item(&long, ContextScope::Minimal)]
            ),
            Err(EvidenceError::InvalidId(_))
        ));
    }

    #[test]
    fn sensitivity_exceeding_scope_rejected() {
        // Drivers 민감도인데 승인은 Minimal → 거부 (fail-closed)
        let err = EvidenceBundle::new(
            ContextScope::Minimal,
            vec![item("d", ContextScope::Drivers)],
        )
        .unwrap_err();
        assert!(matches!(err, EvidenceError::SensitivityExceedsScope { .. }));
    }

    #[test]
    fn snapshot_builder_minimises_by_scope() {
        // Minimal: system/driver 항목 없음
        let b = EvidenceBundle::from_snapshot(&rich_snapshot(), ContextScope::Minimal).unwrap();
        assert!(b.contains(&EvidenceId("snapshot.cpu_usage".into())));
        assert!(!b.contains(&EvidenceId("snapshot.cpu_model".into())));
        assert!(!b.contains(&EvidenceId("snapshot.driver.0".into())));
        // Drivers: 전부 포함
        let full = EvidenceBundle::from_snapshot(&rich_snapshot(), ContextScope::Drivers).unwrap();
        assert!(full.contains(&EvidenceId("snapshot.cpu_model".into())));
        assert!(full.contains(&EvidenceId("snapshot.gpu.0.name".into())));
        assert!(full.contains(&EvidenceId("snapshot.driver.0".into())));
        // 모든 item 민감도 ≤ 승인 범위
        assert!(
            full.items
                .iter()
                .all(|i| i.sensitivity <= full.approved_scope)
        );
    }

    #[test]
    fn to_prompt_cites_ids() {
        let b = EvidenceBundle::from_snapshot(&rich_snapshot(), ContextScope::Minimal).unwrap();
        let p = b.to_prompt();
        assert!(p.contains("(snapshot.cpu_usage)"));
        assert!(p.contains("approved_scope=Minimal"));
    }

    #[test]
    fn diff_builder_scopes_added_removed_as_drivers() {
        let diff = SnapshotDiff {
            changed: vec![Change {
                item: "전원 계획".into(),
                old: "균형".into(),
                new: "고성능".into(),
            }],
            added: vec!["새 장치".into()],
            removed: vec![],
        };
        // System 승인: changed는 있고 added(장치명=Drivers)는 제외
        let b = EvidenceBundle::from_snapshot_diff(&diff, ContextScope::System).unwrap();
        assert!(b.contains(&EvidenceId("diff.changed.0".into())));
        assert!(!b.contains(&EvidenceId("diff.added.0".into())));
    }

    #[test]
    fn bundle_round_trips_through_json() {
        let b = EvidenceBundle::from_snapshot(&rich_snapshot(), ContextScope::System).unwrap();
        let json = serde_json::to_string(&b).unwrap();
        let back: EvidenceBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }
}
