use serde_json::{json, Map, Value};

use super::model::{ParsedPaseo, PaseoError, PermissionSummary};
use super::redaction::{bounded, redact};

pub const PERMISSION_ADAPTER_VERSION: &str = "paseo-permission-adapter-v2";

pub fn parse_permission_listing(
    raw: &str,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<PermissionSummary>>, PaseoError> {
    let root = parse_json(raw, truncated)?;
    let items = array_at(
        &root,
        &["permissions", "pendingPermissions", "PendingPermissions"],
    )
    .ok_or_else(|| schema_error("Permission response must contain an array", truncated))?;
    let mut parsed = ParsedPaseo {
        data: Vec::with_capacity(items.len()),
        source_format: "json",
        parser_version: PERMISSION_ADAPTER_VERSION,
        missing_fields: Vec::new(),
        warnings: Vec::new(),
        truncated,
    };
    let mut malformed = 0_usize;
    let mut unverified = 0_usize;
    for item in items {
        let Some(object) = item.as_object() else {
            malformed += 1;
            continue;
        };
        let agent_id = safe_identifier(string_at(object, &["agent_id", "agentId", "agent"]));
        let request_id = safe_identifier(string_at(object, &["request_id", "requestId"]));
        let tool = permission_tool(object);
        if agent_id.is_none() && request_id.is_none() && tool.is_none() {
            malformed += 1;
            continue;
        }
        let control_safe = agent_id.is_some() && request_id.is_some();
        if !control_safe {
            unverified += 1;
        }
        let permission_type = permission_type(object, tool.as_deref());
        parsed.data.push(PermissionSummary {
            request_id,
            agent_id,
            tool,
            permission_type: permission_type.clone(),
            requested_at: safe_text(
                string_at(
                    object,
                    &[
                        "requested_at",
                        "requestedAt",
                        "created_at",
                        "createdAt",
                        "created",
                    ],
                ),
                80,
            ),
            summary: safe_text(
                string_at(object, &["summary", "description", "command", "request"]),
                500,
            )
            .unwrap_or(permission_type),
            control_safe,
            source: "permission_list".into(),
        });
    }
    add_permission_warnings(&mut parsed, malformed, unverified);
    Ok(parsed)
}

pub fn parse_inspected_permissions(
    raw: &str,
    expected_agent_id: &str,
    truncated: bool,
) -> Result<ParsedPaseo<Vec<PermissionSummary>>, PaseoError> {
    let root = parse_json(raw, truncated)?;
    let object = root
        .as_object()
        .ok_or_else(|| schema_error("Agent inspection response must be an object", truncated))?;
    let agent_id = safe_identifier(string_at(object, &["agent_id", "agentId", "id", "Id"]));
    if agent_id.as_deref() != Some(expected_agent_id) {
        return Err(schema_error(
            "Agent inspection did not return the exact requested agent ID",
            truncated,
        ));
    }
    let items = array_at(
        &root,
        &[
            "pending_permissions",
            "pendingPermissions",
            "PendingPermissions",
        ],
    )
    .ok_or_else(|| {
        schema_error(
            "Agent inspection has no pending permission array",
            truncated,
        )
    })?;
    let mut parsed = ParsedPaseo {
        data: Vec::with_capacity(items.len()),
        source_format: "json",
        parser_version: PERMISSION_ADAPTER_VERSION,
        missing_fields: Vec::new(),
        warnings: Vec::new(),
        truncated,
    };
    let mut malformed = 0_usize;
    let mut unverified = 0_usize;
    for item in items {
        let Some(permission) = item.as_object() else {
            malformed += 1;
            continue;
        };
        let request_id = safe_identifier(string_at(
            permission,
            &["request_id", "requestId", "id", "Id"],
        ));
        let tool = permission_tool(permission);
        if request_id.is_none() {
            unverified += 1;
        }
        if request_id.is_none() && tool.is_none() {
            malformed += 1;
            continue;
        }
        let permission_type = permission_type(permission, tool.as_deref());
        parsed.data.push(PermissionSummary {
            request_id: request_id.clone(),
            agent_id: agent_id.clone(),
            tool,
            permission_type: permission_type.clone(),
            requested_at: safe_text(
                string_at(
                    permission,
                    &[
                        "requested_at",
                        "requestedAt",
                        "created_at",
                        "createdAt",
                        "created",
                    ],
                ),
                80,
            ),
            summary: safe_text(
                string_at(
                    permission,
                    &["summary", "description", "command", "request"],
                ),
                500,
            )
            .unwrap_or(permission_type),
            control_safe: request_id.is_some(),
            source: "agent_inspect".into(),
        });
    }
    add_permission_warnings(&mut parsed, malformed, unverified);
    Ok(parsed)
}

pub fn replace_agent_permissions(
    listing: &mut ParsedPaseo<Vec<PermissionSummary>>,
    agent_id: &str,
    inspected: ParsedPaseo<Vec<PermissionSummary>>,
) {
    listing
        .data
        .retain(|permission| permission.agent_id.as_deref() != Some(agent_id));
    listing.data.extend(inspected.data);
    listing.warnings.extend(inspected.warnings);
    if listing
        .data
        .iter()
        .all(|permission| permission.control_safe)
    {
        listing.warnings.retain(|warning| {
            warning.get("code").and_then(Value::as_str)
                != Some("PASEO_PERMISSION_SCHEMA_UNSUPPORTED")
        });
        listing
            .missing_fields
            .retain(|field| field != "request_id" && field != "agent_id");
    }
}

pub fn permission_warning(error: &PaseoError, agent_id: &str) -> Value {
    json!({
        "code": "PASEO_PERMISSION_SCHEMA_UNSUPPORTED",
        "message": "Pending permission details could not be verified for control operations.",
        "retryable": error.retryable,
        "details": {
            "agent_id": agent_id,
            "cause": error.code,
            "stage": error.stage
        }
    })
}

fn add_permission_warnings(
    parsed: &mut ParsedPaseo<Vec<PermissionSummary>>,
    malformed: usize,
    unverified: usize,
) {
    if malformed > 0 {
        parsed.warnings.push(json!({
            "code": "PASEO_PARSE_PARTIAL",
            "message": "Malformed permission records were skipped.",
            "retryable": false,
            "details": {"skipped_records": malformed}
        }));
    }
    if unverified > 0 {
        add_missing(&mut parsed.missing_fields, "request_id");
        parsed.warnings.push(json!({
            "code": "PASEO_PERMISSION_SCHEMA_UNSUPPORTED",
            "message": "The permission listing did not provide complete safety-critical identifiers; control operations are disabled for these records.",
            "retryable": false,
            "details": {"unverified_records": unverified}
        }));
    }
}

fn permission_tool(object: &Map<String, Value>) -> Option<String> {
    safe_text(
        string_at(object, &["tool", "kind", "name"]).or_else(|| {
            object
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("permission"))
                .and_then(|(_, value)| value.as_object())
                .and_then(|permission| string_at(permission, &["tool", "kind", "name"]))
        }),
        120,
    )
}

