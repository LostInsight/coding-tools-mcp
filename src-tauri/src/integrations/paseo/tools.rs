use std::time::Duration;

use serde_json::{json, Value};

use super::args::{
    bool_arg, bounded_string_arg, required_string, string_arg, string_array, u32_arg, usize_arg,
};
use super::cli::PaseoCliClient;
use super::client::PaseoClient;
use super::diagnostics::{diagnose, DiagnosticInput};
use super::model::{PaseoAgent, PaseoDiagnosis, PaseoError, PaseoRuntimeContext};
use super::monitor;
use super::monitor_store::PaseoMonitorStore;
use super::monitor_tool;
use super::{policy, redaction};

pub fn call(ctx: &PaseoRuntimeContext, name: &str, args: &Value) -> Value {
    let result = policy::authorize(ctx, name).and_then(|_| {
        let client = PaseoCliClient::production(ctx.clone());
        match name {
            "paseo_health" => health(ctx, &client, args),
            "paseo_list_agents" => list_agents(ctx, &client, args, true),
            "paseo_get_agent_activity" => activity(ctx, &client, args),
            "paseo_list_pending_permissions" => permissions(&client, args),
            "paseo_diagnose_agent" => diagnose_agent(ctx, &client, args),
            "paseo_monitor_snapshot" => monitor_tool::monitor_snapshot(ctx, &client, args),
            "paseo_send_agent_prompt" => send_prompt(ctx, &client, args),
            "paseo_stop_agent" => stop_agent(ctx, &client, args),
            _ => Err(PaseoError::argument("unknown Paseo tool")),
        }
    });
    result.unwrap_or_else(error_value)
}

pub fn test_health(ctx: &PaseoRuntimeContext, refresh: bool) -> Value {
    let result = policy::authorize(ctx, "paseo_health").and_then(|_| {
        let client = PaseoCliClient::production(ctx.clone());
        health(ctx, &client, &json!({"refresh": refresh}))
    });
    result.unwrap_or_else(error_value)
}

fn health<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    let refresh = bool_arg(args, "refresh", false);
    if !refresh {
        if let Some(cached) = ctx.state.health.lock().expect("paseo health lock").as_ref() {
            if cached.observed_at.elapsed() < Duration::from_secs(5) {
                return Ok(cached.value.clone());
            }
        }
    }
    let status = client.daemon_status()?;
    let mode = ctx.config.access_mode;
    let value = json!({
        "ok": status.reachable,
        "cli_available": true,
        "cli_version": status.cli_version,
        "daemon_version": status.daemon_version,
        "daemon_reachable": status.reachable,
        "connection_type": status.connection_type,
        "host_configured": ctx.host.is_some(),
        "host": redaction::redact_host(ctx.host.as_deref()),
        "access_mode": mode.as_str(),
        "capabilities": {
            "list_agents": true,
            "activity": true,
            "permissions": true,
            "send_prompt": mode.allows_assist(),
            "stop_agent": mode.allows_control()
        },
        "warnings": []
    });
    *ctx.state.health.lock().expect("paseo health lock") = Some(super::model::CachedHealth {
        observed_at: std::time::Instant::now(),
        value: value.clone(),
    });
    Ok(value)
}

fn list_agents<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
    rate_limit: bool,
) -> Result<Value, PaseoError> {
    if rate_limit {
        policy::check_list_interval(ctx)?;
    }
    let parsed = client.list_agents()?;
    let limit = usize_arg(args, "limit", 100, 1, 100)?;
    let include_completed = bool_arg(args, "include_completed", true);
    let all_directories = bool_arg(args, "all_directories", true);
    let workspace = bounded_string_arg(args, "workspace_path", 4_096)?
        .map(str::to_string)
        .or_else(|| (!all_directories).then(|| ctx.workspace_path.display().to_string()));
    let statuses = string_array(args, "status", 10, 32)?;
    let patterns = string_array(args, "name_patterns", 20, 256)?;
    let labels = string_array(args, "labels", 20, 120)?;
    let agents = monitor::filter_agents(
        parsed.data,
        include_completed,
        workspace.as_deref(),
        &statuses,
        &patterns,
        &labels,
        limit,
    )?;
    Ok(json!({
        "ok": true,
        "agents": agents,
        "count": agents.len(),
        "limit": limit,
        "source_format": parsed.source_format,
        "parser_version": parsed.parser_version,
        "cli_version": client.cli_version()?,
        "missing_fields": parsed.missing_fields,
        "truncated": parsed.truncated
    }))
}

