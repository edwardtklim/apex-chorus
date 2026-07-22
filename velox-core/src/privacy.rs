//! Explicit data minimisation for AI requests.

use serde::{Deserialize, Serialize};

use crate::snapshot::Snapshot;

/// 데이터 범위. 선언 순서 = 확대(escalation) 순서 — `Minimal < System < Drivers`.
/// 이 순서로 `PartialOrd`/`Ord`를 파생해 정책의 scope 초과 검사에 쓴다.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ContextScope {
    /// Only load/temperature/power state. Default for remote AI providers.
    #[default]
    Minimal,
    /// Adds CPU model, core count, RAM size, OS and GPU names.
    System,
    /// Adds installed driver names and versions. Requires explicit user choice.
    Drivers,
}

#[derive(Debug, Serialize)]
pub struct AiContext {
    pub scope: ContextScope,
    pub cpu_usage_pct: f32,
    pub max_temp_c: Option<f32>,
    pub power_plan: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<crate::snapshot::SystemInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drivers: Option<Vec<crate::snapshot::DriverInfo>>,
}

impl AiContext {
    pub fn from_snapshot(snapshot: &Snapshot, scope: ContextScope) -> Self {
        let include_system = matches!(scope, ContextScope::System | ContextScope::Drivers);
        Self {
            scope,
            cpu_usage_pct: snapshot.cpu_usage,
            max_temp_c: snapshot.max_temp_c,
            power_plan: snapshot.plan_label.clone(),
            system: include_system.then(|| snapshot.system.clone()),
            gpu_names: include_system
                .then(|| snapshot.gpus.iter().map(|gpu| gpu.name.clone()).collect()),
            drivers: matches!(scope, ContextScope::Drivers).then(|| snapshot.drivers.clone()),
        }
    }

    pub fn to_prompt_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::Snapshot;

    #[test]
    fn minimal_context_excludes_identity_and_drivers() {
        let mut snapshot = Snapshot::default();
        snapshot.system.cpu_model = "private cpu".into();
        snapshot.drivers.push(crate::snapshot::DriverInfo {
            device: "private device".into(),
            version: "1".into(),
        });
        let json = AiContext::from_snapshot(&snapshot, ContextScope::Minimal).to_prompt_json();
        assert!(!json.contains("private cpu"));
        assert!(!json.contains("private device"));
    }
}
