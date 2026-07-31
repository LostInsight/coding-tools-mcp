use std::time::{Duration, Instant};

use serde_json::Value;

use super::config::PaseoAccessMode;
use super::model::{
    PaseoError, PaseoRuntimeContext, PASEO_ALL_TOOLS, PASEO_ASSIST_TOOLS, PASEO_CONTROL_TOOLS,
};

pub fn is_paseo_tool(name: &str) -> bool {
    PASEO_ALL_TOOLS.contains(&name)
}

pub fn authorize(ctx: &PaseoRuntimeContext, tool: &str) -> Result<(), PaseoError> {
    if !is_paseo_tool(tool) {
        return Err(PaseoError::argument("unknown Paseo tool"));
    }
    if !ctx.config.enabled {
        return Err(PaseoError::disabled());
    }
    let mode = ctx.config.access_mode;
    let allowed = if PASEO_CONTROL_TOOLS.contains(&tool) {
        mode.allows_control()
    } else if PASEO_ASSIST_TOOLS.contains(&tool) {
        mode.allows_assist()
    } else {
        matches!(
            mode,
            PaseoAccessMode::ReadOnly | PaseoAccessMode::Assist | PaseoAccessMode::Control
        )
    };
    if allowed {
        Ok(())
    } else {
        Err(PaseoError::denied(format!(
            "Paseo access mode {} does not allow {tool}.",
            mode.as_str()
        )))
    }
}

pub fn validate_agent_id(value: &str) -> Result<(), PaseoError> {
    if value.is_empty() || value.len() > 128 || !agent_id_pattern().is_match(value) {
        return Err(PaseoError::argument("agent_id has an invalid format"));
    }
    Ok(())
}

pub fn validate_prompt(value: &str) -> Result<(), PaseoError> {
    if value.trim().is_empty() {
        return Err(PaseoError::argument("prompt must not be empty"));
    }
    if value.chars().count() > 8_000 {
        return Err(PaseoError::argument(
            "prompt must not exceed 8000 characters",
        ));
    }
    Ok(())
}

pub fn validate_host(value: Option<&str>) -> Result<(), PaseoError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(PaseoError::new(
            "PASEO_HOST_INVALID",
            "Configured Paseo host is invalid.",
            false,
            "host",
            Value::Null,
        ));
    }
    let (address, query) = value.strip_prefix("tcp://").map_or((value, None), |tail| {
        let (address, query) = tail.split_once('?').unwrap_or((tail, ""));
        (address, Some(query))
    });
    if !host_address_pattern().is_match(address) {
        return Err(PaseoError::new(
            "PASEO_HOST_INVALID",
            "Configured Paseo host is invalid.",
            false,
            "host",
            Value::Null,
        ));
    }
    if address
        .rsplit_once(':')
        .and_then(|(_, port)| port.parse::<u16>().ok())
        .is_none()
    {
        return Err(PaseoError::new(
            "PASEO_HOST_INVALID",
            "Configured Paseo host is invalid.",
            false,
            "host",
            Value::Null,
        ));
    }
    if let Some(query) = query {
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let Some((key, value)) = pair.split_once('=') else {
                return Err(PaseoError::new(
                    "PASEO_HOST_INVALID",
                    "Configured Paseo host is invalid.",
                    false,
                    "host",
                    Value::Null,
                ));
            };
            if !matches!(key, "ssl" | "password") || value.is_empty() || value.len() > 256 {
                return Err(PaseoError::new(
                    "PASEO_HOST_INVALID",
                    "Configured Paseo host is invalid.",
                    false,
                    "host",
                    Value::Null,
                ));
            }
            if key == "ssl" && !matches!(value, "true" | "false") {
                return Err(PaseoError::new(
                    "PASEO_HOST_INVALID",
                    "Configured Paseo host is invalid.",
                    false,
                    "host",
                    Value::Null,
                ));
            }
        }
    }
    Ok(())
}

pub fn check_list_interval(ctx: &PaseoRuntimeContext) -> Result<(), PaseoError> {
    let mut guard = ctx.state.last_list_at.lock().expect("paseo list lock");
    if guard.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
        return Err(rate_limited("list_agents", 2));
    }
    *guard = Some(Instant::now());
    Ok(())
}

pub fn begin_diagnosis(ctx: &PaseoRuntimeContext, agent_id: &str) -> Result<(), PaseoError> {
    let mut guard = ctx.state.diagnoses.lock().expect("paseo diagnosis lock");
    if !guard.insert(agent_id.to_string()) {
        return Err(rate_limited("diagnose_agent", 1));
    }
    Ok(())
}

