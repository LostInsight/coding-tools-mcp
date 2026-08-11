use std::collections::HashMap;

use serde_json::{json, Value};

use super::args::{bool_arg, usize_arg};
use super::client::PaseoClient;
use super::diagnostics::{diagnose, stored_state, DiagnosticInput};
use super::model::{
    now_rfc3339, DiagnosisClassification, PaseoAgent, PaseoDiagnosis, PaseoError,
    PaseoRuntimeContext, PermissionSummary, StoredAgentState, StoredMonitorSnapshot,
};
use super::monitor;
use super::monitor_store::PaseoMonitorStore;
use super::policy;

pub(super) fn monitor_snapshot<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    if !ctx.config.monitor.enabled {
        return Err(PaseoError::denied(
            "Paseo monitor is disabled for this workspace.",
        ));
    }
    policy::begin_monitor(ctx)?;
    let result = monitor_snapshot_inner(ctx, client, args);
    policy::finish_monitor(ctx);
    result
}

fn monitor_snapshot_inner<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    args: &Value,
) -> Result<Value, PaseoError> {
    let include_healthy = bool_arg(args, "include_healthy", false);
    let compare_previous = bool_arg(args, "compare_previous", true);
    let max_agents = usize_arg(args, "max_agents", 100, 1, 100)?;
    let store = PaseoMonitorStore::for_workspace(&ctx.workspace_id)?;
    let loaded = store.load_latest()?;
    if !bool_arg(args, "refresh", true) {
        if let Some(snapshot) = loaded.snapshot.as_ref() {
            return cached_snapshot_value(snapshot, include_healthy, loaded.corrupted_files);
        }
    }

    let previous = compare_previous
        .then_some(loaded.snapshot.as_ref())
        .flatten();
    let previous_map: HashMap<&str, &StoredAgentState> = previous
        .map(|snapshot| {
            snapshot
                .agents
                .iter()
                .map(|state| (state.agent_id.as_str(), state))
                .collect()
        })
        .unwrap_or_default();
    let (permissions, mut warnings) = match client.list_pending_permissions() {
        Ok(parsed) => (parsed.data, parsed.warnings),
        Err(error) => (
            Vec::new(),
            vec![json!({
                "scope": "permissions",
                "error": error.value()
            })],
        ),
    };
    let agents = monitor::filter_monitor_agents(ctx, client.list_agents()?.data, max_agents)?;
    let now = now_rfc3339();
    let (current, summaries, agent_warnings) = collect_agent_states(
        ctx,
        client,
        &agents,
        &permissions,
        &previous_map,
        &now,
        include_healthy,
    );
    warnings.extend(agent_warnings);

    let snapshot_id = uuid::Uuid::new_v4().to_string();
    let snapshot = StoredMonitorSnapshot {
        snapshot_id: snapshot_id.clone(),
        created_at: now.clone(),
        agents: current,
    };
    let changes = monitor::compare_snapshots(previous, &snapshot);
    store.save(&snapshot)?;
    let (healthy, blocked, completed) = summary_counts(&snapshot);
    let changed = previous.is_none() || changes.values().any(|items| !items.is_empty());
    Ok(json!({
        "ok": true,
        "snapshot_id": snapshot_id,
        "created_at": now,
        "previous_snapshot_id": previous.map(|snapshot| snapshot.snapshot_id.clone()),
        "changed": changed,
        "summary": {"total": snapshot.agents.len(), "healthy": healthy, "blocked": blocked, "completed": completed},
        "new_anomalies": changes["new_anomalies"],
        "changed_agents": changes["changed_agents"],
        "resolved_anomalies": changes["resolved_anomalies"],
        "completed_agents": changes["completed_agents"],
        "agents": summaries,
        "warnings": warnings,
        "corrupted_snapshots_ignored": loaded.corrupted_files,
        "next_recommended_check_seconds": 3600
    }))
}

