use serde_json::Value;

use super::model::{
    ActivityEvent, ParsedPaseo, PaseoAgent, PaseoDaemonStatus, PaseoError, PermissionSummary,
};

pub trait PaseoClient {
    fn cli_version(&self) -> Result<String, PaseoError>;
    fn daemon_status(&self) -> Result<PaseoDaemonStatus, PaseoError>;
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
}
