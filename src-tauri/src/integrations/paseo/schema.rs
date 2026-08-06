use serde_json::{json, Value};

use super::model::PASEO_ALL_TOOLS;

pub fn definition(name: &str) -> Option<Value> {
    if !PASEO_ALL_TOOLS.contains(&name) {
        return None;
    }
    let (title, description, read_only, destructive) = match name {
        "paseo_health" => ("Paseo health", "Check the configured Paseo CLI and daemon without exposing host credentials.", true, false),
        "paseo_list_agents" => ("List Paseo agents", "List and bounded-filter Paseo agents across configured workspaces.", true, false),
        "paseo_get_agent_activity" => ("Paseo agent activity", "Return a bounded, redacted recent activity timeline without streaming.", true, false),
        "paseo_list_pending_permissions" => ("Paseo pending permissions", "Read pending Paseo permission requests and their exact request IDs.", true, false),
        "paseo_diagnose_agent" => ("Diagnose Paseo agent", "Apply deterministic rules to status, activity, errors, permissions, and prior monitoring evidence.", true, false),
        "paseo_monitor_snapshot" => ("Paseo monitor snapshot", "Create one low-overhead, redacted monitoring snapshot and compare it with the previous snapshot.", true, false),
        "paseo_send_agent_prompt" => ("Send Paseo agent prompt", "Queue a bounded text prompt for an existing Paseo agent in Assist or Control mode.", false, false),
        "paseo_stop_agent" => ("Stop Paseo agent", "Interrupt the current run of an existing Paseo agent after explicit confirmation.", false, true),
        "paseo_allow_permission" => ("Allow Paseo permission", "Allow one exact pending permission request after explicit confirmation.", false, true),
        "paseo_deny_permission" => ("Deny Paseo permission", "Deny one exact pending permission request after explicit confirmation without interrupting the agent.", false, true),
        "paseo_create_agent" => ("Create Paseo agent", "Create one background Paseo agent confined to the current workspace after explicit confirmation.", false, false),
        _ => return None,
    };
    Some(json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input_schema(name),
        "annotations": {
            "title": title,
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "idempotentHint": read_only,
            "openWorldHint": true
        }
    }))
}

pub fn input_schema(name: &str) -> Value {
    match name {
        "paseo_health" => json!({
            "type": "object",
            "properties": {"refresh": {"type": "boolean", "default": false}},
            "additionalProperties": false
        }),
        "paseo_allow_permission" | "paseo_deny_permission" => json!({
            "type": "object",
            "required": ["agent_id", "request_id", "confirm"],
            "properties": {
                "agent_id": {"type": "string", "minLength": 1, "maxLength": 128},
                "request_id": {"type": "string", "minLength": 1, "maxLength": 128},
                "confirm": {"type": "boolean"},
                "reason": {"type": "string", "maxLength": 500, "default": ""}
            },
            "additionalProperties": false
        }),
        "paseo_create_agent" => json!({
            "type": "object",
            "required": ["prompt", "provider", "confirm"],
            "properties": {
                "prompt": {"type": "string", "minLength": 1, "maxLength": 8000},
                "confirm": {"type": "boolean"},
                "title": {"type": ["string", "null"], "maxLength": 160},
                "provider": {"type": "string", "minLength": 1, "maxLength": 120},
                "cwd": {"type": ["string", "null"], "maxLength": 4096},
                "reason": {"type": "string", "maxLength": 500, "default": ""}
            },
            "additionalProperties": false
        }),
        "paseo_list_agents" => json!({
            "type": "object",
            "properties": {
                "include_completed": {"type": "boolean", "default": true},
                "all_directories": {"type": "boolean", "default": true},
                "workspace_path": {"type": ["string", "null"], "maxLength": 4096},
                "status": {"type": "array", "items": {"type": "string"}, "maxItems": 10, "default": []},
                "name_patterns": {"type": "array", "items": {"type": "string", "maxLength": 256}, "maxItems": 20, "default": []},
                "labels": {"type": "array", "items": {"type": "string", "maxLength": 120}, "maxItems": 20, "default": []},
                "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 100}
            },
            "additionalProperties": false
        }),
        "paseo_get_agent_activity" => json!({
            "type": "object",
            "required": ["agent_id"],
            "properties": {
                "agent_id": {"type": "string", "minLength": 1, "maxLength": 128},
                "tail": {"type": "integer", "minimum": 1, "maximum": 100, "default": 30},
                "filter": {"type": "string", "enum": ["all", "messages", "tools", "errors"], "default": "all"},
                "max_bytes": {"type": "integer", "minimum": 1024, "maximum": 262144, "default": 65536}
            },
            "additionalProperties": false
        }),
        "paseo_list_pending_permissions" => json!({
            "type": "object",
            "properties": {"agent_id": {"type": ["string", "null"], "maxLength": 128}},
            "additionalProperties": false
        }),
        "paseo_diagnose_agent" => json!({
            "type": "object",
            "required": ["agent_id"],
            "properties": {
                "agent_id": {"type": "string", "minLength": 1, "maxLength": 128},
                "stalled_after_minutes": {"type": ["integer", "null"], "minimum": 1, "maximum": 240},
                "repeat_error_threshold": {"type": ["integer", "null"], "minimum": 1, "maximum": 10},
                "activity_tail": {"type": ["integer", "null"], "minimum": 1, "maximum": 100}
            },
            "additionalProperties": false
        }),
        "paseo_monitor_snapshot" => json!({
            "type": "object",
            "properties": {
                "include_healthy": {"type": "boolean", "default": false},
                "compare_previous": {"type": "boolean", "default": true},
                "max_agents": {"type": "integer", "minimum": 1, "maximum": 100, "default": 100},
                "refresh": {"type": "boolean", "default": true}
            },
            "additionalProperties": false
        }),
        "paseo_send_agent_prompt" => json!({
            "type": "object",
            "required": ["agent_id", "prompt"],
            "properties": {
                "agent_id": {"type": "string", "minLength": 1, "maxLength": 128},
                "prompt": {"type": "string", "minLength": 1, "maxLength": 8000},
                "no_wait": {"type": "boolean", "const": true, "default": true},
                "reason": {"type": "string", "maxLength": 500, "default": ""}
            },
            "additionalProperties": false
        }),
        "paseo_stop_agent" => json!({
            "type": "object",
            "required": ["agent_id", "confirm", "reason"],
            "properties": {
                "agent_id": {"type": "string", "minLength": 1, "maxLength": 128},
                "confirm": {"type": "boolean"},
                "reason": {"type": "string", "minLength": 1, "maxLength": 500}
            },
            "additionalProperties": false
        }),
        _ => json!({"type": "object", "properties": {}, "additionalProperties": false}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_match_side_effects() {
        let send = definition("paseo_send_agent_prompt").unwrap();
        let stop = definition("paseo_stop_agent").unwrap();
        let allow = definition("paseo_allow_permission").unwrap();
        let create = definition("paseo_create_agent").unwrap();
        assert_eq!(send["annotations"]["readOnlyHint"], false);
        assert_eq!(send["annotations"]["destructiveHint"], false);
        assert_eq!(stop["annotations"]["destructiveHint"], true);
        assert_eq!(allow["annotations"]["destructiveHint"], true);
        assert_eq!(create["annotations"]["readOnlyHint"], false);
        assert_eq!(create["annotations"]["destructiveHint"], false);
    }
}