fn collect_agent_states<C: PaseoClient>(
    ctx: &PaseoRuntimeContext,
    client: &C,
    agents: &[PaseoAgent],
    permissions: &[PermissionSummary],
    previous_map: &HashMap<&str, &StoredAgentState>,
    now: &str,
    include_healthy: bool,
) -> (Vec<StoredAgentState>, Vec<Value>, Vec<Value>) {
    let mut current = Vec::with_capacity(agents.len());
    let mut summaries = Vec::new();
    let mut warnings = Vec::new();
    for agent in agents {
        let previous_state = previous_map.get(agent.id.as_str()).copied();
        let needs_detail = monitor::detailed_check(
            agent,
            previous_state,
            permissions,
            ctx.config.monitor.stalled_after_minutes,
        );
        let activity = needs_detail.then(|| {
            client.get_activity(
                agent.id.as_str(),
                ctx.config.monitor.activity_tail,
                "all",
                ctx.config.max_output_bytes.min(65_536),
            )
        });
        let (events, activity_truncated, activity_warnings, activity_error) = match activity {
            Some(Ok(activity)) => {
                let warnings = (!activity.warnings.is_empty())
                    .then(|| {
                        vec![json!({
                            "agent_id": agent.id,
                            "scope": "activity",
                            "warnings": activity.warnings
                        })]
                    })
                    .unwrap_or_default();
                (activity.data, activity.truncated, warnings, None)
            }
            Some(Err(error)) => {
                let warning = json!({
                    "agent_id": agent.id,
                    "scope": "activity",
                    "error": error.value()
                });
                (Vec::new(), false, vec![warning.clone()], Some(warning))
            }
            None => (Vec::new(), false, Vec::new(), None),
        };
        let diagnosis = if let Some(warning) = activity_error.as_ref() {
            degraded_diagnosis(agent, warning)
        } else if needs_detail {
            diagnose(DiagnosticInput {
                agent,
                events: &events,
                permissions,
                previous: previous_state,
                stalled_after_minutes: ctx.config.monitor.stalled_after_minutes,
                repeat_error_threshold: ctx.config.monitor.repeat_error_threshold,
                truncated: activity_truncated,
            })
        } else {
            monitor::lightweight_diagnosis(agent, previous_state, now)
        };
        let mut state = stored_state(agent, &diagnosis, &events);
        state.degraded = !activity_warnings.is_empty();
        state.warnings = activity_warnings.clone();
        warnings.extend(activity_warnings);
        if include_healthy || monitor::is_anomaly(diagnosis.classification) {
            summaries.push(json!({
                "agent_id": agent.id,
                "name": agent.name,
                "status": if state.degraded { "DEGRADED" } else { agent.status.as_str() },
                "agent_status": agent.status.as_str(),
                "monitor_status": if state.degraded { "degraded" } else { "normal" },
                "parse_error": state.degraded,
                "warnings": state.warnings,
                "classification": diagnosis.classification.as_str(),
                "last_effective_progress_at": diagnosis.last_effective_progress_at,
                "blocking_stage": diagnosis.blocking_stage
            }));
        }
        current.push(state);
    }
    (current, summaries, warnings)
}

fn degraded_diagnosis(agent: &PaseoAgent, warning: &Value) -> PaseoDiagnosis {
    PaseoDiagnosis {
        agent_id: agent.id.clone(),
        classification: DiagnosisClassification::Unknown,
        confidence: "low".into(),
        current_state: "DEGRADED".into(),
        last_effective_progress_at: agent.last_activity_at.clone(),
        stalled_for_minutes: None,
        blocking_stage: Some("activity_parse".into()),
        blocking_tool: None,
        pending_permission: false,
        repeated_errors: Vec::new(),
        evidence: vec![format!(
            "activity_error={}",
            warning["error"]["code"]
                .as_str()
                .unwrap_or("PASEO_PARSE_ERROR")
        )],
        likely_causes: vec!["Paseo activity schema or transport degraded for this agent".into()],
        recommended_actions: vec!["inspect_agent_activity".into()],
        safe_automatic_action: None,
        requires_user_action: false,
        truncated: false,
    }
}

