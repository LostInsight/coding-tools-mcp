use std::collections::HashMap;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use super::model::{
    now_rfc3339, parse_rfc3339, ActivityEvent, DiagnosisClassification, PaseoAgent,
    PaseoAgentStatus, PaseoDiagnosis, PermissionSummary, StoredAgentState,
};
use super::redaction::{bounded, redact};

pub struct DiagnosticInput<'a> {
    pub agent: &'a PaseoAgent,
    pub events: &'a [ActivityEvent],
    pub permissions: &'a [PermissionSummary],
    pub previous: Option<&'a StoredAgentState>,
    pub stalled_after_minutes: u32,
    pub repeat_error_threshold: u32,
    pub truncated: bool,
}

pub fn diagnose(input: DiagnosticInput<'_>) -> PaseoDiagnosis {
    let now = OffsetDateTime::now_utc();
    let fingerprint = activity_fingerprint(input.agent, input.events);
    let progress_changed = input
        .previous
        .is_none_or(|previous| previous.activity_fingerprint != fingerprint);
    let last_progress = effective_progress_at(&input, progress_changed, now);
    let stalled_for = last_progress
        .and_then(|at| u64::try_from((now - at).whole_minutes()).ok())
        .filter(|minutes| *minutes > 0);
    let pending_permission = input.permissions.iter().any(|permission| {
        permission
            .agent_id
            .as_deref()
            .is_none_or(|agent_id| agent_id == input.agent.id)
    });
    let repeated_errors = repeated_errors(input.events, input.repeat_error_threshold);
    let repeated_operation_groups = repeated_operation_groups(input.events);
    let waiting_user = input.events.iter().rev().take(4).any(waiting_for_user);
    let external_wait = input.agent.status == PaseoAgentStatus::Running
        && input.previous.is_some()
        && progress_changed
        && input.events.iter().rev().take(4).any(long_running_tool);
    let stalled = input.agent.status == PaseoAgentStatus::Running
        && !progress_changed
        && stalled_for.is_some_and(|minutes| minutes >= u64::from(input.stalled_after_minutes));
    let incomplete = input.events.iter().rev().take(5).any(incomplete_evidence);
    let completed_evidence = input.events.iter().rev().take(5).any(completion_evidence);

    let classification = if input.agent.status == PaseoAgentStatus::Completed
        || (input.agent.status == PaseoAgentStatus::Stopped && completed_evidence)
    {
        DiagnosisClassification::Completed
    } else if input.agent.status == PaseoAgentStatus::Crashed {
        DiagnosisClassification::Crashed
    } else if pending_permission {
        DiagnosisClassification::WaitingPermission
    } else if !repeated_errors.is_empty() {
        DiagnosisClassification::RepeatedFailure
    } else if waiting_user {
        DiagnosisClassification::WaitingUserInput
    } else if external_wait {
        DiagnosisClassification::ExternalWait
    } else if stalled {
        DiagnosisClassification::PossiblyStalled
    } else if matches!(
        input.agent.status,
        PaseoAgentStatus::Idle | PaseoAgentStatus::Stopped
    ) && incomplete
    {
        DiagnosisClassification::IdleIncomplete
    } else if matches!(
        input.agent.status,
        PaseoAgentStatus::Running | PaseoAgentStatus::Idle
    ) && (input.events.iter().any(|event| event.is_progress)
        || input.agent.last_activity_at.is_some())
    {
        DiagnosisClassification::Healthy
    } else {
        DiagnosisClassification::Unknown
    };

    build_diagnosis(
        input,
        classification,
        pending_permission,
        repeated_errors,
        repeated_operation_groups,
        last_progress,
        stalled_for,
        progress_changed,
    )
}

pub fn stored_state(
    agent: &PaseoAgent,
    diagnosis: &PaseoDiagnosis,
    events: &[ActivityEvent],
) -> StoredAgentState {
    StoredAgentState {
        agent_id: agent.id.clone(),
        name: agent
            .name
            .as_deref()
            .map(|name| bounded(&redact(name), 160)),
        status: agent.status.as_str().into(),
        classification: diagnosis.classification,
        activity_fingerprint: activity_fingerprint(agent, events),
        last_effective_progress_at: diagnosis.last_effective_progress_at.clone(),
        observed_at: now_rfc3339(),
        degraded: false,
        warnings: Vec::new(),
    }
}

