use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use super::config::PaseoIntegrationConfig;

pub const PASEO_READ_ONLY_TOOLS: &[&str] = &[
    "paseo_health",
    "paseo_list_agents",
    "paseo_get_agent_activity",
    "paseo_list_pending_permissions",
    "paseo_diagnose_agent",
    "paseo_monitor_snapshot",
];
pub const PASEO_ASSIST_TOOLS: &[&str] = &["paseo_send_agent_prompt"];
pub const PASEO_CONTROL_TOOLS: &[&str] = &[
    "paseo_stop_agent",
    "paseo_allow_permission",
    "paseo_deny_permission",
    "paseo_create_agent",
];
pub const PASEO_ALL_TOOLS: &[&str] = &[
    "paseo_health",
    "paseo_list_agents",
    "paseo_get_agent_activity",
    "paseo_list_pending_permissions",
    "paseo_diagnose_agent",
    "paseo_monitor_snapshot",
    "paseo_send_agent_prompt",
    "paseo_stop_agent",
    "paseo_allow_permission",
    "paseo_deny_permission",
    "paseo_create_agent",
];

#[derive(Debug, Clone)]
pub struct PaseoError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub stage: &'static str,
    pub details: Value,
}

impl PaseoError {
    pub fn argument(message: impl Into<String>) -> Self {
        Self::new(
            "PASEO_ARGUMENT_INVALID",
            message,
            false,
            "validate",
            json!({}),
        )
    }

    pub fn disabled() -> Self {
        Self::new(
            "PASEO_DISABLED",
            "Paseo integration is disabled for this workspace.",
            false,
            "policy",
            json!({}),
        )
    }

    pub fn denied(message: impl Into<String>) -> Self {
        Self::new("PASEO_ACCESS_DENIED", message, false, "policy", json!({}))
    }

    pub fn new(
        code: &'static str,
        message: impl Into<String>,
        retryable: bool,
        stage: &'static str,
        details: Value,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            stage,
            details,
        }
    }

    pub fn value(&self) -> Value {
        let mut details = self.details.as_object().cloned().unwrap_or_default();
        if !self.details.is_null() && !self.details.is_object() {
            details.insert("context".into(), self.details.clone());
        }
        details.insert("stage".into(), Value::String(self.stage.into()));
        json!({
            "code": self.code,
            "message": self.message,
            "category": "paseo",
            "retryable": self.retryable,
            "details": details
        })
    }
}

impl std::fmt::Display for PaseoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PaseoError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaseoAgentStatus {
    Running,
    Idle,
    Completed,
    Stopped,
    Crashed,
    Waiting,
    Unknown,
}

