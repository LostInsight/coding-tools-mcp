use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::binary::resolve_binary;
use super::command::{PaseoCommandRunner, SystemPaseoCommandRunner};
use super::config::{PaseoAccessMode, PaseoIntegrationConfig};
use super::model::{PaseoCommandSpec, PaseoRuntimeContext};
use super::tools;

const POLL_INTERVAL: Duration = Duration::from_secs(1);
const E2E_TIMEOUT: Duration = Duration::from_secs(120);

struct LiveCleanup {
    agent_ids: Vec<String>,
    files: Vec<PathBuf>,
}

impl LiveCleanup {
    fn new(files: Vec<PathBuf>) -> Self {
        Self {
            agent_ids: Vec::new(),
            files,
        }
    }

    fn track_agent(&mut self, agent_id: String) {
        self.agent_ids.push(agent_id);
    }

    fn archive_agent(&mut self, agent_id: &str) {
        archive_agent(agent_id);
        self.agent_ids.retain(|current| current != agent_id);
    }
}

impl Drop for LiveCleanup {
    fn drop(&mut self) {
        for agent_id in self.agent_ids.drain(..) {
            archive_agent(&agent_id);
        }
        for path in &self.files {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn archive_agent(agent_id: &str) {
    let _ = run_paseo_cleanup(["stop", agent_id, "--json"]);
    let _ = run_paseo_cleanup(["archive", agent_id, "--force", "--json"]);
}

fn archive_agent_with_title(title: &str) {
    let Some(output) = run_paseo_cleanup(["ls", "--json"]) else {
        return;
    };
    let Ok(agents) = serde_json::from_str::<Vec<Value>>(&output.stdout) else {
        return;
    };
    for agent_id in agents.iter().filter_map(|agent| {
        (agent["name"].as_str() == Some(title))
            .then(|| agent["id"].as_str())
            .flatten()
    }) {
        archive_agent(agent_id);
    }
}

fn run_paseo_cleanup<const N: usize>(args: [&str; N]) -> Option<super::model::PaseoCommandOutput> {
    let launch = resolve_binary("").ok()?;
    let mut launch_args = launch.args;
    launch_args.extend(args.into_iter().map(str::to_string));
    SystemPaseoCommandRunner
        .run(&PaseoCommandSpec {
            program: launch.program,
            args: launch_args,
            environment: launch.environment,
            timeout_ms: 30_000,
            max_output_bytes: 262_144,
        })
        .ok()
}

fn control_context(workspace: &Path, unique: &str) -> PaseoRuntimeContext {
    let config = PaseoIntegrationConfig {
        enabled: true,
        access_mode: PaseoAccessMode::Control,
        command_timeout_ms: 30_000,
        ..PaseoIntegrationConfig::default()
    };
    PaseoRuntimeContext::new(
        format!("paseo-live-e2e-{unique}"),
        workspace.to_path_buf(),
        config,
        None,
    )
}

fn require_ok(value: Value, action: &str) -> Value {
    assert_eq!(value["ok"], true, "{action} failed: {value}");
    value
}

fn create_agent(ctx: &PaseoRuntimeContext, title: &str, prompt: &str) -> String {
    let response = require_ok(
        tools::call(
            ctx,
            "paseo_create_agent",
            &json!({
                "provider": "claude",
                "title": title,
                "cwd": ctx.workspace_path,
                "prompt": prompt,
                "reason": "Paseo wrapper permission compatibility E2E",
                "confirm": true
            }),
        ),
        "create agent",
    );
    response["agent_id"]
        .as_str()
        .filter(|value| !value.is_empty())
        .expect("create agent response must contain a full agent_id")
        .to_string()
}

fn wait_for_permission(ctx: &PaseoRuntimeContext, agent_id: &str) -> Value {
    let deadline = Instant::now() + E2E_TIMEOUT;
    loop {
        let response = require_ok(
            tools::call(
                ctx,
                "paseo_list_pending_permissions",
                &json!({"agent_id": agent_id}),
            ),
            "list pending permissions",
        );
        if let Some(permission) = response["permissions"]
            .as_array()
            .and_then(|permissions| permissions.first())
        {
            assert_eq!(permission["agent_id"], agent_id);
            assert_eq!(permission["control_safe"], true);
            assert_eq!(permission["source"], "agent_inspect");
            assert_ne!(permission["request_id"], "permissi");
            assert_eq!(permission["tool"], "Write");
            return permission.clone();
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a permission request for {agent_id}"
        );
        thread::sleep(POLL_INTERVAL);
    }
}

fn assert_activity_readable(ctx: &PaseoRuntimeContext, agent_id: &str) {
    let response = require_ok(
        tools::call(
            ctx,
            "paseo_get_agent_activity",
            &json!({"agent_id": agent_id, "tail": 30}),
        ),
        "read activity",
    );
    assert!(response["events"].is_array());
    assert!(matches!(
        response["source_format"].as_str(),
        Some("json" | "text_fallback")
    ));
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + E2E_TIMEOUT;
    while !path.exists() {
        assert!(Instant::now() < deadline, "timed out waiting for {path:?}");
        thread::sleep(POLL_INTERVAL);
    }
}

fn assert_permission_gone(ctx: &PaseoRuntimeContext, agent_id: &str) {
    let deadline = Instant::now() + E2E_TIMEOUT;
    loop {
        let response = require_ok(
            tools::call(
                ctx,
                "paseo_list_pending_permissions",
                &json!({"agent_id": agent_id}),
            ),
            "confirm permission resolved",
        );
        if response["count"] == 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "permission remained pending for {agent_id}"
        );
        thread::sleep(POLL_INTERVAL);
    }
}

fn stop_agent(ctx: &PaseoRuntimeContext, agent_id: &str) {
    require_ok(
        tools::call(
            ctx,
            "paseo_stop_agent",
            &json!({
                "agent_id": agent_id,
                "reason": "Paseo wrapper permission compatibility E2E cleanup",
                "confirm": true
            }),
        ),
        "stop agent",
    );
}

#[test]
#[ignore = "requires a live Paseo daemon and PASEO_LIVE_PERMISSION_E2E=1"]
fn wrapper_health_reports_detected_capabilities() {
    assert_eq!(
        std::env::var("PASEO_LIVE_PERMISSION_E2E").as_deref(),
        Ok("1"),
        "set PASEO_LIVE_PERMISSION_E2E=1 to authorize the live test"
    );
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a repository parent")
        .to_path_buf();
    let ctx = control_context(&workspace, "health");
    let health = require_ok(
        tools::call(&ctx, "paseo_health", &json!({"refresh": true})),
        "health",
    );
    assert_eq!(health["cli_version"], "0.3.1");
    assert_eq!(health["daemon_version"], "0.3.1");
    assert_eq!(health["daemon_reachable"], true);
    for capability in [
        "list_agents",
        "activity",
        "permissions",
        "permission_details",
        "json_output",
        "send_prompt",
        "stop_agent",
        "allow_permission",
        "deny_permission",
        "create_agent",
    ] {
        assert_eq!(
            health["capabilities"][capability], true,
            "capability {capability} was not detected: {health}"
        );
    }
}

#[test]
#[ignore = "requires a live Paseo daemon and PASEO_LIVE_PERMISSION_E2E=1"]
fn runner_captures_complete_run_json() {
    assert_eq!(
        std::env::var("PASEO_LIVE_PERMISSION_E2E").as_deref(),
        Ok("1"),
        "set PASEO_LIVE_PERMISSION_E2E=1 to authorize the live test"
    );
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let title = format!("Paseo wrapper runner probe {unique}");
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a repository parent")
        .to_path_buf();
    let launch = resolve_binary("").unwrap();
    let mut args = launch.args;
    args.extend([
        "--no-color".into(),
        "run".into(),
        "--background".into(),
        "--json".into(),
        "--title".into(),
        title.clone(),
        "--provider".into(),
        "claude".into(),
        "--cwd".into(),
        workspace.display().to_string(),
        "--".into(),
        "Do not modify files. Reply with done.".into(),
    ]);
    let output = SystemPaseoCommandRunner
        .run(&PaseoCommandSpec {
            program: launch.program,
            args,
            environment: launch.environment,
            timeout_ms: 30_000,
            max_output_bytes: 262_144,
        })
        .unwrap();
    eprintln!("runner stdout={:?}", output.stdout);
    eprintln!("runner stderr={:?}", output.stderr);
    let parsed = serde_json::from_str::<Value>(&output.stdout);
    let agent_id = parsed
        .as_ref()
        .ok()
        .and_then(|value| value["agentId"].as_str())
        .map(str::to_string);
    if let Some(agent_id) = agent_id.as_deref() {
        archive_agent(agent_id);
    } else {
        archive_agent_with_title(&title);
    }
    assert_eq!(output.exit_code, Some(0));
    assert!(!output.stdout_truncated);
    assert!(
        parsed.is_ok(),
        "runner captured incomplete JSON: {parsed:?}"
    );
}

#[test]
#[ignore = "requires a live Paseo daemon and PASEO_LIVE_PERMISSION_E2E=1"]
fn wrapper_allow_and_deny_permissions() {
    assert_eq!(
        std::env::var("PASEO_LIVE_PERMISSION_E2E").as_deref(),
        Ok("1"),
        "set PASEO_LIVE_PERMISSION_E2E=1 to authorize the live test"
    );

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri must have a repository parent")
        .to_path_buf();
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let allow_path = workspace.join(format!("__paseo_allow_e2e_{unique}.txt"));
    let deny_path = workspace.join(format!("__paseo_deny_e2e_{unique}.txt"));
    let expected = format!("paseo-wrapper-allow-{unique}");
    let mut cleanup = LiveCleanup::new(vec![allow_path.clone(), deny_path.clone()]);
    let ctx = control_context(&workspace, &unique);

    assert!(!allow_path.exists());
    let allow_agent = create_agent(
        &ctx,
        &format!("Paseo wrapper allow E2E {unique}"),
        &format!(
            "Use the Write tool, not a shell, to create exactly one file at {} containing exactly: {}. Do not touch any other file. Wait for permission if asked, then finish.",
            allow_path.display(),
            expected
        ),
    );
    cleanup.track_agent(allow_agent.clone());
    let allow_permission = wait_for_permission(&ctx, &allow_agent);
    assert_activity_readable(&ctx, &allow_agent);
    let allow_request_id = allow_permission["request_id"]
        .as_str()
        .expect("allow request must have a full request_id");
    require_ok(
        tools::call(
            &ctx,
            "paseo_allow_permission",
            &json!({
                "agent_id": allow_agent,
                "request_id": allow_request_id,
                "reason": "allow harmless unique E2E file write",
                "confirm": true
            }),
        ),
        "allow permission",
    );
    assert_permission_gone(&ctx, &allow_agent);
    wait_for_file(&allow_path);
    assert_eq!(
        std::fs::read_to_string(&allow_path).unwrap().trim(),
        expected
    );
    stop_agent(&ctx, &allow_agent);
    cleanup.archive_agent(&allow_agent);

    assert!(!deny_path.exists());
    let deny_agent = create_agent(
        &ctx,
        &format!("Paseo wrapper deny E2E {unique}"),
        &format!(
            "Use the Write tool, not a shell, to create exactly one file at {} containing exactly: denied. Do not touch any other file. Wait for permission if asked, then finish.",
            deny_path.display()
        ),
    );
    cleanup.track_agent(deny_agent.clone());
    let deny_permission = wait_for_permission(&ctx, &deny_agent);
    assert_activity_readable(&ctx, &deny_agent);
    let deny_request_id = deny_permission["request_id"]
        .as_str()
        .expect("deny request must have a full request_id");
    require_ok(
        tools::call(
            &ctx,
            "paseo_deny_permission",
            &json!({
                "agent_id": deny_agent,
                "request_id": deny_request_id,
                "reason": "deny harmless unique E2E file write",
                "confirm": true
            }),
        ),
        "deny permission",
    );
    assert_permission_gone(&ctx, &deny_agent);
    thread::sleep(Duration::from_secs(2));
    assert!(!deny_path.exists());
    stop_agent(&ctx, &deny_agent);
    cleanup.archive_agent(&deny_agent);

    std::fs::remove_file(&allow_path).unwrap();
    assert!(!allow_path.exists());
    assert!(!deny_path.exists());
}
