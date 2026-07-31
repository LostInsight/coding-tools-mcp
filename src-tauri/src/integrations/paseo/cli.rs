use serde_json::{json, Value};

use super::binary::resolve_binary;
use super::client::PaseoClient;
use super::command::{PaseoCommandRunner, SystemPaseoCommandRunner};
use super::model::{
    ActivityEvent, ParsedPaseo, PaseoAgent, PaseoCommandOutput, PaseoCommandSpec,
    PaseoDaemonStatus, PaseoError, PaseoRuntimeContext, PermissionSummary,
};
use super::parser::{parse_activity, parse_agents, parse_permissions};
use super::policy::validate_host;
use super::redaction::{bounded, redact, redact_host};

pub struct PaseoCliClient<R: PaseoCommandRunner> {
    context: PaseoRuntimeContext,
    runner: R,
}

impl PaseoCliClient<SystemPaseoCommandRunner> {
    pub fn production(context: PaseoRuntimeContext) -> Self {
        Self::new(context, SystemPaseoCommandRunner)
    }
}

impl<R: PaseoCommandRunner> PaseoCliClient<R> {
    pub fn new(context: PaseoRuntimeContext, runner: R) -> Self {
        Self { context, runner }
    }

    fn run(
        &self,
        args: Vec<String>,
        stage: &'static str,
        max_bytes: Option<usize>,
    ) -> Result<PaseoCommandOutput, PaseoError> {
        self.context.config.validate()?;
        if self.context.config.host_configured && self.context.host.is_none() {
            return Err(PaseoError::new(
                "PASEO_HOST_INVALID",
                "Configured Paseo host secret is unavailable.",
                false,
                "host",
                json!({"connection_type": "remote", "host": "<redacted>"}),
            ));
        }
        validate_host(self.context.host.as_deref())?;
        let program = resolve_binary(&self.context.config.binary_path)?;
        let output = self.runner.run(&PaseoCommandSpec {
            program,
            args,
            timeout_ms: self.context.config.command_timeout_ms,
            max_output_bytes: max_bytes
                .unwrap_or(self.context.config.max_output_bytes)
                .min(self.context.config.max_output_bytes),
        })?;
        if output.exit_code != Some(0) {
            return Err(map_command_failure(
                &output,
                stage,
                self.context.host.as_deref(),
            ));
        }
        Ok(output)
    }

    fn host_args(&self) -> Vec<String> {
        self.context
            .host
            .as_ref()
            .map(|host| vec!["--host".into(), host.clone()])
            .unwrap_or_default()
    }

    fn checked_version(&self) -> Result<String, PaseoError> {
        if let Some(version) = self
            .context
            .state
            .cli_version
            .lock()
            .expect("paseo version lock")
            .clone()
        {
            return Ok(version);
        }
        let output = self.run(vec!["--version".into()], "version", Some(4_096))?;
        ensure_complete_stdout(&output, "version")?;
        let version = output.stdout.trim().trim_start_matches('v').to_string();
        if version.len() > 40 || !is_supported_version(&version) {
            return Err(PaseoError::new(
                "PASEO_VERSION_UNSUPPORTED",
                "Installed Paseo CLI version is not supported by this integration.",
                false,
                "version",
                json!({"cli_version": bounded(&redact(&version), 40), "supported": "0.2.x"}),
            ));
        }
        *self
            .context
            .state
            .cli_version
            .lock()
            .expect("paseo version lock") = Some(version.clone());
        Ok(version)
    }
}

impl<R: PaseoCommandRunner> PaseoClient for PaseoCliClient<R> {
    fn cli_version(&self) -> Result<String, PaseoError> {
        self.checked_version()
    }

    fn daemon_status(&self) -> Result<PaseoDaemonStatus, PaseoError> {
        let cli_version = self.checked_version()?;
        if self.context.host.is_some() {
            let mut args = vec!["--no-color".into(), "ls".into(), "--json".into()];
            args.extend(self.host_args());
            self.run(args, "connect", Some(16_384))?;
            return Ok(PaseoDaemonStatus {
                cli_version,
                daemon_version: None,
                reachable: true,
                connection_type: "remote".into(),
                raw_status: None,
            });
        }
        let output = self.run(
            vec![
                "--no-color".into(),
                "daemon".into(),
                "status".into(),
                "--json".into(),
            ],
            "connect",
            Some(65_536),
        )?;
        let value = parse_json_output(&output, "connect", "Paseo daemon status")?;
        let reachable = value
            .get("connectedDaemon")
            .and_then(Value::as_str)
            .map(status_is_reachable)
            .unwrap_or_else(|| {
                value
                    .get("localDaemon")
                    .and_then(Value::as_str)
                    .is_some_and(status_is_reachable)
            });
        Ok(PaseoDaemonStatus {
            cli_version,
            daemon_version: value
                .get("daemonVersion")
                .and_then(Value::as_str)
                .map(|value| bounded(&redact(value), 40)),
            reachable,
            connection_type: "local".into(),
            raw_status: value
                .get("connectedDaemon")
                .or_else(|| value.get("localDaemon"))
                .and_then(Value::as_str)
                .map(|value| bounded(&redact(value), 120)),
        })
    }