fn activity<C: PaseoClient>(
    _ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    let agent_id = required_string(args, "agent_id")?;
    policy::validate_agent_id(agent_id)?;
    let tail = u32_arg(args, "tail", 30, 1, 100)?;
    let filter = string_arg(args, "filter").unwrap_or("all");
    if !matches!(filter, "all" | "messages" | "tools" | "errors") {
        return Err(PaseoError::argument(
            "filter must be all, messages, tools, or errors",
        ));
    }
    let max_bytes = usize_arg(args, "max_bytes", 65_536, 1_024, 262_144)?;
    let parsed = client.get_activity(agent_id, tail, filter, max_bytes)?;
    Ok(json!({
        "ok": true,
        "agent_id": agent_id,
        "events": parsed.data,
        "count": parsed.data.len(),
        "source_format": parsed.source_format,
        "parser_version": parsed.parser_version,
        "cli_version": client.cli_version()?,
        "missing_fields": parsed.missing_fields,
        "truncated": parsed.truncated
    }))
}

fn permissions<C: PaseoClient>(client: &C, args: &Value) -> Result<Value, PaseoError> {
    let agent_id = string_arg(args, "agent_id");
    if let Some(agent_id) = agent_id {
        policy::validate_agent_id(agent_id)?;
    }
    let mut parsed = client.list_pending_permissions()?;
    if let Some(agent_id) = agent_id {
        parsed
            .data
            .retain(|item| item.agent_id.as_deref() == Some(agent_id));
    }
    Ok(json!({
        "ok": true,
        "permissions": parsed.data,
        "count": parsed.data.len(),
        "source_format": parsed.source_format,
        "parser_version": parsed.parser_version,
        "cli_version": client.cli_version()?,
        "missing_fields": parsed.missing_fields,
        "truncated": parsed.truncated
    }))
}

fn diagnose_agent<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    let agent_id = required_string(args, "agent_id")?;
    policy::validate_agent_id(agent_id)?;
    policy::begin_diagnosis(ctx, agent_id)?;
    let result = diagnose_agent_inner(ctx, client, args, agent_id);
    policy::finish_diagnosis(ctx, agent_id);
    result
}

fn diagnose_agent_inner<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
    agent_id: &str,
) -> Result<Value, PaseoError> {
    let agents = client.list_agents()?.data;
    let agent = find_agent(agents, agent_id)?;
    let permissions = client.list_pending_permissions()?.data;
    let tail = u32_arg(
        args,
        "activity_tail",
        ctx.config.monitor.activity_tail,
        1,
        100,
    )?;
    let activity = client.get_activity(
        agent_id,
        tail,
        "all",
        ctx.config.max_output_bytes.min(65_536),
    )?;
    let store = PaseoMonitorStore::for_workspace(&ctx.workspace_id)?;
    let loaded = store.load_latest()?;
    let previous = loaded.snapshot.as_ref().and_then(|snapshot| {
        snapshot
            .agents
            .iter()
            .find(|item| item.agent_id == agent_id)
    });
    let diagnosis = diagnose(DiagnosticInput {
        agent: &agent,
        events: &activity.data,
        permissions: &permissions,
        previous,
        stalled_after_minutes: u32_arg(
            args,
            "stalled_after_minutes",
            ctx.config.monitor.stalled_after_minutes,
            1,
            240,
        )?,
        repeat_error_threshold: u32_arg(
            args,
            "repeat_error_threshold",
            ctx.config.monitor.repeat_error_threshold,
            1,
            10,
        )?,
        truncated: activity.truncated,
    });
    Ok(diagnosis_value(diagnosis, loaded.corrupted_files))
}

fn send_prompt<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    let agent_id = required_string(args, "agent_id")?;
    let prompt = required_string(args, "prompt")?;
    policy::validate_agent_id(agent_id)?;
    policy::validate_prompt(prompt)?;
    if !bool_arg(args, "no_wait", true) {
        return Err(PaseoError::argument("no_wait must be true"));
    }
    let reason = string_arg(args, "reason").unwrap_or("");
    if reason.chars().count() > 500 {
        return Err(PaseoError::argument(
            "reason must not exceed 500 characters",
        ));
    }
    policy::begin_send(ctx, agent_id)?;
    if let Err(error) = client.send_prompt(agent_id, prompt) {
        audit(
            ctx,
            "send_prompt",
            "failed",
            agent_id,
            reason,
            prompt.chars().count(),
        );
        return Err(error);
    }
    audit(
        ctx,
        "send_prompt",
        "succeeded",
        agent_id,
        reason,
        prompt.chars().count(),
    );
    Ok(
        json!({"ok": true, "agent_id": agent_id, "queued": true, "prompt_length": prompt.chars().count()}),
    )
}