impl PaseoAgentStatus {
    pub fn from_cli(value: Option<&str>) -> Self {
        match value
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "running" | "active" | "busy" => Self::Running,
            "idle" | "waiting" => Self::Idle,
            "completed" | "complete" | "done" | "finished" => Self::Completed,
            "stopped" | "interrupted" | "cancelled" | "canceled" | "closed" => Self::Stopped,
            "crashed" | "failed" | "error" => Self::Crashed,
            "awaiting_input" | "waiting_permission" => Self::Waiting,
            _ => Self::Unknown,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Idle => "IDLE",
            Self::Completed => "COMPLETED",
            Self::Stopped => "STOPPED",
            Self::Crashed => "CRASHED",
            Self::Waiting => "WAITING",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaseoAgent {
    pub id: String,
    pub name: Option<String>,
    pub status: PaseoAgentStatus,
    pub provider: Option<String>,
    pub workspace: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub last_activity_at: Option<String>,
    pub labels: Vec<String>,
    pub missing_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub kind: String,
    pub event_type: String,
    pub occurred_at: Option<String>,
    pub summary: String,
    pub known: bool,
    pub is_progress: bool,
    pub is_waiting: bool,
    pub requires_user_action: bool,
    pub error_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSummary {
    pub request_id: Option<String>,
    pub agent_id: Option<String>,
    pub tool: Option<String>,
    pub permission_type: String,
    pub requested_at: Option<String>,
    pub summary: String,
    pub control_safe: bool,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaseoCapabilities {
    pub list_agents: bool,
    pub activity: bool,
    pub permissions: bool,
    pub permission_details: bool,
    pub send_prompt: bool,
    pub stop_agent: bool,
    pub allow_permission: bool,
    pub deny_permission: bool,
    pub create_agent: bool,
    pub json_output: bool,
    pub warnings: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct PaseoCommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct PaseoCommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct ParsedPaseo<T> {
    pub data: T,
    pub source_format: &'static str,
    pub parser_version: &'static str,
    pub missing_fields: Vec<String>,
    pub warnings: Vec<Value>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaseoDaemonStatus {
    pub cli_version: String,
    pub daemon_version: Option<String>,
    pub reachable: bool,
    pub connection_type: String,
    pub raw_status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiagnosisClassification {
    Healthy,
    Completed,
    WaitingPermission,
    WaitingUserInput,
    ExternalWait,
    PossiblyStalled,
    RepeatedFailure,
    Crashed,
    IdleIncomplete,
    Unknown,
}

impl DiagnosisClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "HEALTHY",
            Self::Completed => "COMPLETED",
            Self::WaitingPermission => "WAITING_PERMISSION",
            Self::WaitingUserInput => "WAITING_USER_INPUT",
            Self::ExternalWait => "EXTERNAL_WAIT",
            Self::PossiblyStalled => "POSSIBLY_STALLED",
            Self::RepeatedFailure => "REPEATED_FAILURE",
            Self::Crashed => "CRASHED",
            Self::IdleIncomplete => "IDLE_INCOMPLETE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaseoDiagnosis {
    pub agent_id: String,
    pub classification: DiagnosisClassification,
    pub confidence: String,
    pub current_state: String,
    pub last_effective_progress_at: Option<String>,
    pub stalled_for_minutes: Option<u64>,
    pub blocking_stage: Option<String>,
    pub blocking_tool: Option<String>,
    pub pending_permission: bool,
    pub repeated_errors: Vec<Value>,
    pub evidence: Vec<String>,
    pub likely_causes: Vec<String>,
    pub recommended_actions: Vec<String>,
    pub safe_automatic_action: Option<String>,
    pub requires_user_action: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAgentState {
    pub agent_id: String,
    pub name: Option<String>,
    pub status: String,
    pub classification: DiagnosisClassification,
    pub activity_fingerprint: String,
    pub last_effective_progress_at: Option<String>,
    pub observed_at: String,
    #[serde(default)]
    pub degraded: bool,
    #[serde(default)]
    pub warnings: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMonitorSnapshot {
    pub snapshot_id: String,
    pub created_at: String,
    pub agents: Vec<StoredAgentState>,
}

#[derive(Debug, Clone)]
pub struct CachedHealth {
    pub observed_at: Instant,
    pub value: Value,
}

#[derive(Default)]
pub struct PaseoRuntimeState {
    pub health: Mutex<Option<CachedHealth>>,
    pub cli_version: Mutex<Option<String>>,
    pub last_list_at: Mutex<Option<Instant>>,
    pub sends: Mutex<HashMap<String, Vec<Instant>>>,
    pub stops: Mutex<HashSet<String>>,
    pub permission_operations: Mutex<HashSet<String>>,
    pub creates: Mutex<Vec<Instant>>,
    pub diagnoses: Mutex<HashSet<String>>,
    pub monitor_active: Mutex<bool>,
}

#[derive(Clone)]
pub struct PaseoRuntimeContext {
    pub workspace_id: String,
    pub workspace_path: PathBuf,
    pub config: PaseoIntegrationConfig,
    pub host: Option<String>,
    pub state: Arc<PaseoRuntimeState>,
}

impl PaseoRuntimeContext {
    pub fn disabled(workspace_path: PathBuf) -> Self {
        Self::new(
            String::new(),
            workspace_path,
            PaseoIntegrationConfig::default(),
            None,
        )
    }

    pub fn new(
        workspace_id: String,
        workspace_path: PathBuf,
        config: PaseoIntegrationConfig,
        host: Option<String>,
    ) -> Self {
        let host = host.filter(|value| !value.trim().is_empty());
        let state = shared_runtime_state(&workspace_id, &workspace_path, &config, host.as_deref());
        Self {
            workspace_id,
            workspace_path,
            config,
            host,
            state,
        }
    }

    pub fn connection_type(&self) -> &'static str {
        if self.config.host_configured {
            "remote"
        } else {
            "local"
        }
    }
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

pub fn parse_rfc3339(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn shared_runtime_state(
    workspace_id: &str,
    workspace_path: &std::path::Path,
    config: &PaseoIntegrationConfig,
    host: Option<&str>,
) -> Arc<PaseoRuntimeState> {
    static STATES: OnceLock<Mutex<HashMap<String, Weak<PaseoRuntimeState>>>> = OnceLock::new();
    let key_material = format!(
        "{workspace_id}\0{}\0{}\0{}",
        workspace_path.display(),
        serde_json::to_string(config).unwrap_or_default(),
        host.unwrap_or_default()
    );
    let key = format!("{:x}", Sha256::digest(key_material.as_bytes()));
    let mut states = STATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("paseo runtime state registry lock");
    states.retain(|_, state| state.strong_count() > 0);
    if let Some(state) = states.get(&key).and_then(Weak::upgrade) {
        return state;
    }
    let state = Arc::new(PaseoRuntimeState::default());
    states.insert(key, Arc::downgrade(&state));
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_details_are_flat_and_include_stage() {
        let value = PaseoError::new(
            "PASEO_COMMAND_FAILED",
            "failed",
            true,
            "connect",
            json!({"exit_code": 1}),
        )
        .value();
        assert_eq!(value["details"]["stage"], "connect");
        assert_eq!(value["details"]["exit_code"], 1);
        assert!(value["details"].get("details").is_none());
    }

    #[test]
    fn equivalent_workspace_contexts_share_rate_limit_state() {
        let path = PathBuf::from("shared-workspace");
        let config = PaseoIntegrationConfig::default();
        let first =
            PaseoRuntimeContext::new("workspace".into(), path.clone(), config.clone(), None);
        let second = PaseoRuntimeContext::new("workspace".into(), path, config, None);
        assert!(Arc::ptr_eq(&first.state, &second.state));
    }

    #[test]
    fn closed_cli_status_maps_to_stopped() {
        assert_eq!(
            PaseoAgentStatus::from_cli(Some("closed")),
            PaseoAgentStatus::Stopped
        );
    }
}