    fn list_agents(&self) -> Result<ParsedPaseo<Vec<PaseoAgent>>, PaseoError> {
        let _ = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "ls".into(),
            "-a".into(),
            "-g".into(),
            "--json".into(),
        ];
        args.extend(self.host_args());
        let output = self.run(args, "list_agents", None)?;
        parse_agents(&output.stdout, output.stdout_truncated)
    }

    fn get_activity(
        &self,
        agent_id: &str,
        tail: u32,
        filter: &str,
        max_bytes: usize,
    ) -> Result<ParsedPaseo<Vec<ActivityEvent>>, PaseoError> {
        let version = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "logs".into(),
            agent_id.into(),
            "--tail".into(),
            tail.to_string(),
        ];
        let cli_filter = match filter {
            "messages" => Some("text"),
            "tools" => Some("tools"),
            "errors" => Some("errors"),
            _ => None,
        };
        if let Some(filter) = cli_filter {
            args.extend(["--filter".into(), filter.into()]);
        }
        args.extend(self.host_args());
        let output = self.run(args, "activity", Some(max_bytes))?;
        parse_activity(&output.stdout, &version, output.stdout_truncated)
    }

    fn list_pending_permissions(&self) -> Result<ParsedPaseo<Vec<PermissionSummary>>, PaseoError> {
        let _ = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "permit".into(),
            "ls".into(),
            "--json".into(),
        ];
        args.extend(self.host_args());
        let output = self.run(args, "permissions", None)?;
        parse_permissions(&output.stdout, output.stdout_truncated)
    }

    fn send_prompt(&self, agent_id: &str, prompt: &str) -> Result<Value, PaseoError> {
        let _ = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "send".into(),
            agent_id.into(),
            "--no-wait".into(),
            "--json".into(),
        ];
        args.extend(self.host_args());
        args.push("--".into());
        args.push(prompt.into());
        let output = self.run(args, "send", None)?;
        parse_json_output(&output, "send", "Paseo send response")
    }

    fn stop_agent(&self, agent_id: &str) -> Result<Value, PaseoError> {
        let _ = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "stop".into(),
            agent_id.into(),
            "--json".into(),
        ];
        args.extend(self.host_args());
        let output = self.run(args, "stop", None)?;
        parse_json_output(&output, "stop", "Paseo stop response")
    }
}

fn ensure_complete_stdout(
    output: &PaseoCommandOutput,
    stage: &'static str,
) -> Result<(), PaseoError> {
    if output.stdout_truncated {
        return Err(PaseoError::new(
            "PASEO_OUTPUT_LIMIT",
            "Paseo output exceeded the configured limit.",
            true,
            stage,
            json!({"stdout_truncated": true}),
        ));
    }
    Ok(())
}

fn parse_json_output(
    output: &PaseoCommandOutput,
    stage: &'static str,
    label: &'static str,
) -> Result<Value, PaseoError> {
    ensure_complete_stdout(output, stage)?;
    serde_json::from_str(&output.stdout).map_err(|error| {
        PaseoError::new(
            "PASEO_PARSE_ERROR",
            format!("{label} was not valid JSON."),
            false,
            stage,
            json!({"reason": bounded(&error.to_string(), 240)}),
        )
    })
}

fn is_supported_version(version: &str) -> bool {
    let mut parts = version.split('.');
    matches!(parts.next(), Some("0"))
        && matches!(parts.next(), Some("2"))
        && parts
            .next()
            .is_some_and(|patch| !patch.is_empty() && patch.chars().all(|ch| ch.is_ascii_digit()))
        && parts.next().is_none()
}

fn status_is_reachable(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "reachable" | "running" | "connected" | "ready"
    )
}

