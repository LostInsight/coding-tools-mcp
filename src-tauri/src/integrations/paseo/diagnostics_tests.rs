use super::*;
use crate::integrations::paseo::model::StoredMonitorSnapshot;

fn agent(status: PaseoAgentStatus) -> PaseoAgent {
    PaseoAgent {
        id: "agent-1".into(),
        name: None,
        status,
        provider: None,
        workspace: None,
        created_at: None,
        updated_at: None,
        last_activity_at: None,
        labels: Vec::new(),
        missing_fields: Vec::new(),
    }
}

fn event(kind: &str, summary: &str, error: Option<&str>) -> ActivityEvent {
    ActivityEvent {
        event_type: kind.into(),
        occurred_at: None,
        summary: summary.into(),
        is_progress: error.is_none(),
        is_waiting: false,
        error_signature: error.map(str::to_string),
    }
}

fn classify(
    status: PaseoAgentStatus,
    events: &[ActivityEvent],
    permissions: &[PermissionSummary],
) -> DiagnosisClassification {
    diagnose(DiagnosticInput {
        agent: &agent(status),
        events,
        permissions,
        previous: None,
        stalled_after_minutes: 45,
        repeat_error_threshold: 2,
        truncated: false,
    })
    .classification
}

#[test]
fn covers_immediate_diagnostic_classes() {
    assert_eq!(
        classify(
            PaseoAgentStatus::Running,
            &[event("messages", "progress", None)],
            &[]
        ),
        DiagnosisClassification::Healthy
    );
    assert_eq!(
        classify(PaseoAgentStatus::Completed, &[], &[]),
        DiagnosisClassification::Completed
    );
    assert_eq!(
        classify(
            PaseoAgentStatus::Stopped,
            &[event("messages", "message indicates completion", None)],
            &[]
        ),
        DiagnosisClassification::Completed
    );
    assert_eq!(
        classify(PaseoAgentStatus::Crashed, &[], &[]),
        DiagnosisClassification::Crashed
    );
    assert_eq!(
        classify(
            PaseoAgentStatus::Running,
            &[event("messages", "waiting for user", None)],
            &[]
        ),
        DiagnosisClassification::WaitingUserInput
    );
    assert_eq!(
        classify(
            PaseoAgentStatus::Idle,
            &[event("messages", "remaining work", None)],
            &[]
        ),
        DiagnosisClassification::IdleIncomplete
    );
    assert_eq!(
        classify(PaseoAgentStatus::Unknown, &[], &[]),
        DiagnosisClassification::Unknown
    );
}

#[test]
fn pending_permission_has_priority() {
    let permission = PermissionSummary {
        request_id: Some("req-1".into()),
        agent_id: Some("agent-1".into()),
        permission_type: "shell".into(),
        requested_at: None,
        summary: "bounded".into(),
    };
    assert_eq!(
        classify(PaseoAgentStatus::Running, &[], &[permission]),
        DiagnosisClassification::WaitingPermission
    );
}

#[test]
fn repeated_errors_are_aggregated() {
    let events = [
        event("errors", "first", Some("err-a")),
        event("errors", "second", Some("err-a")),
    ];
    assert_eq!(
        classify(PaseoAgentStatus::Running, &events, &[]),
        DiagnosisClassification::RepeatedFailure
    );
}

#[test]
fn repeated_tool_operations_are_reported_as_evidence() {
    let events = [
        event("tools", "tool activity observed: cargo", None),
        event("tools", "tool activity observed: cargo", None),
        event("tools", "tool activity observed: cargo", None),
    ];
    let result = diagnose(DiagnosticInput {
        agent: &agent(PaseoAgentStatus::Running),
        events: &events,
        permissions: &[],
        previous: None,
        stalled_after_minutes: 45,
        repeat_error_threshold: 2,
        truncated: false,
    });
    assert!(result
        .evidence
        .iter()
        .any(|item| item == "repeated_operation_groups=1"));
}

#[test]
fn first_silent_observation_is_not_stalled() {
    assert_eq!(
        classify(PaseoAgentStatus::Running, &[], &[]),
        DiagnosisClassification::Unknown
    );
}

#[test]
fn external_wait_requires_output_growth_across_observations() {
    let agent = agent(PaseoAgentStatus::Running);
    let events = [event("tools", "tool activity observed: cargo", None)];
    assert_eq!(
        classify(PaseoAgentStatus::Running, &events, &[]),
        DiagnosisClassification::Healthy
    );
    let previous = StoredAgentState {
        agent_id: agent.id.clone(),
        name: None,
        status: "RUNNING".into(),
        classification: DiagnosisClassification::Healthy,
        activity_fingerprint: "older-output".into(),
        last_effective_progress_at: Some("2020-01-01T00:00:00Z".into()),
        observed_at: "2020-01-01T00:00:00Z".into(),
    };
    let result = diagnose(DiagnosticInput {
        agent: &agent,
        events: &events,
        permissions: &[],
        previous: Some(&previous),
        stalled_after_minutes: 45,
        repeat_error_threshold: 2,
        truncated: false,
    });
    assert_eq!(result.classification, DiagnosisClassification::ExternalWait);
    assert_eq!(result.blocking_tool.as_deref(), Some("cargo"));
}

#[test]
fn unchanged_activity_after_threshold_is_stalled() {
    let agent = agent(PaseoAgentStatus::Running);
    let events = [event("messages", "message activity observed", None)];
    let previous = StoredAgentState {
        agent_id: agent.id.clone(),
        name: None,
        status: "RUNNING".into(),
        classification: DiagnosisClassification::Healthy,
        activity_fingerprint: activity_fingerprint(&agent, &events),
        last_effective_progress_at: Some("2020-01-01T00:00:00Z".into()),
        observed_at: "2020-01-01T00:00:00Z".into(),
    };
    let result = diagnose(DiagnosticInput {
        agent: &agent,
        events: &events,
        permissions: &[],
        previous: Some(&previous),
        stalled_after_minutes: 45,
        repeat_error_threshold: 2,
        truncated: false,
    });
    assert_eq!(
        result.classification,
        DiagnosisClassification::PossiblyStalled
    );
}

#[test]
fn stored_snapshot_name_is_redacted_and_bounded() {
    let mut agent = agent(PaseoAgentStatus::Running);
    agent.name = Some(format!("token=secret {}", "x".repeat(300)));
    let diagnosis = diagnose(DiagnosticInput {
        agent: &agent,
        events: &[],
        permissions: &[],
        previous: None,
        stalled_after_minutes: 45,
        repeat_error_threshold: 2,
        truncated: false,
    });
    let stored = stored_state(&agent, &diagnosis, &[]);
    let name = stored.name.as_deref().unwrap();
    assert!(!name.contains("secret"));
    assert!(name.chars().count() <= 161);

    let serialized = serde_json::to_string(&StoredMonitorSnapshot {
        snapshot_id: "snapshot-1".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        agents: vec![stored],
    })
    .unwrap();
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("prompt"));
}
