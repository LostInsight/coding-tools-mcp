use std::collections::HashMap;

use serde_json::{json, Value};

use super::args::{bool_arg, usize_arg};
use super::client::PaseoClient;
use super::diagnostics::{diagnose, stored_state, DiagnosticInput};
use super::model::{
    now_rfc3339, DiagnosisClassification, PaseoError, PaseoRuntimeContext, StoredAgentState,
    StoredMonitorSnapshot,
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
    let permissions = client.list_pending_permissions()?.data;
    let agents = monitor::filter_monitor_agents(ctx, client.list_agents()?.data, max_agents)?;
    let now = now_rfc3339();
    let mut current = Vec::new();
    let mut summaries = Vec::new();

    for agent in &agents {
        let previous_state = previous_map.get(agent.id.as_str()).copied();
        let needs_detail = monitor::detailed_check(
            agent,
            previous_state,
            &permissions,
            ctx.config.monitor.stalled_after_minutes,
        );
        let (events, activity_truncated) = if needs_detail {
            let activity = client.get_activity(
                agent.id.as_str(),
                ctx.config.monitor.activity_tail,
                "all",
                ctx.config.max_output_bytes.min(65_536),
            )?;
            (activity.data, activity.truncated)
        } else {
            (Vec::new(), false)
        };
        let diagnosis = if needs_detail {
            diagnose(DiagnosticInput {
                agent,
                events: &events,
                permissions: &permissions,
                previous: previous_state,
                stalled_after_minutes: ctx.config.monitor.stalled_after_minutes,
                repeat_error_threshold: ctx.config.monitor.repeat_error_threshold,
                truncated: activity_truncated,
            })
        } else {
            monitor::lightweight_diagnosis(agent, previous_state, &now)
        };
        current.push(stored_state(agent, &diagnosis, &events));
        if include_healthy || monitor::is_anomaly(diagnosis.classification) {
            summaries.push(json!({
                "agent_id": agent.id,
                "name": agent.name,
                "status": agent.status.as_str(),
                "classification": diagnosis.classification.as_str(),
                "last_effective_progress_at": diagnosis.last_effective_progress_at,
                "blocking_stage": diagnosis.blocking_stage
            }));
        }
    }

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
        "corrupted_snapshots_ignored": loaded.corrupted_files,
        "next_recommended_check_seconds": 3600
    }))
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
                "status": agent.status,
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