fn map_command_failure(
    output: &PaseoCommandOutput,
    stage: &'static str,
    host: Option<&str>,
) -> PaseoError {
    let stderr = redact(&output.stderr);
    let lower = stderr.to_ascii_lowercase();
    let (code, message, retryable) =
        if lower.contains("unauthorized") || lower.contains("authentication") {
            ("PASEO_AUTH_FAILED", "Paseo authentication failed.", false)
        } else if lower.contains("not found") && lower.contains("agent") {
            ("PASEO_AGENT_NOT_FOUND", "Paseo agent was not found.", false)
        } else if lower.contains("connect")
            || lower.contains("daemon")
            || lower.contains("econnrefused")
        {
            (
                "PASEO_DAEMON_UNREACHABLE",
                "Unable to reach the configured Paseo daemon.",
                true,
            )
        } else {
            ("PASEO_COMMAND_FAILED", "Paseo command failed.", true)
        };
    PaseoError::new(
        code,
        message,
        retryable,
        stage,
        json!({
            "connection_type": if host.is_some() { "remote" } else { "local" },
            "host": redact_host(host),
            "exit_code": output.exit_code,
            "stderr_summary": bounded(&stderr, 1_000),
            "stderr_truncated": output.stderr_truncated,
            "duration_ms": output.duration_ms
        }),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::integrations::paseo::command::PaseoCommandRunner;
    use crate::integrations::paseo::config::{PaseoAccessMode, PaseoIntegrationConfig};

    #[derive(Clone)]
    struct FakeRunner {
        calls: Arc<Mutex<Vec<PaseoCommandSpec>>>,
        outputs: Arc<Mutex<Vec<PaseoCommandOutput>>>,
    }

    impl PaseoCommandRunner for FakeRunner {
        fn run(&self, spec: &PaseoCommandSpec) -> Result<PaseoCommandOutput, PaseoError> {
            self.calls.lock().unwrap().push(spec.clone());
            Ok(self.outputs.lock().unwrap().remove(0))
        }
    }

    fn output(stdout: &str) -> PaseoCommandOutput {
        PaseoCommandOutput {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 1,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn prompt_is_one_argument_even_with_shell_characters() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp
            .path()
            .join(if cfg!(windows) { "paseo.exe" } else { "paseo" });
        std::fs::write(&binary, "fixture").unwrap();
        let config = PaseoIntegrationConfig {
            enabled: true,
            access_mode: PaseoAccessMode::Assist,
            binary_path: binary.display().to_string(),
            ..PaseoIntegrationConfig::default()
        };
        let context = PaseoRuntimeContext::new("ws".into(), temp.path().into(), config, None);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            calls: calls.clone(),
            outputs: Arc::new(Mutex::new(vec![
                output("0.2.2"),
                output(r#"{"queued":true}"#),
            ])),
        };
        let client = PaseoCliClient::new(context, runner);
        let prompt = "--help\n\"quoted\" `tick` $() & |";
        client.send_prompt("agent-1", prompt).unwrap();
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].args.last().map(String::as_str), Some(prompt));
        assert_eq!(
            calls[1]
                .args
                .get(calls[1].args.len() - 2)
                .map(String::as_str),
            Some("--")
        );
        assert!(!calls[1].args.iter().any(|arg| arg == "cmd" || arg == "sh"));
    }

    #[test]
    fn configured_remote_host_never_falls_back_to_local() {
        let temp = tempfile::tempdir().unwrap();
        let config = PaseoIntegrationConfig {
            enabled: true,
            host_configured: true,
            ..PaseoIntegrationConfig::default()
        };
        let context = PaseoRuntimeContext::new("ws".into(), temp.path().into(), config, None);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = PaseoCliClient::new(
            context,
            FakeRunner {
                calls: calls.clone(),
                outputs: Arc::new(Mutex::new(Vec::new())),
            },
        );
        assert_eq!(client.cli_version().unwrap_err().code, "PASEO_HOST_INVALID");
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn local_daemon_status_uses_structured_reachability() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp
            .path()
            .join(if cfg!(windows) { "paseo.exe" } else { "paseo" });
        std::fs::write(&binary, "fixture").unwrap();
        let config = PaseoIntegrationConfig {
            enabled: true,
            binary_path: binary.display().to_string(),
            ..PaseoIntegrationConfig::default()
        };
        let context = PaseoRuntimeContext::new("ws".into(), temp.path().into(), config, None);
        let client = PaseoCliClient::new(
            context,
            FakeRunner {
                calls: Arc::new(Mutex::new(Vec::new())),
                outputs: Arc::new(Mutex::new(vec![
                    output("0.2.2"),
                    output(r#"{"localDaemon":"running","connectedDaemon":"unreachable"}"#),
                ])),
            },
        );
        assert!(!client.daemon_status().unwrap().reachable);
    }

    #[test]
    fn structured_output_rejects_truncation_and_non_json() {
        let mut truncated = output("{}");
        truncated.stdout_truncated = true;
        assert_eq!(
            parse_json_output(&truncated, "send", "response")
                .unwrap_err()
                .code,
            "PASEO_OUTPUT_LIMIT"
        );
        assert_eq!(
            parse_json_output(&output("not-json"), "stop", "response")
                .unwrap_err()
                .code,
            "PASEO_PARSE_ERROR"
        );
    }

    #[test]
    fn supported_version_requires_a_numeric_patch_component() {
        assert!(is_supported_version("0.2.2"));
        assert!(is_supported_version("0.2.30"));
        assert!(!is_supported_version("0.2.secret"));
        assert!(!is_supported_version("0.2.2.extra"));
    }
}
