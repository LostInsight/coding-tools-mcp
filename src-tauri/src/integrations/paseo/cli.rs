use serde_json::{json, Value};

use super::binary::resolve_binary;
use super::client::PaseoClient;
use super::command::{PaseoCommandRunner, SystemPaseoCommandRunner};
use super::compat::{
    parse_inspected_permissions, parse_permission_listing, permission_warning,
    replace_agent_permissions,
};
use super::model::{
    ActivityEvent, ParsedPaseo, PaseoAgent, PaseoCapabilities, PaseoCommandOutput,
    PaseoCommandSpec, PaseoDaemonStatus, PaseoError, PaseoRuntimeContext, PermissionSummary,
};
use super::parser::{parse_activity, parse_agents};
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
        let launch = resolve_binary(&self.context.config.binary_path)?;
        let mut launch_args = launch.args;
        launch_args.extend(args);
        let output = self.runner.run(&PaseoCommandSpec {
            program: launch.program,
            args: launch_args,
            environment: launch.environment,
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
        if !is_observable_version(&version) {
            return Err(PaseoError::new(
                "PASEO_VERSION_UNSUPPORTED",
                "Installed Paseo CLI returned an unreadable version string.",
                false,
                "version",
                json!({"cli_version": bounded(&redact(&version), 40)}),
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
        let output = self
            .run(
                vec!["--no-color".into(), "status".into(), "--json".into()],
                "connect",
                Some(65_536),
            )
            .or_else(|_| {
                self.run(
                    vec![
                        "--no-color".into(),
                        "daemon".into(),
                        "status".into(),
                        "--json".into(),
                    ],
                    "connect",
                    Some(65_536),
                )
            })?;
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

    fn detect_capabilities(&self) -> Result<PaseoCapabilities, PaseoError> {
        let _ = self.checked_version()?;
        let mut capabilities = PaseoCapabilities::default();

        let mut list_args = vec![
            "--no-color".into(),
            "ls".into(),
            "-a".into(),
            "-g".into(),
            "--json".into(),
        ];
        list_args.extend(self.host_args());
        match self.run(list_args, "capability_list_agents", Some(65_536)) {
            Ok(output) => {
                capabilities.list_agents =
                    parse_agents(&output.stdout, output.stdout_truncated).is_ok();
            }
            Err(error) => capabilities
                .warnings
                .push(capability_warning("list_agents", &error)),
        }

        let mut permission_args = vec![
            "--no-color".into(),
            "permit".into(),
            "ls".into(),
            "--json".into(),
        ];
        permission_args.extend(self.host_args());
        match self.run(permission_args, "capability_permissions", Some(65_536)) {
            Ok(output) => {
                capabilities.permissions =
                    parse_permission_listing(&output.stdout, output.stdout_truncated).is_ok();
            }
            Err(error) => capabilities
                .warnings
                .push(capability_warning("permissions", &error)),
        }
        capabilities.json_output = capabilities.list_agents && capabilities.permissions;

        match self.run(
            vec!["--no-color".into(), "--help".into()],
            "capability_commands",
            Some(65_536),
        ) {
            Ok(output) => {
                capabilities.activity = help_has_command(&output.stdout, "logs");
                capabilities.permission_details = help_has_command(&output.stdout, "inspect");
                capabilities.send_prompt = help_has_command(&output.stdout, "send");
                capabilities.stop_agent = help_has_command(&output.stdout, "stop");
                capabilities.create_agent = help_has_command(&output.stdout, "run");
            }
            Err(error) => capabilities
                .warnings
                .push(capability_warning("command_surface", &error)),
        }

        match self.run(
            vec!["--no-color".into(), "permit".into(), "--help".into()],
            "capability_permission_control",
            Some(16_384),
        ) {
            Ok(output) => {
                capabilities.allow_permission = help_has_command(&output.stdout, "allow");
                capabilities.deny_permission = help_has_command(&output.stdout, "deny");
            }
            Err(error) => capabilities
                .warnings
                .push(capability_warning("permission_control", &error)),
        }
        Ok(capabilities)
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
        let mut parsed = parse_permission_listing(&output.stdout, output.stdout_truncated)?;
        let agent_ids = parsed
            .data
            .iter()
            .filter(|permission| !permission.control_safe)
            .filter_map(|permission| permission.agent_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for agent_id in agent_ids {
            let mut inspect_args = vec![
                "--no-color".into(),
                "inspect".into(),
                agent_id.clone(),
                "--json".into(),
            ];
            inspect_args.extend(self.host_args());
            match self
                .run(inspect_args, "permission_details", None)
                .and_then(|output| {
                    parse_inspected_permissions(&output.stdout, &agent_id, output.stdout_truncated)
                }) {
                Ok(inspected) => replace_agent_permissions(&mut parsed, &agent_id, inspected),
                Err(error) => parsed.warnings.push(permission_warning(&error, &agent_id)),
            }
        }
        Ok(parsed)
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

    fn allow_permission(&self, agent_id: &str, request_id: &str) -> Result<Value, PaseoError> {
        let _ = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "permit".into(),
            "allow".into(),
            agent_id.into(),
            request_id.into(),
            "--json".into(),
        ];
        args.extend(self.host_args());
        let output = self.run(args, "allow_permission", None)?;
        parse_json_output(
            &output,
            "allow_permission",
            "Paseo permission allow response",
        )
    }

    fn deny_permission(
        &self,
        agent_id: &str,
        request_id: &str,
        message: Option<&str>,
    ) -> Result<Value, PaseoError> {
        let _ = self.checked_version()?;
        let mut args = vec![
            "--no-color".into(),
            "permit".into(),
            "deny".into(),
            agent_id.into(),
            request_id.into(),
        ];
        if let Some(message) = message.filter(|value| !value.is_empty()) {
            args.extend(["--message".into(), message.into()]);
        }
        args.push("--json".into());
        args.extend(self.host_args());
        let output = self.run(args, "deny_permission", None)?;
        parse_json_output(&output, "deny_permission", "Paseo permission deny response")
    }

    fn create_agent(
        &self,
        prompt: &str,
        title: Option<&str>,
        provider: &str,
        cwd: &str,
    ) -> Result<Value, PaseoError> {
        let _ = self.checked_version()?;
        let mut workspace_args = vec![
            "--no-color".into(),
            "workspace".into(),
            "ls".into(),
            "--json".into(),
        ];
        workspace_args.extend(self.host_args());
        let workspace_id = self
            .run(workspace_args, "list_workspaces", Some(65_536))
            .ok()
            .and_then(|output| exact_workspace_for_cwd(&output.stdout, cwd));
        let mut args = vec![
            "--no-color".into(),
            "run".into(),
            "--background".into(),
            "--json".into(),
            "--mode".into(),
            "default".into(),
        ];
        if let Some(title) = title {
            args.extend(["--title".into(), title.into()]);
        }
        args.extend(["--provider".into(), provider.into()]);
        if let Some(workspace_id) = workspace_id {
            args.extend(["--workspace".into(), workspace_id]);
        } else {
            args.extend(["--cwd".into(), cwd.into()]);
        }
        args.extend(self.host_args());
        args.push("--".into());
        args.push(prompt.into());
        let output = self.run(args, "create_agent", None)?;
        parse_json_output(&output, "create_agent", "Paseo create agent response")
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

fn is_observable_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 40
        && !version.chars().any(char::is_control)
        && version.chars().any(|ch| ch.is_ascii_digit())
}

fn exact_workspace_for_cwd(raw: &str, expected_cwd: &str) -> Option<String> {
    let root = serde_json::from_str::<Value>(raw).ok()?;
    let workspaces = root
        .as_array()
        .or_else(|| root.get("workspaces").and_then(Value::as_array))?;
    let expected = normalized_cwd(expected_cwd);
    let mut matches = workspaces.iter().filter_map(|workspace| {
        let cwd = workspace
            .get("cwd")
            .or_else(|| workspace.get("path"))
            .and_then(Value::as_str)?;
        if normalized_cwd(cwd) != expected {
            return None;
        }
        workspace
            .get("workspaceId")
            .or_else(|| workspace.get("workspace_id"))
            .or_else(|| workspace.get("id"))
            .and_then(Value::as_str)
            .filter(|id| {
                !id.is_empty()
                    && id.len() <= 4_096
                    && !id.starts_with('-')
                    && !id.chars().any(char::is_control)
            })
            .map(str::to_string)
    });
    let first = matches.next()?;
    matches.all(|candidate| candidate == first).then_some(first)
}

fn normalized_cwd(value: &str) -> String {
    value
        .trim()
        .strip_prefix(r"\\?\")
        .unwrap_or(value.trim())
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn help_has_command(help: &str, command: &str) -> bool {
    help.lines().any(|line| {
        line.trim_start()
            .split_whitespace()
            .next()
            .is_some_and(|token| token == command)
    })
}

fn capability_warning(capability: &str, error: &PaseoError) -> Value {
    json!({
        "code": "PASEO_CAPABILITY_UNAVAILABLE",
        "message": "A Paseo capability probe did not succeed.",
        "retryable": error.retryable,
        "details": {"capability": capability, "cause": error.code}
    })
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
    let lower = format!(
        "{}\n{}",
        stderr.to_ascii_lowercase(),
        redact(&output.stdout).to_ascii_lowercase()
    );
    let (code, message, retryable) =
        if lower.contains("unauthorized") || lower.contains("authentication") {
            ("PASEO_AUTH_FAILED", "Paseo authentication failed.", false)
        } else if lower.contains("not found")
            && (lower.contains("permission") || lower.contains("request"))
        {
            (
                "PASEO_PERMISSION_NOT_FOUND",
                "The Paseo permission request was not found.",
                false,
            )
        } else if lower.contains("not found") && lower.contains("agent") {
            ("PASEO_AGENT_NOT_FOUND", "Paseo agent was not found.", false)
        } else if lower.contains("connect")
            || lower.contains("daemon")
            || lower.contains("econnrefused")
            || lower.contains("bad gateway")
            || lower.contains("service unavailable")
            || lower.contains("gateway timeout")
            || lower.contains("http 502")
            || lower.contains("http 503")
            || lower.contains("http 504")
            || lower.contains("status 502")
            || lower.contains("status 503")
            || lower.contains("status 504")
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
    fn control_commands_use_exact_bounded_cli_shapes() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp
            .path()
            .join(if cfg!(windows) { "paseo.exe" } else { "paseo" });
        std::fs::write(&binary, "fixture").unwrap();
        let config = PaseoIntegrationConfig {
            enabled: true,
            access_mode: PaseoAccessMode::Control,
            binary_path: binary.display().to_string(),
            ..PaseoIntegrationConfig::default()
        };
        let context =
            PaseoRuntimeContext::new("ws-control".into(), temp.path().to_path_buf(), config, None);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let runner = FakeRunner {
            calls: calls.clone(),
            outputs: Arc::new(Mutex::new(vec![
                output("0.2.5"),
                output(r#"{"ok":true}"#),
                output(r#"{"ok":true}"#),
                output("[]"),
                output(r#"{"id":"agent-new"}"#),
            ])),
        };
        let client = PaseoCliClient::new(context, runner);

        client.allow_permission("agent-1", "req-1").unwrap();
        client
            .deny_permission("agent-1", "req-2", Some("not approved"))
            .unwrap();
        client
            .create_agent(
                "run tests && report",
                Some("smoke"),
                "codex/gpt-5.6-luna",
                temp.path().to_str().unwrap(),
            )
            .unwrap();

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 5);
        let allow = &calls[1].args;
        assert!(allow
            .windows(3)
            .any(|args| args == ["allow", "agent-1", "req-1"]));
        assert!(!allow.iter().any(|arg| arg == "--all"));

        let deny = &calls[2].args;
        assert!(deny
            .windows(3)
            .any(|args| args == ["deny", "agent-1", "req-2"]));
        assert!(deny
            .windows(2)
            .any(|args| args == ["--message", "not approved"]));
        assert!(!deny
            .iter()
            .any(|arg| arg == "--all" || arg == "--interrupt"));

        let create = &calls[4].args;
        assert!(create.iter().any(|arg| arg == "--background"));
        assert!(create.iter().any(|arg| arg == "--json"));
        assert!(create.windows(2).any(|args| args == ["--mode", "default"]));
        assert_eq!(
            create.last().map(String::as_str),
            Some("run tests && report")
        );
        assert_eq!(create.get(create.len() - 2).map(String::as_str), Some("--"));
        assert!(!create.iter().any(|arg| arg == "cmd" || arg == "sh"));
    }

    #[test]
    fn workspace_selection_requires_one_exact_cwd_identity() {
        let raw = r#"[
          {"workspaceId":"wks-exact","cwd":"E:\\Coding\\repo"},
          {"workspaceId":"wks-other","cwd":"E:\\Coding\\other"}
        ]"#;
        assert_eq!(
            exact_workspace_for_cwd(raw, r"\\?\E:\Coding\repo").as_deref(),
            Some("wks-exact")
        );

        let ambiguous = r#"[
          {"workspaceId":"wks-one","cwd":"E:\\Coding\\repo"},
          {"workspaceId":"wks-two","cwd":"E:\\Coding\\repo"}
        ]"#;
        assert_eq!(exact_workspace_for_cwd(ambiguous, r"E:\Coding\repo"), None);
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
    fn version_is_diagnostic_not_a_minor_version_gate() {
        assert!(is_observable_version("0.2.5"));
        assert!(is_observable_version("0.3.1"));
        assert!(is_observable_version("0.4.0-beta.1"));
        assert!(is_observable_version("1.0.0"));
        assert!(!is_observable_version(""));
        assert!(!is_observable_version("version-unknown"));
    }

    #[test]
    fn permission_listing_is_enriched_from_exact_agent_inspection() {
        let temp = tempfile::tempdir().unwrap();
        let binary = temp
            .path()
            .join(if cfg!(windows) { "paseo.exe" } else { "paseo" });
        std::fs::write(&binary, "fixture").unwrap();
        let config = PaseoIntegrationConfig {
            enabled: true,
            access_mode: PaseoAccessMode::Control,
            binary_path: binary.display().to_string(),
            ..PaseoIntegrationConfig::default()
        };
        let context = PaseoRuntimeContext::new("ws".into(), temp.path().into(), config, None);
        let calls = Arc::new(Mutex::new(Vec::new()));
        let client = PaseoCliClient::new(
            context,
            FakeRunner {
                calls: calls.clone(),
                outputs: Arc::new(Mutex::new(vec![
                    output("0.3.1"),
                    output(include_str!(
                        "../../../tests/fixtures/paseo/permissions-v0.3.1-list.json"
                    )),
                    output(include_str!(
                        "../../../tests/fixtures/paseo/permissions-v0.3.1-inspect.json"
                    )),
                ])),
            },
        );

        let parsed = client.list_pending_permissions().unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(
            parsed.data[0].request_id.as_deref(),
            Some("permission-174b288d-cc76-499e-bf68-9fb7b968f4e9")
        );
        assert!(parsed.data[0].control_safe);
        assert_eq!(parsed.data[0].source, "agent_inspect");
        assert!(parsed.warnings.is_empty());
        let calls = calls.lock().unwrap();
        assert!(calls[2]
            .args
            .windows(2)
            .any(|args| args == ["inspect", "37c6d4c7-8092-4939-8ef4-178b63879ca2"]));
    }

    #[test]
    fn capabilities_are_probed_from_safe_reads_and_command_surfaces() {
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
                    output("0.3.1"),
                    output("[]"),
                    output("[]"),
                    output("  logs\n  inspect\n  send\n  stop\n  run\n"),
                    output("  ls\n  allow\n  deny\n"),
                ])),
            },
        );

        let capabilities = client.detect_capabilities().unwrap();
        assert!(capabilities.list_agents);
        assert!(capabilities.permissions);
        assert!(capabilities.permission_details);
        assert!(capabilities.allow_permission);
        assert!(capabilities.deny_permission);
        assert!(capabilities.create_agent);
        assert!(capabilities.json_output);
    }

    #[test]
    fn command_failures_have_specific_stable_codes() {
        let failure = |stderr: &str| PaseoCommandOutput {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code: Some(1),
            duration_ms: 12,
            stdout_truncated: false,
            stderr_truncated: false,
        };

        assert_eq!(
            map_command_failure(&failure("502 Bad Gateway"), "activity", None).code,
            "PASEO_DAEMON_UNREACHABLE"
        );
        assert_eq!(
            map_command_failure(
                &failure("permission request req-1 not found"),
                "allow_permission",
                None,
            )
            .code,
            "PASEO_PERMISSION_NOT_FOUND"
        );
        assert_eq!(
            map_command_failure(&failure("agent agent-1 not found"), "activity", None).code,
            "PASEO_AGENT_NOT_FOUND"
        );
    }
}
