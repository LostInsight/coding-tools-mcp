use serde::{Deserialize, Serialize};

use super::model::PaseoError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaseoAccessMode {
    #[default]
    ReadOnly,
    Assist,
    Control,
}

impl PaseoAccessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Assist => "assist",
            Self::Control => "control",
        }
    }

    pub fn allows_assist(self) -> bool {
        matches!(self, Self::Assist | Self::Control)
    }

    pub fn allows_control(self) -> bool {
        matches!(self, Self::Control)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaseoMonitorConfig {
    #[serde(default = "default_monitor_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub workspace_only: bool,
    #[serde(default)]
    pub agent_ids: Vec<String>,
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub agent_name_patterns: Vec<String>,
    #[serde(default)]
    pub agent_labels: Vec<String>,
    #[serde(default = "default_exclude_patterns")]
    pub exclude_name_patterns: Vec<String>,
    #[serde(default = "default_stalled_after_minutes")]
    pub stalled_after_minutes: u32,
    #[serde(default = "default_repeat_error_threshold")]
    pub repeat_error_threshold: u32,
    #[serde(default = "default_activity_tail")]
    pub activity_tail: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaseoIntegrationConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub access_mode: PaseoAccessMode,
    #[serde(default)]
    pub binary_path: String,
    #[serde(default = "default_command_timeout_ms")]
    pub command_timeout_ms: u64,
    #[serde(default = "default_max_output_bytes")]
    pub max_output_bytes: usize,
    #[serde(default)]
    pub host_configured: bool,
    #[serde(default)]
    pub monitor: PaseoMonitorConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceIntegrations {
    #[serde(default)]
    pub paseo: PaseoIntegrationConfig,
}

impl Default for PaseoMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: default_monitor_enabled(),
            workspace_only: false,
            agent_ids: Vec::new(),
            workspaces: Vec::new(),
            agent_name_patterns: Vec::new(),
            agent_labels: Vec::new(),
            exclude_name_patterns: default_exclude_patterns(),
            stalled_after_minutes: default_stalled_after_minutes(),
            repeat_error_threshold: default_repeat_error_threshold(),
            activity_tail: default_activity_tail(),
        }
    }
}

impl Default for PaseoIntegrationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            access_mode: PaseoAccessMode::ReadOnly,
            binary_path: String::new(),
            command_timeout_ms: default_command_timeout_ms(),
            max_output_bytes: default_max_output_bytes(),
            host_configured: false,
            monitor: PaseoMonitorConfig::default(),
        }
    }
}

impl PaseoIntegrationConfig {
    pub fn validate(&self) -> Result<(), PaseoError> {
        if !(1_000..=120_000).contains(&self.command_timeout_ms) {
            return Err(PaseoError::argument(
                "command_timeout_ms must be between 1000 and 120000",
            ));
        }
        if !(1_024..=1_048_576).contains(&self.max_output_bytes) {
            return Err(PaseoError::argument(
                "max_output_bytes must be between 1024 and 1048576",
            ));
        }
        if !(1..=240).contains(&self.monitor.stalled_after_minutes) {
            return Err(PaseoError::argument(
                "stalled_after_minutes must be between 1 and 240",
            ));
        }
        if !(1..=10).contains(&self.monitor.repeat_error_threshold) {
            return Err(PaseoError::argument(
                "repeat_error_threshold must be between 1 and 10",
            ));
        }
        if !(1..=100).contains(&self.monitor.activity_tail) {
            return Err(PaseoError::argument(
                "activity_tail must be between 1 and 100",
            ));
        }
        if self.binary_path.len() > 1_024 {
            return Err(PaseoError::argument("binary_path is too long"));
        }
        if self
            .monitor
            .agent_name_patterns
            .iter()
            .chain(self.monitor.exclude_name_patterns.iter())
            .any(|pattern| pattern.len() > 256)
        {
            return Err(PaseoError::argument("agent name pattern is too long"));
        }
        validate_patterns("agent_name_patterns", &self.monitor.agent_name_patterns)?;
        validate_patterns("exclude_name_patterns", &self.monitor.exclude_name_patterns)?;
        validate_values("agent_labels", &self.monitor.agent_labels, 20, 120)?;
        validate_values("agent_ids", &self.monitor.agent_ids, 200, 128)?;
        validate_values("workspaces", &self.monitor.workspaces, 100, 4_096)?;
        for id in &self.monitor.agent_ids {
            super::policy::validate_agent_id(id)?;
        }
        Ok(())
    }
}

fn validate_patterns(key: &str, values: &[String]) -> Result<(), PaseoError> {
    validate_values(key, values, 20, 256)?;
    if values
        .iter()
        .any(|value| glob::Pattern::new(value).is_err())
    {
        return Err(PaseoError::argument(format!(
            "{key} contains an invalid glob"
        )));
    }
    Ok(())
}

fn validate_values(
    key: &str,
    values: &[String],
    max_items: usize,
    max_length: usize,
) -> Result<(), PaseoError> {
    if values.len() > max_items {
        return Err(PaseoError::argument(format!(
            "{key} contains too many values"
        )));
    }
    if values.iter().any(|value| value.len() > max_length) {
        return Err(PaseoError::argument(format!("{key} value is too long")));
    }
    Ok(())
}

fn default_monitor_enabled() -> bool {
    true
}

fn default_exclude_patterns() -> Vec<String> {
    vec!["paseo-supervisor".into()]
}

fn default_stalled_after_minutes() -> u32 {
    45
}

fn default_repeat_error_threshold() -> u32 {
    2
}

fn default_activity_tail() -> u32 {
    30
}

fn default_command_timeout_ms() -> u64 {
    15_000
}

fn default_max_output_bytes() -> usize {
    262_144
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_disabled_and_safe() {
        let config = PaseoIntegrationConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.access_mode, PaseoAccessMode::ReadOnly);
        assert_eq!(config.command_timeout_ms, 15_000);
        assert_eq!(config.max_output_bytes, 262_144);
        assert_eq!(config.monitor.exclude_name_patterns, ["paseo-supervisor"]);
    }

    #[test]
    fn rejects_unbounded_output_limit() {
        let config = PaseoIntegrationConfig {
            max_output_bytes: 1_048_577,
            ..PaseoIntegrationConfig::default()
        };
        assert_eq!(
            config.validate().unwrap_err().code,
            "PASEO_ARGUMENT_INVALID"
        );
    }

    #[test]
    fn rejects_unbounded_or_invalid_monitor_filters() {
        let mut config = PaseoIntegrationConfig::default();
        config.monitor.agent_labels = (0..21).map(|index| format!("label-{index}")).collect();
        assert_eq!(
            config.validate().unwrap_err().code,
            "PASEO_ARGUMENT_INVALID"
        );

        config.monitor.agent_labels.clear();
        config.monitor.agent_name_patterns = vec!["[invalid".into()];
        assert_eq!(
            config.validate().unwrap_err().code,
            "PASEO_ARGUMENT_INVALID"
        );
    }
}
