use serde_json::Value;

use super::model::{
    ActivityEvent, ParsedPaseo, PaseoAgent, PaseoCapabilities, PaseoDaemonStatus, PaseoError,
    PermissionSummary,
};

pub trait PaseoClient {
    fn cli_version(&self) -> Result<String, PaseoError>;
    fn daemon_status(&self) -> Result<PaseoDaemonStatus, PaseoError>;
    fn detect_capabilities(&self) -> Result<PaseoCapabilities, PaseoError>;
    fn list_agents(&self) -> Result<ParsedPaseo<Vec<PaseoAgent>>, PaseoError>;
    fn get_activity(
        &self,
        agent_id: &str,
        tail: u32,
        filter: &str,
        max_bytes: usize,
    ) -> Result<ParsedPaseo<Vec<ActivityEvent>>, PaseoError>;
    fn list_pending_permissions(&self) -> Result<ParsedPaseo<Vec<PermissionSummary>>, PaseoError>;
    fn send_prompt(&self, agent_id: &str, prompt: &str) -> Result<Value, PaseoError>;
    fn stop_agent(&self, agent_id: &str) -> Result<Value, PaseoError>;
    fn allow_permission(&self, agent_id: &str, request_id: &str) -> Result<Value, PaseoError>;
    fn deny_permission(
        &self,
        agent_id: &str,
        request_id: &str,
        message: Option<&str>,
    ) -> Result<Value, PaseoError>;
    fn create_agent(
        &self,
        prompt: &str,
        title: Option<&str>,
        provider: &str,
        cwd: &str,
    ) -> Result<Value, PaseoError>;
}