fn stop_agent<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    let agent_id = required_string(args, "agent_id")?;
    let reason = required_string(args, "reason")?;
    policy::validate_agent_id(agent_id)?;
    if !bool_arg(args, "confirm", false) {
        return Err(PaseoError::new(
            "PASEO_CONFIRMATION_REQUIRED",
            "paseo_stop_agent requires confirm=true.",
            false,
            "policy",
            json!({}),
        ));
    }
    if reason.trim().is_empty() || reason.chars().count() > 500 {
        return Err(PaseoError::argument(
            "reason must be non-empty and at most 500 characters",
        ));
    }
    let before = find_agent(client.list_agents()?.data, agent_id)?;
    policy::begin_stop(ctx, agent_id)?;
    let stop_result = client.stop_agent(agent_id);
    policy::finish_stop(ctx, agent_id);
    if let Err(error) = stop_result {
        audit(ctx, "stop_agent", "failed", agent_id, reason, 0);
        return Err(error);
    }
    audit(ctx, "stop_agent", "succeeded", agent_id, reason, 0);
    let (after_status, warnings) = match client.list_agents() {
        Ok(parsed) => (
            parsed
                .data
                .into_iter()
                .find(|agent| agent.id == agent_id)
                .map(|agent| agent.status.as_str())
                .unwrap_or("UNKNOWN"),
            Vec::new(),
        ),
        Err(_) => ("UNKNOWN", vec!["post_stop_status_unavailable"]),
    };
    Ok(json!({
        "ok": true,
        "agent_id": agent_id,
        "before_status": before.status.as_str(),
        "after_status": after_status,
        "stop_requested": true,
        "warnings": warnings
    }))
}

fn diagnosis_value(diagnosis: PaseoDiagnosis, corrupted: usize) -> Value {
    json!({
        "ok": true,
        "agent_id": diagnosis.agent_id,
        "classification": diagnosis.classification.as_str(),
        "confidence": diagnosis.confidence,
        "current_state": diagnosis.current_state,
        "last_effective_progress_at": diagnosis.last_effective_progress_at,
        "stalled_for_minutes": diagnosis.stalled_for_minutes,
        "blocking_stage": diagnosis.blocking_stage,
        "blocking_tool": diagnosis.blocking_tool,
        "pending_permission": diagnosis.pending_permission,
        "repeated_errors": diagnosis.repeated_errors,
        "evidence": diagnosis.evidence,
        "likely_causes": diagnosis.likely_causes,
        "recommended_actions": diagnosis.recommended_actions,
        "safe_automatic_action": diagnosis.safe_automatic_action,
        "requires_user_action": diagnosis.requires_user_action,
        "truncated": diagnosis.truncated,
        "corrupted_snapshots_ignored": corrupted
    })
}

fn find_agent(agents: Vec<PaseoAgent>, agent_id: &str) -> Result<PaseoAgent, PaseoError> {
    agents
        .into_iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| {
            PaseoError::new(
                "PASEO_AGENT_NOT_FOUND",
                "Paseo agent was not found.",
                false,
                "agent_lookup",
                json!({"agent_id": agent_id}),
            )
        })
}

fn audit(
    ctx: &PaseoRuntimeContext,
    action: &str,
    outcome: &str,
    agent_id: &str,
    reason: &str,
    prompt_length: usize,
) {
    crate::tunnel::append_profile_log(
        &ctx.workspace_id,
        "paseo-audit.log",
        &format!(
            "[paseo] action={action} outcome={outcome} agent={agent_id} reason={} prompt_length={prompt_length}",
            redaction::bounded(&redaction::redact(reason), 240)
        ),
    );
}

fn error_value(error: PaseoError) -> Value {
    json!({
        "ok": false,
        "error": error.value(),
        "next_actions": [{"action": "paseo_health", "reason": "Verify Paseo CLI and daemon connectivity."}]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::paseo::model::PaseoAgentStatus;

    #[test]
    fn bounded_filter_enforces_limit_and_patterns() {
        let agents = (0..5)
            .map(|index| PaseoAgent {
                id: format!("agent-{index}"),
                name: Some(format!("worker-{index}")),
                status: PaseoAgentStatus::Running,
                provider: None,
                workspace: Some("C:\\work".into()),
                created_at: None,
                updated_at: None,
                last_activity_at: None,
                labels: vec!["team=one".into()],
                missing_fields: Vec::new(),
            })
            .collect();
        let filtered = monitor::filter_agents(
            agents,
            true,
            Some("C:/work"),
            &[],
            &["worker-*".into()],
            &["team=one".into()],
            2,
        )
        .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn desktop_health_test_does_not_probe_when_disabled() {
        let context = PaseoRuntimeContext::disabled(std::path::PathBuf::from("."));
        let result = test_health(&context, true);
        assert_eq!(result["error"]["code"], "PASEO_DISABLED");
    }
}