pub fn activity_fingerprint(agent: &PaseoAgent, events: &[ActivityEvent]) -> String {
    let mut hash = Sha256::new();
    hash.update(agent.status.as_str().as_bytes());
    for event in events {
        hash.update(event.event_type.as_bytes());
        hash.update(event.summary.as_bytes());
        if let Some(signature) = event.error_signature.as_deref() {
            hash.update(signature.as_bytes());
        }
    }
    let digest = format!("{:x}", hash.finalize());
    digest[..24].to_string()
}

fn effective_progress_at(
    input: &DiagnosticInput<'_>,
    progress_changed: bool,
    now: OffsetDateTime,
) -> Option<OffsetDateTime> {
    input
        .agent
        .last_activity_at
        .as_deref()
        .and_then(parse_rfc3339)
        .or_else(|| input.agent.updated_at.as_deref().and_then(parse_rfc3339))
        .or_else(|| {
            if progress_changed && input.events.iter().any(|event| event.is_progress) {
                Some(now)
            } else {
                input
                    .previous
                    .and_then(|previous| previous.last_effective_progress_at.as_deref())
                    .and_then(parse_rfc3339)
            }
        })
}

fn repeated_errors(events: &[ActivityEvent], threshold: u32) -> Vec<Value> {
    let mut counts: HashMap<&str, (u32, Option<&str>)> = HashMap::new();
    for event in events {
        if let Some(signature) = event.error_signature.as_deref() {
            let entry = counts.entry(signature).or_default();
            entry.0 += 1;
            entry.1 = event.occurred_at.as_deref().or(entry.1);
        }
    }
    let mut repeated = counts
        .into_iter()
        .filter(|(_, (count, _))| *count >= threshold)
        .map(|(signature, (count, last_seen_at))| {
            json!({"signature": signature, "count": count, "last_seen_at": last_seen_at})
        })
        .collect::<Vec<_>>();
    repeated.sort_by(|left, right| right["count"].as_u64().cmp(&left["count"].as_u64()));
    repeated
}

fn repeated_operation_groups(events: &[ActivityEvent]) -> usize {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for event in events.iter().filter(|event| event.event_type == "tools") {
        *counts.entry(event.summary.as_str()).or_default() += 1;
    }
    counts.values().filter(|count| **count >= 3).count()
}

fn waiting_for_user(event: &ActivityEvent) -> bool {
    let summary = event.summary.to_ascii_lowercase();
    [
        "waiting for user",
        "awaiting user",
        "awaiting input",
        "need your input",
        "please choose",
        "user decision",
    ]
    .iter()
    .any(|needle| summary.contains(needle))
}

fn long_running_tool(event: &ActivityEvent) -> bool {
    if event.event_type != "tools" {
        return false;
    }
    let summary = event.summary.to_ascii_lowercase();
    [
        "build", "test", "download", "install", "compile", "cargo", "npm",
    ]
    .iter()
    .any(|needle| summary.contains(needle))
}

fn incomplete_evidence(event: &ActivityEvent) -> bool {
    let summary = event.summary.to_ascii_lowercase();
    [
        "remaining",
        "unfinished",
        "next step",
        "blocked",
        "cannot continue",
        "still need",
    ]
    .iter()
    .any(|needle| summary.contains(needle))
}

fn completion_evidence(event: &ActivityEvent) -> bool {
    let summary = event.summary.to_ascii_lowercase();
    [
        "completed",
        "finished",
        "all tests pass",
        "task is done",
        "indicates completion",
    ]
    .iter()
    .any(|needle| summary.contains(needle))
}