fn permission_type(object: &Map<String, Value>, tool: Option<&str>) -> String {
    safe_text(
        string_at(object, &["permission_type", "permissionType", "type"]),
        120,
    )
    .or_else(|| tool.map(str::to_string))
    .unwrap_or_else(|| "unknown".into())
}

fn parse_json(raw: &str, truncated: bool) -> Result<Value, PaseoError> {
    serde_json::from_str(raw).map_err(|error| {
        if truncated {
            PaseoError::new(
                "PASEO_OUTPUT_LIMIT",
                "Paseo permission output exceeded the configured limit.",
                true,
                "parse_permissions",
                json!({}),
            )
        } else {
            PaseoError::new(
                "PASEO_PERMISSION_SCHEMA_UNSUPPORTED",
                "Paseo returned malformed permission JSON.",
                false,
                "parse_permissions",
                json!({"reason": bounded(&error.to_string(), 240)}),
            )
        }
    })
}

fn schema_error(message: &str, truncated: bool) -> PaseoError {
    PaseoError::new(
        if truncated {
            "PASEO_OUTPUT_LIMIT"
        } else {
            "PASEO_PERMISSION_SCHEMA_UNSUPPORTED"
        },
        message,
        truncated,
        "parse_permissions",
        json!({}),
    )
}

fn array_at<'a>(root: &'a Value, keys: &[&str]) -> Option<&'a Vec<Value>> {
    root.as_array().or_else(|| {
        root.as_object().and_then(|object| {
            object
                .iter()
                .find(|(key, _)| {
                    keys.iter()
                        .any(|candidate| key.eq_ignore_ascii_case(candidate))
                })
                .and_then(|(_, value)| value.as_array())
        })
    })
}

fn string_at(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    object.iter().find_map(|(key, value)| {
        keys.iter()
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
            .then(|| match value {
                Value::String(value) => Some(value.clone()),
                Value::Number(value) => Some(value.to_string()),
                _ => None,
            })
            .flatten()
    })
}

fn safe_identifier(value: Option<String>) -> Option<String> {
    value.filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && !value.starts_with('-')
            && value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | ':' | '-'))
    })
}

fn safe_text(value: Option<String>, limit: usize) -> Option<String> {
    value.map(|value| bounded(&redact(&value), limit))
}

fn add_missing(fields: &mut Vec<String>, field: &str) {
    if !fields.iter().any(|value| value == field) {
        fields.push(field.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v031_inspection_preserves_full_ids_and_ignores_future_fields() {
        let fixture = include_str!("../../../tests/fixtures/paseo/permissions-v0.3.1-inspect.json");
        let parsed =
            parse_inspected_permissions(fixture, "37c6d4c7-8092-4939-8ef4-178b63879ca2", false)
                .unwrap();
        assert_eq!(
            parsed.data[0].request_id.as_deref(),
            Some("permission-174b288d-cc76-499e-bf68-9fb7b968f4e9")
        );
        assert_eq!(parsed.data[0].tool.as_deref(), Some("Write"));
        assert!(parsed.data[0].control_safe);
    }

    #[test]
    fn field_order_and_nested_permission_tool_are_tolerated() {
        let raw = r#"{
          "PendingPermissions":[{"future":1,"permission":{"tool":"Read"},"requestId":"permission-full"}],
          "Id":"agent-full"
        }"#;
        let parsed = parse_inspected_permissions(raw, "agent-full", false).unwrap();
        assert_eq!(parsed.data[0].tool.as_deref(), Some("Read"));
        assert!(parsed.data[0].control_safe);
    }

    #[test]
    fn malformed_and_missing_ids_degrade_without_guessing() {
        let parsed = parse_permission_listing(
            r#"[{"agentId":"agent-full","name":"Write"},42,{"future":true}]"#,
            false,
        )
        .unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].request_id, None);
        assert!(!parsed.data[0].control_safe);
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning["code"] == "PASEO_PARSE_PARTIAL"));
        assert!(parsed
            .warnings
            .iter()
            .any(|warning| warning["code"] == "PASEO_PERMISSION_SCHEMA_UNSUPPORTED"));
    }
}