fn cached_snapshot_value(
    snapshot: &StoredMonitorSnapshot,
    include_healthy: bool,
    corrupted_files: usize,
) -> Result<Value, PaseoError> {
    let (healthy, blocked, completed) = summary_counts(snapshot);
    let agents = snapshot
        .agents
        .iter()
        .filter(|agent| include_healthy || monitor::is_anomaly(agent.classification))
        .map(|agent| {
            json!({
                "agent_id": agent.agent_id,
                "name": agent.name,
                "status": if agent.degraded { "DEGRADED" } else { agent.status.as_str() },
                "agent_status": agent.status,
                "monitor_status": if agent.degraded { "degraded" } else { "normal" },
                "parse_error": agent.degraded,
                "warnings": agent.warnings,
                "classification": agent.classification.as_str(),
                "last_effective_progress_at": agent.last_effective_progress_at
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "snapshot_id": snapshot.snapshot_id,
        "created_at": snapshot.created_at,
        "previous_snapshot_id": null,
        "changed": false,
        "summary": {"total": snapshot.agents.len(), "healthy": healthy, "blocked": blocked, "completed": completed},
        "new_anomalies": [],
        "changed_agents": [],
        "resolved_anomalies": [],
        "completed_agents": [],
        "agents": agents,
        "cached": true,
        "corrupted_snapshots_ignored": corrupted_files,
        "next_recommended_check_seconds": 3600
    }))
}

fn summary_counts(snapshot: &StoredMonitorSnapshot) -> (usize, usize, usize) {
    let healthy = snapshot
        .agents
        .iter()
        .filter(|agent| agent.classification == DiagnosisClassification::Healthy)
        .count();
    let completed = snapshot
        .agents
        .iter()
        .filter(|agent| agent.classification == DiagnosisClassification::Completed)
        .count();
    let blocked = snapshot.agents.len().saturating_sub(healthy + completed);
    (healthy, blocked, completed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::paseo::model::{
        ActivityEvent, ParsedPaseo, PaseoAgentStatus, PaseoCapabilities, PaseoDaemonStatus,
    };

    struct ActivityClient;

    impl PaseoClient for ActivityClient {
        fn cli_version(&self) -> Result<String, PaseoError> {
            Ok("0.3.1".into())
        }

        fn daemon_status(&self) -> Result<PaseoDaemonStatus, PaseoError> {
            unreachable!()
        }

        fn detect_capabilities(&self) -> Result<PaseoCapabilities, PaseoError> {
            Ok(PaseoCapabilities::default())
        }

        fn list_agents(&self) -> Result<ParsedPaseo<Vec<PaseoAgent>>, PaseoError> {
            unreachable!()
        }

        fn get_activity(
            &self,
            agent_id: &str,
            _tail: u32,
            _filter: &str,
            _max_bytes: usize,
        ) -> Result<ParsedPaseo<Vec<ActivityEvent>>, PaseoError> {
            if agent_id == "agent-bad" {
                return Err(PaseoError::new(
                    "PASEO_UNKNOWN_ACTIVITY",
                    "malformed activity",
                    false,
                    "parse_activity",
                    json!({}),
                ));
            }
            let warnings = if agent_id == "agent-partial" {
                vec![json!({
                    "code": "PASEO_PARSE_PARTIAL",
                    "message": "one malformed activity record was skipped"
                })]
            } else {
                Vec::new()
            };
            Ok(ParsedPaseo {
                data: vec![ActivityEvent {
                    kind: "Shell".into(),
                    event_type: "tools".into(),
                    occurred_at: None,
                    summary: "tool activity observed: cargo".into(),
                    known: true,
                    is_progress: true,
                    is_waiting: false,
                    requires_user_action: false,
                    error_signature: None,
                }],
                source_format: "text_fallback",
                parser_version: "test",
                missing_fields: Vec::new(),
                warnings,
                truncated: false,
            })
        }

        fn list_pending_permissions(
            &self,
        ) -> Result<ParsedPaseo<Vec<PermissionSummary>>, PaseoError> {
            unreachable!()
        }

        fn send_prompt(&self, _agent_id: &str, _prompt: &str) -> Result<Value, PaseoError> {
            unreachable!()
        }

        fn stop_agent(&self, _agent_id: &str) -> Result<Value, PaseoError> {
            unreachable!()
        }

        fn allow_permission(
            &self,
            _agent_id: &str,
            _request_id: &str,
        ) -> Result<Value, PaseoError> {
            unreachable!()
        }

        fn deny_permission(
            &self,
            _agent_id: &str,
            _request_id: &str,
            _message: Option<&str>,
        ) -> Result<Value, PaseoError> {
            unreachable!()
        }

        fn create_agent(
            &self,
            _prompt: &str,
            _title: Option<&str>,
            _provider: &str,
            _cwd: &str,
        ) -> Result<Value, PaseoError> {
            unreachable!()
        }
    }

    fn idle_agent(id: &str) -> PaseoAgent {
        PaseoAgent {
            id: id.into(),
            name: Some(id.into()),
            status: PaseoAgentStatus::Idle,
            provider: None,
            workspace: None,
            created_at: None,
            updated_at: None,
            last_activity_at: None,
            labels: Vec::new(),
            missing_fields: Vec::new(),
        }
    }

    #[test]
    fn one_malformed_agent_degrades_without_failing_other_agents() {
        let ctx = PaseoRuntimeContext::disabled(std::path::PathBuf::from("."));
        let agents = vec![idle_agent("agent-good"), idle_agent("agent-bad")];
        let previous = HashMap::new();
        let (states, summaries, warnings) = collect_agent_states(
            &ctx,
            &ActivityClient,
            &agents,
            &[],
            &previous,
            "2026-08-10T00:00:00Z",
            true,
        );

        assert_eq!(states.len(), 2);
        assert_eq!(summaries.len(), 2);
        assert!(!states[0].degraded);
        assert!(states[1].degraded);
        assert_eq!(summaries[1]["status"], "DEGRADED");
        assert_eq!(summaries[1]["parse_error"], true);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["agent_id"], "agent-bad");
    }

    #[test]
    fn partial_activity_preserves_events_and_marks_only_that_agent_degraded() {
        let ctx = PaseoRuntimeContext::disabled(std::path::PathBuf::from("."));
        let agents = vec![idle_agent("agent-good"), idle_agent("agent-partial")];
        let previous = HashMap::new();
        let (states, summaries, warnings) = collect_agent_states(
            &ctx,
            &ActivityClient,
            &agents,
            &[],
            &previous,
            "2026-08-10T00:00:00Z",
            true,
        );

        assert!(!states[0].degraded);
        assert!(states[1].degraded);
        assert!(!states[1].activity_fingerprint.is_empty());
        assert_eq!(summaries[1]["monitor_status"], "degraded");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["agent_id"], "agent-partial");
        assert_eq!(warnings[0]["warnings"][0]["code"], "PASEO_PARSE_PARTIAL");
    }
}
