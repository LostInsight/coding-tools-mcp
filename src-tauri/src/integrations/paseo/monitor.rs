use std::collections::HashMap;

use glob::Pattern;
use serde_json::{json, Value};

use super::model::{
    parse_rfc3339, DiagnosisClassification, PaseoAgent, PaseoAgentStatus, PaseoDiagnosis,
    PaseoError, PaseoRuntimeContext, PermissionSummary, StoredAgentState, StoredMonitorSnapshot,
};

pub fn filter_agents(
    agents: Vec<PaseoAgent>,
    include_completed: bool,
    workspace: Option<&str>,
    statuses: &[String],
    patterns: &[String],
    labels: &[String],
    limit: usize,
) -> Result<Vec<PaseoAgent>, PaseoError> {
    let patterns = patterns
        .iter()
        .map(|value| {
            Pattern::new(value)
                .map_err(|_| PaseoError::argument("name_patterns contains an invalid glob"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(agents
        .into_iter()
        .filter(|agent| {
            include_completed
                || !matches!(
                    agent.status,
                    PaseoAgentStatus::Completed | PaseoAgentStatus::Stopped
                )
        })
        .filter(|agent| {
            workspace.is_none_or(|path| {
                agent
                    .workspace
                    .as_deref()
                    .is_some_and(|candidate| paths_equal(candidate, path))
            })
        })
        .filter(|agent| {
            statuses.is_empty()
                || statuses
                    .iter()
                    .any(|status| status.eq_ignore_ascii_case(agent.status.as_str()))
        })
        .filter(|agent| {
            patterns.is_empty()
                || agent
                    .name
                    .as_deref()
                    .is_some_and(|name| patterns.iter().any(|pattern| pattern.matches(name)))
        })
        .filter(|agent| {
            labels.is_empty() || labels.iter().all(|label| agent.labels.contains(label))
        })
        .take(limit)
        .collect())
}

pub fn filter_monitor_agents(
    ctx: &PaseoRuntimeContext,
    agents: Vec<PaseoAgent>,
    limit: usize,
) -> Result<Vec<PaseoAgent>, PaseoError> {
    filter_agents(
        agents,
        true,
        ctx.config
            .monitor
            .workspace_only
            .then(|| ctx.workspace_path.to_string_lossy())
            .as_deref(),
        &[],
        &ctx.config.monitor.agent_name_patterns,
        &ctx.config.monitor.agent_labels,
        limit,
    )
    .map(|agents| {
        agents
            .into_iter()
            .filter(|agent| {
                ctx.config.monitor.agent_ids.is_empty()
                    || ctx
                        .config
                        .monitor
                        .agent_ids
                        .iter()
                        .any(|id| id == &agent.id)
            })
            .filter(|agent| {
                ctx.config.monitor.workspaces.is_empty()
                    || agent.workspace.as_deref().is_some_and(|workspace| {
                        ctx.config
                            .monitor
                            .workspaces
                            .iter()
                            .any(|selected| paths_equal(workspace, selected))
                    })
            })
            .filter(|agent| {
                let name = agent.name.as_deref().unwrap_or("");
                !ctx.config
                    .monitor
                    .exclude_name_patterns
                    .iter()
                    .any(|value| Pattern::new(value).is_ok_and(|pattern| pattern.matches(name)))
            })
            .collect()
    })
}

pub fn detailed_check(
    agent: &PaseoAgent,
    previous: Option<&StoredAgentState>,
    permissions: &[PermissionSummary],
    threshold: u32,
) -> bool {
    if permissions.iter().any(|item| {
        item.agent_id
            .as_deref()
            .is_none_or(|agent_id| agent_id == agent.id)
    }) || !matches!(
        agent.status,
        PaseoAgentStatus::Running | PaseoAgentStatus::Completed
    ) {
        return true;
    }
    previous
        .and_then(|state| {
            parse_rfc3339(
                &state
                    .last_effective_progress_at
                    .clone()
                    .unwrap_or(state.observed_at.clone()),
            )
        })
        .is_some_and(|time| {
            (time::OffsetDateTime::now_utc() - time).whole_minutes() >= i64::from(threshold)
        })
}

pub fn lightweight_diagnosis(
    agent: &PaseoAgent,
    previous: Option<&StoredAgentState>,
    now: &str,
) -> PaseoDiagnosis {
    let classification = if agent.status == PaseoAgentStatus::Completed {
        DiagnosisClassification::Completed
    } else if agent.status == PaseoAgentStatus::Running {
        DiagnosisClassification::Healthy
    } else {
        DiagnosisClassification::Unknown
    };
    PaseoDiagnosis {
        agent_id: agent.id.clone(),
        classification,
        confidence: "low".into(),
        current_state: agent.status.as_str().into(),
        last_effective_progress_at: (classification == DiagnosisClassification::Healthy)
            .then(|| lightweight_progress_at(agent, previous, now)),
        stalled_for_minutes: None,
        blocking_stage: None,
        blocking_tool: None,
        pending_permission: false,
        repeated_errors: Vec::new(),
        evidence: vec![format!("agent_status={}", agent.status.as_str())],
        likely_causes: Vec::new(),
        recommended_actions: vec!["wait".into()],
        safe_automatic_action: None,
        requires_user_action: false,
        truncated: false,
    }
}

fn lightweight_progress_at(
    agent: &PaseoAgent,
    previous: Option<&StoredAgentState>,
    now: &str,
) -> String {
    agent
        .last_activity_at
        .as_deref()
        .filter(|value| parse_rfc3339(value).is_some())
        .or_else(|| {
            agent
                .updated_at
                .as_deref()
                .filter(|value| parse_rfc3339(value).is_some())
        })
        .map(str::to_string)
        .or_else(|| previous.and_then(|state| state.last_effective_progress_at.clone()))
        .unwrap_or_else(|| now.to_string())
}

pub fn compare_snapshots(
    previous: Option<&StoredMonitorSnapshot>,
    current: &StoredMonitorSnapshot,
) -> HashMap<&'static str, Vec<Value>> {
    let mut changes = HashMap::from([
        ("new_anomalies", Vec::new()),
        ("changed_agents", Vec::new()),
        ("resolved_anomalies", Vec::new()),
        ("completed_agents", Vec::new()),
    ]);
    let Some(previous) = previous else {
        for agent in &current.agents {
            if is_anomaly(agent.classification) {
                changes
                    .get_mut("new_anomalies")
                    .expect("change key")
                    .push(change_value(agent, None));
            }
        }
        return changes;
    };
    let old: HashMap<&str, &StoredAgentState> = previous
        .agents
        .iter()
        .map(|agent| (agent.agent_id.as_str(), agent))
        .collect();
    for agent in &current.agents {
        match old.get(agent.agent_id.as_str()).copied() {
            Some(prior) => compare_agent(&mut changes, agent, prior),
            None if is_anomaly(agent.classification) => changes
                .get_mut("new_anomalies")
                .expect("change key")
                .push(change_value(agent, None)),
            None => {}
        }
    }
    let current_ids: std::collections::HashSet<&str> = current
        .agents
        .iter()
        .map(|agent| agent.agent_id.as_str())
        .collect();
    for prior in &previous.agents {
        if !current_ids.contains(prior.agent_id.as_str()) {
            changes
                .get_mut("changed_agents")
                .expect("change key")
                .push(json!({"agent_id": prior.agent_id, "change": "no_longer_visible"}));
        }
    }
    changes
}

pub fn is_anomaly(classification: DiagnosisClassification) -> bool {
    !matches!(
        classification,
        DiagnosisClassification::Healthy | DiagnosisClassification::Completed
    )
}

fn compare_agent(
    changes: &mut HashMap<&'static str, Vec<Value>>,
    agent: &StoredAgentState,
    prior: &StoredAgentState,
) {
    if prior.status != agent.status || prior.classification != agent.classification {
        changes
            .get_mut("changed_agents")
            .expect("change key")
            .push(change_value(agent, Some(prior)));
    }
    if !is_anomaly(prior.classification) && is_anomaly(agent.classification) {
        changes
            .get_mut("new_anomalies")
            .expect("change key")
            .push(change_value(agent, Some(prior)));
    }
    if is_anomaly(prior.classification) && !is_anomaly(agent.classification) {
        changes
            .get_mut("resolved_anomalies")
            .expect("change key")
            .push(change_value(agent, Some(prior)));
    }
    if prior.classification != DiagnosisClassification::Completed
        && agent.classification == DiagnosisClassification::Completed
    {
        changes
            .get_mut("completed_agents")
            .expect("change key")
            .push(change_value(agent, Some(prior)));
    }
}

fn change_value(current: &StoredAgentState, previous: Option<&StoredAgentState>) -> Value {
    json!({
        "agent_id": current.agent_id,
        "name": current.name,
        "classification": current.classification.as_str(),
        "previous_classification": previous.map(|item| item.classification.as_str()),
        "status": current.status,
        "previous_status": previous.map(|item| item.status.as_str())
    })
}

fn paths_equal(left: &str, right: &str) -> bool {
    #[cfg(windows)]
    return left
        .replace('/', "\\")
        .trim_end_matches('\\')
        .eq_ignore_ascii_case(right.replace('/', "\\").trim_end_matches('\\'));
    #[cfg(not(windows))]
    return left.trim_end_matches('/') == right.trim_end_matches('/');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_filters_by_selected_agent_ids_and_workspaces() {
        let mut config = crate::integrations::paseo::PaseoIntegrationConfig::default();
        config.monitor.agent_ids = vec!["agent-2".into()];
        config.monitor.workspaces = vec!["C:\\work\\selected".into()];
        let ctx = PaseoRuntimeContext::new(
            "workspace".into(),
            std::path::PathBuf::from("C:\\work"),
            config,
            None,
        );
        let agent = |id: &str, workspace: &str| PaseoAgent {
            id: id.into(),
            name: Some(id.into()),
            status: PaseoAgentStatus::Running,
            provider: None,
            workspace: Some(workspace.into()),
            created_at: None,
            updated_at: None,
            last_activity_at: None,
            labels: Vec::new(),
            missing_fields: Vec::new(),
        };

        let filtered = filter_monitor_agents(
            &ctx,
            vec![
                agent("agent-1", "C:\\work\\selected"),
                agent("agent-2", "C:\\work\\other"),
                agent("agent-2", "C:\\work\\selected"),
            ],
            100,
        )
        .expect("filter");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "agent-2");
    }

    #[test]
    fn unchanged_snapshots_have_no_events() {
        let agent = StoredAgentState {
            agent_id: "a".into(),
            name: None,
            status: "RUNNING".into(),
            classification: DiagnosisClassification::Healthy,
            activity_fingerprint: "x".into(),
            last_effective_progress_at: None,
            observed_at: "2026-01-01T00:00:00Z".into(),
        };
        let previous = StoredMonitorSnapshot {
            snapshot_id: "1".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            agents: vec![agent.clone()],
        };
        let current = StoredMonitorSnapshot {
            snapshot_id: "2".into(),
            created_at: "2026-01-01T01:00:00Z".into(),
            agents: vec![agent],
        };
        assert!(compare_snapshots(Some(&previous), &current)
            .values()
            .all(Vec::is_empty));
    }

    #[test]
    fn newly_stalled_agent_is_reported_as_a_new_anomaly() {
        let state = |classification: DiagnosisClassification| StoredAgentState {
            agent_id: "a".into(),
            name: None,
            status: "RUNNING".into(),
            classification,
            activity_fingerprint: "x".into(),
            last_effective_progress_at: None,
            observed_at: "2026-01-01T00:00:00Z".into(),
        };
        let previous = StoredMonitorSnapshot {
            snapshot_id: "1".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            agents: vec![state(DiagnosisClassification::Healthy)],
        };
        let current = StoredMonitorSnapshot {
            snapshot_id: "2".into(),
            created_at: "2026-01-01T01:00:00Z".into(),
            agents: vec![state(DiagnosisClassification::PossiblyStalled)],
        };

        let changes = compare_snapshots(Some(&previous), &current);
        assert_eq!(changes["new_anomalies"].len(), 1);
        assert_eq!(changes["changed_agents"].len(), 1);
    }

    #[test]
    fn snapshot_diff_reports_anomaly_recovery_completion_and_disappearance() {
        let states = |classification: DiagnosisClassification| StoredAgentState {
            agent_id: "a".into(),
            name: None,
            status: classification.as_str().into(),
            classification,
            activity_fingerprint: "x".into(),
            last_effective_progress_at: None,
            observed_at: "2026-01-01T00:00:00Z".into(),
        };
        let previous = StoredMonitorSnapshot {
            snapshot_id: "1".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            agents: vec![states(DiagnosisClassification::PossiblyStalled)],
        };
        let recovered = StoredMonitorSnapshot {
            snapshot_id: "2".into(),
            created_at: "2026-01-01T01:00:00Z".into(),
            agents: vec![states(DiagnosisClassification::Healthy)],
        };
        let changes = compare_snapshots(Some(&previous), &recovered);
        assert_eq!(changes["resolved_anomalies"].len(), 1);
        assert_eq!(changes["changed_agents"].len(), 1);

        let completed = StoredMonitorSnapshot {
            snapshot_id: "3".into(),
            created_at: "2026-01-01T02:00:00Z".into(),
            agents: vec![states(DiagnosisClassification::Completed)],
        };
        assert_eq!(
            compare_snapshots(Some(&recovered), &completed)["completed_agents"].len(),
            1
        );
        let empty = StoredMonitorSnapshot {
            snapshot_id: "4".into(),
            created_at: "2026-01-01T03:00:00Z".into(),
            agents: vec![],
        };
        assert_eq!(
            compare_snapshots(Some(&completed), &empty)["changed_agents"].len(),
            1
        );
    }

    #[test]
    fn lightweight_checks_preserve_known_progress_time() {
        let agent = PaseoAgent {
            id: "a".into(),
            name: None,
            status: PaseoAgentStatus::Running,
            provider: None,
            workspace: None,
            created_at: None,
            updated_at: None,
            last_activity_at: None,
            labels: Vec::new(),
            missing_fields: Vec::new(),
        };
        let previous = StoredAgentState {
            agent_id: "a".into(),
            name: None,
            status: "RUNNING".into(),
            classification: DiagnosisClassification::Healthy,
            activity_fingerprint: "fingerprint".into(),
            last_effective_progress_at: Some("2026-01-01T00:00:00Z".into()),
            observed_at: "2026-01-01T00:00:00Z".into(),
        };
        let diagnosis = lightweight_diagnosis(&agent, Some(&previous), "2026-01-01T00:05:00Z");
        assert_eq!(
            diagnosis.last_effective_progress_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
    }

    #[test]
    fn permission_without_agent_id_triggers_conservative_detail_check() {
        let agent = PaseoAgent {
            id: "a".into(),
            name: None,
            status: PaseoAgentStatus::Running,
            provider: None,
            workspace: None,
            created_at: None,
            updated_at: None,
            last_activity_at: None,
            labels: Vec::new(),
            missing_fields: Vec::new(),
        };
        let permission = PermissionSummary {
            agent_id: None,
            permission_type: "unknown".into(),
            requested_at: None,
            summary: "permission activity observed".into(),
        };
        assert!(detailed_check(&agent, None, &[permission], 45));
    }
}
