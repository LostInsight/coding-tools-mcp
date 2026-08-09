use serde_json::Value;

use super::model::{
    ActivityEvent, ParsedPaseo, PaseoAgent, PaseoAgentStatus, PaseoError, PermissionSummary,
};
use super::redaction::{bounded, normalize_error_signature, redact};

pub const JSON_PARSER_VERSION: &str = "paseo-json-v1";
pub const TEXT_PARSER_VERSION: &str = "paseo-0.2.x-0.3.x-activity-text-v3";

pub fn parse_agents(
    raw: &str,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<PaseoAgent>>, PaseoError> {
    let root: Value = parse_json(raw, truncated)?;
    let items = root
        .as_array()
        .or_else(|| root.get("agents").and_then(Value::as_array))
        .ok_or_else(|| parse_error("Agent response must contain an array", truncated))?;
    let mut agents = Vec::with_capacity(items.len());
    let mut response_missing = Vec::new();
    for item in items {
        let id = string_at(item, &["id", "agentId", "agent_id"])
            .filter(|value| !value.is_empty())
            .ok_or_else(|| parse_error("Agent response is missing id", truncated))?;
        super::policy::validate_agent_id(&id)
            .map_err(|_| parse_error("Agent response contains an invalid id", truncated))?;
        let provider = safe_string_at(item, &["provider", "providerName"], 120);
        let workspace = string_at(item, &["cwd", "workspace", "workspacePath"])
            .map(|value| bounded(&value, 4_096));
        let created_at = safe_string_at(item, &["createdAt", "created_at", "created"], 80);
        let updated_at = safe_string_at(item, &["updatedAt", "updated_at", "updated"], 80);
        let last_activity_at = safe_string_at(
            item,
            &["lastActivityAt", "last_activity_at", "lastActivity"],
            80,
        );
        let mut missing_fields = Vec::new();
        for (name, missing) in [
            ("provider", provider.is_none()),
            ("workspace", workspace.is_none()),
            ("created_at", created_at.is_none()),
            ("updated_at", updated_at.is_none()),
            ("last_activity_at", last_activity_at.is_none()),
        ] {
            if missing {
                missing_fields.push(name.to_string());
                if !response_missing.iter().any(|field| field == name) {
                    response_missing.push(name.to_string());
                }
            }
        }
        agents.push(PaseoAgent {
            id,
            name: safe_string_at(item, &["name", "title"], 160),
            status: PaseoAgentStatus::from_cli(string_at(item, &["status", "state"]).as_deref()),
            provider,
            workspace,
            created_at,
            updated_at,
            last_activity_at,
            labels: labels_at(item),
            missing_fields,
        });
    }
    Ok(ParsedPaseo {
        data: agents,
        source_format: "json",
        parser_version: JSON_PARSER_VERSION,
        missing_fields: response_missing,
        warnings: Vec::new(),
        truncated,
    })
}

fn is_supported_activity_text_version(version: &str) -> bool {
    let mut parts = version.trim_start_matches('v').split('.');
    matches!(parts.next(), Some("0"))
        && matches!(parts.next(), Some("2" | "3"))
        && parts
            .next()
            .is_some_and(|patch| !patch.is_empty() && patch.chars().all(|ch| ch.is_ascii_digit()))
        && parts.next().is_none()
}

pub fn parse_permissions(
    raw: &str,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<PermissionSummary>>, PaseoError> {
    let root: Value = parse_json(raw, truncated)?;
    let items = root
        .as_array()
        .or_else(|| root.get("permissions").and_then(Value::as_array))
        .ok_or_else(|| parse_error("Permission response must contain an array", truncated))?;
    let mut permissions = Vec::with_capacity(items.len());
    let mut missing_fields = Vec::new();
    for item in items {
        let permission_type = string_at(item, &["type", "permissionType", "permission"])
            .unwrap_or_else(|| "unknown".into());
        let requested_at = string_at(item, &["requestedAt", "createdAt", "created"]);
        if requested_at.is_none() && !missing_fields.contains(&"requested_at".to_string()) {
            missing_fields.push("requested_at".into());
        }
        let summary = string_at(item, &["summary", "description", "command", "request"])
            .unwrap_or_else(|| permission_type.clone());
        let request_id = safe_string_at(item, &["requestId", "request_id", "id"], 128);
        if request_id.is_none() && !missing_fields.contains(&"request_id".to_string()) {
            missing_fields.push("request_id".into());
        }
        permissions.push(PermissionSummary {
            request_id,
            agent_id: safe_string_at(item, &["agentId", "agent_id", "agent"], 128),
            permission_type: bounded(&redact(&permission_type), 120),
            requested_at,
            summary: bounded(&redact(&summary), 500),
        });
    }
    Ok(ParsedPaseo {
        data: permissions,
        source_format: "json",
        parser_version: JSON_PARSER_VERSION,
        missing_fields,
        warnings: Vec::new(),
        truncated,
    })
}

pub fn parse_activity(
    raw: &str,
    cli_version: &str,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<ActivityEvent>>, PaseoError> {
    if let Ok(root) = serde_json::from_str::<Value>(raw) {
        return parse_activity_json(&root, truncated);
    }
    if !is_supported_activity_text_version(cli_version) {
        return Err(PaseoError::new(
            "PASEO_VERSION_UNSUPPORTED",
            "This Paseo CLI activity text format is not supported; install a 0.2.x or 0.3.x release, or update the integration parser.",
            false,
            "parse_activity",
            serde_json::json!({"cli_version": cli_version, "supported": "0.2.x-0.3.x"}),
        ));
    }
    parse_activity_text(raw, truncated)
}

fn parse_activity_json(
    root: &Value,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<ActivityEvent>>, PaseoError> {
    let items = root
        .as_array()
        .or_else(|| root.get("events").and_then(Value::as_array))
        .or_else(|| root.get("activity").and_then(Value::as_array))
        .or_else(|| root.get("entries").and_then(Value::as_array))
        .or_else(|| root.get("logs").and_then(Value::as_array))
        .or_else(|| root.get("data").and_then(Value::as_array))
        .ok_or_else(|| unknown_activity("Activity JSON does not contain a known event array", truncated, 0))?;
    let mut events = Vec::with_capacity(items.len());
    let mut skipped_records = 0_usize;
    let mut missing_fields = Vec::new();
    for item in items {
        if let Some(line) = item.as_str() {
            if let Some(captures) = text_event_pattern().captures(line) {
                events.push(normalized_event(
                    captures.get(1).map_or("unknown", |value| value.as_str()),
                    captures.get(2).map_or("", |value| value.as_str()),
                    None,
                ));
                add_missing_field(&mut missing_fields, "event_timestamps");
            } else {
                skipped_records += 1;
            }
            continue;
        }

        let event_type = string_at(item, &["type", "eventType", "kind", "role"]);
        let raw_summary = string_at(item, &["summary", "message", "text", "output", "content"]);
        if event_type.is_none() && raw_summary.as_deref().is_none_or(str::is_empty) {
            skipped_records += 1;
            continue;
        }
        let occurred_at = string_at(item, &["timestamp", "createdAt", "created_at", "at"]);
        if event_type.is_none() {
            add_missing_field(&mut missing_fields, "event_type");
        }
        if occurred_at.is_none() {
            add_missing_field(&mut missing_fields, "event_timestamps");
        }
        events.push(normalized_event(
            event_type.as_deref().unwrap_or("unknown"),
            raw_summary.as_deref().unwrap_or(""),
            occurred_at,
        ));
    }
    if events.is_empty() && !items.is_empty() {
        return Err(unknown_activity(
            "Paseo returned non-empty activity JSON with no recognized records",
            truncated,
            skipped_records,
        ));
    }
    let warnings = partial_warnings(skipped_records, truncated);
    Ok(ParsedPaseo {
        data: events,
        source_format: "json",
        parser_version: JSON_PARSER_VERSION,
        missing_fields,
        warnings,
        truncated,
    })
}

fn parse_activity_text(
    raw: &str,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<ActivityEvent>>, PaseoError> {
    let normalized = raw.replace("\r\n", "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("No activity to display.") {
        return Ok(ParsedPaseo {
            data: Vec::new(),
            source_format: "text_fallback",
            parser_version: TEXT_PARSER_VERSION,
            missing_fields: vec!["event_timestamps".into()],
            warnings: partial_warnings(0, truncated),
            truncated,
        });
    }

    let mut events = Vec::new();
    let mut current_type: Option<String> = None;
    let mut current_summary = String::new();
    let mut skipped_lines = 0_usize;

    let flush = |events: &mut Vec<ActivityEvent>,
                 current_type: &mut Option<String>,
                 current_summary: &mut String| {
        if let Some(event_type) = current_type.take() {
            events.push(normalized_event(&event_type, current_summary.trim(), None));
            current_summary.clear();
        }
    };

    for line in normalized.lines() {
        let line = line.trim_end();
        if line.trim() == "---" {
            flush(&mut events, &mut current_type, &mut current_summary);
            continue;
        }

        if let Some(captures) = text_event_pattern().captures(line) {
            flush(&mut events, &mut current_type, &mut current_summary);
            current_type = Some(
                captures
                    .get(1)
                    .map_or("unknown", |value| value.as_str())
                    .to_string(),
            );
            current_summary.push_str(captures.get(2).map_or("", |value| value.as_str()));
            continue;
        }

        if line.trim().is_empty() {
            if current_type.is_some() && !current_summary.is_empty() {
                current_summary.push('\n');
            }
            continue;
        }

        if current_type.is_none() {
            skipped_lines += 1;
            continue;
        }
        if !current_summary.is_empty() {
            current_summary.push('\n');
        }
        current_summary.push_str(line);
    }

    flush(&mut events, &mut current_type, &mut current_summary);
    if events.is_empty() {
        return Err(unknown_activity(
            "Paseo activity text contained no recognized records",
            truncated,
            skipped_lines,
        ));
    }
    Ok(ParsedPaseo {
        data: events,
        source_format: "text_fallback",
        parser_version: TEXT_PARSER_VERSION,
        missing_fields: vec!["event_timestamps".into()],
        warnings: partial_warnings(skipped_lines, truncated),
        truncated,
    })
}

fn add_missing_field(fields: &mut Vec<String>, field: &str) {
    if !fields.iter().any(|value| value == field) {
        fields.push(field.into());
    }
}

fn partial_warnings(skipped_records: usize, truncated: bool) -> Vec<Value> {
    if skipped_records == 0 && !truncated {
        return Vec::new();
    }
    vec![serde_json::json!({
        "code": "PASEO_PARSE_PARTIAL",
        "message": "Paseo activity was only partially parsed; recognized events were preserved.",
        "retryable": true,
        "details": {
            "skipped_records": skipped_records,
            "truncated": truncated
        }
    })]
}

fn unknown_activity(message: &str, truncated: bool, skipped_records: usize) -> PaseoError {
    PaseoError::new(
        if truncated {
            "PASEO_PARSE_PARTIAL"
        } else {
            "PASEO_UNKNOWN_ACTIVITY"
        },
        message,
        truncated,
        "parse_activity",
        serde_json::json!({
            "skipped_records": skipped_records,
            "truncated": truncated
        }),
    )
}

fn normalized_event(
    event_type: &str,
    raw_summary: &str,
    occurred_at: Option<String>,
) -> ActivityEvent {
    let event_type = normalize_event_type(event_type);
    let raw_lower = raw_summary.to_ascii_lowercase();
    let lower = format!("{} {}", event_type, raw_lower);
    let is_error = event_type == "errors"
        || ["error", "failed", "panic", "exception"]
            .iter()
            .any(|needle| lower.contains(needle));
    let is_waiting = [
        "waiting for user",
        "awaiting input",
        "permission",
        "needs approval",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    let error_signature = is_error.then(|| normalize_error_signature(raw_summary));
    let summary = safe_activity_summary(
        event_type,
        &raw_lower,
        is_waiting,
        error_signature.as_deref(),
    );
    ActivityEvent {
        event_type: event_type.to_string(),
        occurred_at,
        summary: summary.clone(),
        is_progress: !summary.is_empty() && !is_error && !is_waiting,
        is_waiting,
        error_signature,
    }
}

fn safe_activity_summary(
    event_type: &str,
    raw_lower: &str,
    is_waiting: bool,
    error_signature: Option<&str>,
) -> String {
    if event_type == "errors" {
        return format!(
            "error activity observed: {}",
            error_signature.unwrap_or("err-unknown")
        );
    }
    if event_type == "tools" {
        let tool = [
            "cargo", "npm", "pnpm", "pytest", "python", "git", "build", "test", "download",
            "install", "compile",
        ]
        .iter()
        .find(|needle| raw_lower.contains(**needle))
        .copied()
        .unwrap_or("tool");
        return format!("tool activity observed: {tool}");
    }
    if event_type == "permissions" {
        return "permission activity observed".into();
    }
    if is_waiting {
        return "message indicates waiting for user input".into();
    }
    if ["completed", "finished", "all tests pass", "task is done"]
        .iter()
        .any(|needle| raw_lower.contains(needle))
    {
        return "message indicates completion".into();
    }
    if [
        "remaining",
        "unfinished",
        "next step",
        "blocked",
        "cannot continue",
        "still need",
    ]
    .iter()
    .any(|needle| raw_lower.contains(needle))
    {
        return "message indicates unfinished work".into();
    }
    "message activity observed".into()
}

fn normalize_event_type(value: &str) -> &'static str {
    let value = value.to_ascii_lowercase();
    if value.contains("error") || value.contains("fail") {
        "errors"
    } else if value.contains("tool")
        || value.contains("command")
        || value.contains("shell")
        || value.contains("edit")
    {
        "tools"
    } else if value.contains("permission") {
        "permissions"
    } else {
        "messages"
    }
}

fn parse_json(raw: &str, truncated: bool) -> Result<Value, PaseoError> {
    serde_json::from_str(raw).map_err(|error| {
        if truncated {
            PaseoError::new(
                "PASEO_OUTPUT_LIMIT",
                "Paseo JSON output exceeded the configured limit.",
                true,
                "parse",
                serde_json::json!({}),
            )
        } else {
            PaseoError::new(
                "PASEO_PARSE_ERROR",
                "Paseo returned an unsupported response format.",
                false,
                "parse",
                serde_json::json!({"reason": bounded(&error.to_string(), 240)}),
            )
        }
    })
}

fn parse_error(message: &str, truncated: bool) -> PaseoError {
    PaseoError::new(
        if truncated {
            "PASEO_OUTPUT_LIMIT"
        } else {
            "PASEO_PARSE_ERROR"
        },
        message,
        truncated,
        "parse",
        serde_json::json!({}),
    )
}

fn string_at(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value.get(*key).and_then(|entry| match entry {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
    })
}

fn safe_string_at(value: &Value, keys: &[&str], max_chars: usize) -> Option<String> {
    string_at(value, keys).map(|value| bounded(&redact(&value), max_chars))
}

fn labels_at(value: &Value) -> Vec<String> {
    match value.get("labels") {
        Some(Value::Array(labels)) => labels
            .iter()
            .filter_map(Value::as_str)
            .map(|label| bounded(&redact(label), 120))
            .collect(),
        Some(Value::Object(labels)) => labels
            .iter()
            .map(|(key, value)| {
                format!(
                    "{}={}",
                    bounded(key, 60),
                    bounded(&redact(value.as_str().unwrap_or("")), 60)
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn text_event_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?s)^\[([A-Za-z0-9_-]{1,32})\]\s*(.*)$")
            .expect("activity text regex")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_and_legacy_agent_fixtures() {
        let current = include_str!("../../../tests/fixtures/paseo/agents-v0.2.2.json");
        let legacy = include_str!("../../../tests/fixtures/paseo/agents-legacy.json");
        assert_eq!(parse_agents(current, false).unwrap().data.len(), 2);
        assert_eq!(
            parse_agents(legacy, false).unwrap().data[0].status,
            PaseoAgentStatus::Unknown
        );
    }

    #[test]
    fn missing_optional_fields_are_reported() {
        let parsed = parse_agents(r#"[{"id":"abc","status":"idle"}]"#, false).unwrap();
        assert!(parsed.missing_fields.contains(&"provider".into()));
    }

    #[test]
    fn malformed_json_is_structured_error() {
        assert_eq!(
            parse_agents("not-json", false).unwrap_err().code,
            "PASEO_PARSE_ERROR"
        );
        assert_eq!(
            parse_agents("{", true).unwrap_err().code,
            "PASEO_OUTPUT_LIMIT"
        );
    }

    #[test]
    fn parses_text_activity_and_redacts_secrets() {
        let fixture = include_str!("../../../tests/fixtures/paseo/activity-v0.2.2.txt");
        let parsed = parse_activity(fixture, "0.2.2", false).unwrap();
        assert_eq!(parsed.source_format, "text_fallback");
        assert!(parsed
            .data
            .iter()
            .all(|event| !event.summary.contains("secret-value")));
        assert!(parsed
            .data
            .iter()
            .all(|event| !event.summary.contains("cargo test returned")));
    }

    #[test]
    fn parses_v025_line_activity_and_multiline_messages() {
        let fixture = include_str!("../../../tests/fixtures/paseo/activity-v0.2.5.txt");
        let parsed = parse_activity(fixture, "0.2.5", false).unwrap();
        assert_eq!(parsed.data.len(), 4);
        assert_eq!(parsed.data[0].event_type, "tools");
        assert_eq!(parsed.data[1].event_type, "messages");
        assert_eq!(parsed.data[2].event_type, "tools");
        assert!(parsed
            .data
            .iter()
            .all(|event| !event.summary.contains("secret-value")));
    }

    #[test]
    fn empty_v025_activity_is_a_valid_empty_timeline() {
        let parsed = parse_activity("No activity to display.\r\n", "0.2.5", false).unwrap();
        assert!(parsed.data.is_empty());
        assert_eq!(parsed.parser_version, TEXT_PARSER_VERSION);
    }

    #[test]
    fn text_activity_requires_a_supported_minor_version_and_known_shape() {
        let fixture = include_str!("../../../tests/fixtures/paseo/activity-v0.2.2.txt");
        assert!(parse_activity(fixture, "0.2.3", false).is_ok());
        assert_eq!(
            parse_activity(fixture, "0.4.0", false).unwrap_err().code,
            "PASEO_VERSION_UNSUPPORTED"
        );
        assert_eq!(
            parse_activity("unstructured output", "0.2.2", false)
                .unwrap_err()
                .code,
            "PASEO_UNKNOWN_ACTIVITY"
        );
    }

    #[test]
    fn partial_activity_preserves_known_records_and_reports_a_warning() {
        let parsed = parse_activity(
            "Paseo activity follows\n[Shell2] cargo test\n[Assistant] done",
            "0.2.5",
            false,
        )
        .unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert_eq!(parsed.warnings[0]["code"], "PASEO_PARSE_PARTIAL");
        assert_eq!(parsed.warnings[0]["details"]["skipped_records"], 1);
    }

    #[test]
    fn activity_json_accepts_v025_envelopes_and_string_records() {
        let fixture = include_str!("../../../tests/fixtures/paseo/activity-v0.2.5.json");
        let parsed = parse_activity(fixture, "0.2.5", false).unwrap();
        assert_eq!(parsed.data.len(), 2);
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn parses_v030_line_activity_from_real_cli_shape() {
        let fixture = include_str!("../../../tests/fixtures/paseo/activity-v0.3.0.txt");
        let parsed = parse_activity(fixture, "0.3.0", false).unwrap();
        assert_eq!(parsed.data.len(), 3);
        assert_eq!(parsed.source_format, "text_fallback");
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn agent_display_metadata_is_redacted_and_bounded() {
        let long_name = format!("token=secret {}", "x".repeat(300));
        let raw = serde_json::json!([{"id": "agent-1", "name": long_name, "status": "running"}]);
        let agent = parse_agents(&raw.to_string(), false)
            .unwrap()
            .data
            .remove(0);
        let name = agent.name.unwrap();
        assert!(!name.contains("secret"));
        assert!(name.chars().count() <= 161);
    }

    #[test]
    fn parses_empty_permissions_array() {
        let fixture = include_str!("../../../tests/fixtures/paseo/permissions-v0.2.2.json");
        assert!(parse_permissions(fixture, false).unwrap().data.is_empty());
    }

    #[test]
    fn permission_request_ids_are_preserved() {
        let parsed = parse_permissions(
            r#"[{"id":"req-123","agentId":"agent-1","type":"shell","summary":"cargo test"}]"#,
            false,
        )
        .unwrap();
        assert_eq!(parsed.data[0].request_id.as_deref(), Some("req-123"));
        assert!(!parsed.missing_fields.contains(&"request_id".into()));
    }
}