pub fn finish_diagnosis(ctx: &PaseoRuntimeContext, agent_id: &str) {
    ctx.state
        .diagnoses
        .lock()
        .expect("paseo diagnosis lock")
        .remove(agent_id);
}

pub fn begin_monitor(ctx: &PaseoRuntimeContext) -> Result<(), PaseoError> {
    let mut active = ctx.state.monitor_active.lock().expect("paseo monitor lock");
    if *active {
        return Err(rate_limited("monitor_snapshot", 1));
    }
    *active = true;
    Ok(())
}

pub fn finish_monitor(ctx: &PaseoRuntimeContext) {
    *ctx.state.monitor_active.lock().expect("paseo monitor lock") = false;
}

pub fn begin_send(ctx: &PaseoRuntimeContext, agent_id: &str) -> Result<(), PaseoError> {
    let mut sends = ctx.state.sends.lock().expect("paseo send lock");
    let now = Instant::now();
    let entries = sends.entry(agent_id.to_string()).or_default();
    entries.retain(|time| now.duration_since(*time) < Duration::from_secs(60));
    if entries.len() >= 3 {
        return Err(rate_limited("send_agent_prompt", 60));
    }
    entries.push(now);
    Ok(())
}

pub fn begin_stop(ctx: &PaseoRuntimeContext, agent_id: &str) -> Result<(), PaseoError> {
    let mut stops = ctx.state.stops.lock().expect("paseo stop lock");
    if !stops.insert(agent_id.to_string()) {
        return Err(rate_limited("stop_agent", 1));
    }
    Ok(())
}

pub fn finish_stop(ctx: &PaseoRuntimeContext, agent_id: &str) {
    ctx.state
        .stops
        .lock()
        .expect("paseo stop lock")
        .remove(agent_id);
}

fn rate_limited(operation: &str, retry_after_seconds: u64) -> PaseoError {
    PaseoError::new(
        "PASEO_RATE_LIMITED",
        "Paseo operation is temporarily rate limited.",
        true,
        "rate_limit",
        serde_json::json!({"operation": operation, "retry_after_seconds": retry_after_seconds}),
    )
}

fn agent_id_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$").expect("agent id regex")
    })
}

fn host_address_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"^(?:[A-Za-z0-9.-]+|\[[0-9A-Fa-f:]+\]):[1-9][0-9]{0,4}$")
            .expect("host regex")
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::integrations::paseo::config::PaseoIntegrationConfig;

    #[test]
    fn disabled_policy_rejects_direct_call() {
        let context = PaseoRuntimeContext::disabled(PathBuf::from("."));
        assert_eq!(
            authorize(&context, "paseo_health").unwrap_err().code,
            "PASEO_DISABLED"
        );
    }

    #[test]
    fn agent_id_does_not_allow_flags() {
        assert!(validate_agent_id("--all").is_err());
        assert!(validate_agent_id("agent_123").is_ok());
    }

    #[test]
    fn host_supports_safe_remote_forms() {
        assert!(validate_host(Some("host.example:6767")).is_ok());
        assert!(validate_host(Some("host.example:65535")).is_ok());
        assert!(validate_host(Some("tcp://host.example:6767?ssl=true&password=safe")).is_ok());
        assert!(validate_host(Some("host;rm:6767")).is_err());
        assert!(validate_host(Some("host.example:65536")).is_err());
        let _ = PaseoIntegrationConfig::default();
    }

    #[test]
    fn concurrent_monitor_and_stop_are_deduplicated() {
        let mut config = PaseoIntegrationConfig {
            enabled: true,
            access_mode: crate::integrations::paseo::config::PaseoAccessMode::Control,
            ..PaseoIntegrationConfig::default()
        };
        config.monitor.enabled = true;
        let context = PaseoRuntimeContext::new("ws".into(), PathBuf::from("."), config, None);
        assert!(begin_monitor(&context).is_ok());
        assert_eq!(
            begin_monitor(&context).unwrap_err().code,
            "PASEO_RATE_LIMITED"
        );
        finish_monitor(&context);
        assert!(begin_stop(&context, "agent-1").is_ok());
        assert_eq!(
            begin_stop(&context, "agent-1").unwrap_err().code,
            "PASEO_RATE_LIMITED"
        );
        finish_stop(&context, "agent-1");
    }
}