#[allow(clippy::too_many_arguments)]
fn build_diagnosis(
    input: DiagnosticInput<'_>,
    classification: DiagnosisClassification,
    pending_permission: bool,
    repeated_errors: Vec<Value>,
    repeated_operation_groups: usize,
    last_progress: Option<OffsetDateTime>,
    stalled_for_minutes: Option<u64>,
    progress_changed: bool,
) -> PaseoDiagnosis {
    let (confidence, blocking_stage, recommendations, causes, user_action, safe_action) =
        match classification {
            DiagnosisClassification::Completed => ("high", None, vec!["wait"], vec![], false, None),
            DiagnosisClassification::WaitingPermission => (
                "high",
                Some("permission"),
                vec!["review_permission"],
                vec!["pending_permission"],
                true,
                None,
            ),
            DiagnosisClassification::WaitingUserInput => (
                "high",
                Some("user_input"),
                vec!["user_decision_required"],
                vec!["explicit_user_input_request"],
                true,
                None,
            ),
            DiagnosisClassification::ExternalWait => (
                "medium",
                Some("external_process"),
                vec!["inspect_external_process", "wait"],
                vec!["long_running_tool_with_output"],
                false,
                None,
            ),
            DiagnosisClassification::PossiblyStalled => (
                "medium",
                Some("tool_execution"),
                vec!["send_diagnostic_prompt", "stop_and_resume_manually"],
                vec!["no_effective_progress_across_snapshots"],
                false,
                Some("send_diagnostic_prompt"),
            ),
            DiagnosisClassification::RepeatedFailure => (
                "high",
                Some("tool_execution"),
                vec!["send_diagnostic_prompt", "stop_and_resume_manually"],
                vec!["repeated_normalized_error"],
                false,
                Some("send_diagnostic_prompt"),
            ),
            DiagnosisClassification::Crashed => (
                "high",
                Some("agent_process"),
                vec!["stop_and_resume_manually"],
                vec!["agent_reported_crashed"],
                true,
                None,
            ),
            DiagnosisClassification::IdleIncomplete => (
                "medium",
                Some("idle"),
                vec!["send_continue_prompt"],
                vec!["idle_with_unfinished_evidence"],
                false,
                Some("send_continue_prompt"),
            ),
            DiagnosisClassification::Healthy => ("medium", None, vec!["wait"], vec![], false, None),
            DiagnosisClassification::Unknown => (
                "low",
                None,
                vec!["wait", "user_decision_required"],
                vec!["insufficient_structured_evidence"],
                false,
                None,
            ),
        };
    let mut evidence = vec![format!("agent_status={}", input.agent.status.as_str())];
    if pending_permission {
        evidence.push("pending_permission=true".into());
    }
    if !repeated_errors.is_empty() {
        evidence.push(format!("repeated_error_groups={}", repeated_errors.len()));
    }
    if repeated_operation_groups > 0 {
        evidence.push(format!(
            "repeated_operation_groups={repeated_operation_groups}"
        ));
    }
    if input.previous.is_some() {
        evidence.push(format!("activity_changed={progress_changed}"));
    }
    PaseoDiagnosis {
        agent_id: input.agent.id.clone(),
        classification,
        confidence: confidence.into(),
        current_state: input.agent.status.as_str().into(),
        last_effective_progress_at: last_progress.and_then(|time| {
            time.format(&time::format_description::well_known::Rfc3339)
                .ok()
        }),
        stalled_for_minutes,
        blocking_stage: blocking_stage.map(str::to_string),
        blocking_tool: input
            .events
            .iter()
            .rev()
            .find(|event| event.event_type == "tools")
            .map(|event| tool_hint(&event.summary)),
        pending_permission,
        repeated_errors,
        evidence,
        likely_causes: causes.into_iter().map(str::to_string).collect(),
        recommended_actions: recommendations.into_iter().map(str::to_string).collect(),
        safe_automatic_action: safe_action.map(str::to_string),
        requires_user_action: user_action,
        truncated: input.truncated,
    }
}

fn tool_hint(summary: &str) -> String {
    summary
        .rsplit_once(':')
        .map(|(_, value)| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(summary)
        .split_whitespace()
        .next()
        .unwrap_or("unknown")
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
