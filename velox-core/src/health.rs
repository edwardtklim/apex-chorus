//! Deterministic, AI-independent system health report.

use serde::{Deserialize, Serialize};

use crate::snapshot::Snapshot;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HealthReport {
    pub version: String,
    pub snapshot: Snapshot,
    pub findings: Vec<Finding>,
}

impl HealthReport {
    pub fn collect() -> Self {
        Self::from_snapshot(Snapshot::collect())
    }

    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        let mut findings = Vec::new();
        if snapshot.cpu_usage >= 90.0 {
            findings.push(Finding {
                severity: Severity::Warning,
                code: "cpu.high_usage".into(),
                message: format!("CPU 사용률이 {:.0}%입니다.", snapshot.cpu_usage),
            });
        }
        match snapshot.max_temp_c {
            Some(temp) if temp >= 95.0 => findings.push(Finding {
                severity: Severity::Critical,
                code: "thermal.critical".into(),
                message: format!("최고 온도가 {:.1}°C입니다.", temp),
            }),
            Some(temp) if temp >= 85.0 => findings.push(Finding {
                severity: Severity::Warning,
                code: "thermal.high".into(),
                message: format!("최고 온도가 {:.1}°C입니다.", temp),
            }),
            None => findings.push(Finding {
                severity: Severity::Info,
                code: "thermal.unavailable".into(),
                message: "온도 센서를 읽지 못했습니다.".into(),
            }),
            _ => {}
        }
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            snapshot,
            findings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_hot_cpu_without_ai() {
        let snapshot = Snapshot {
            max_temp_c: Some(91.0),
            ..Snapshot::default()
        };
        let report = HealthReport::from_snapshot(snapshot);
        assert!(report.findings.iter().any(|f| f.code == "thermal.high"));
    }
}
