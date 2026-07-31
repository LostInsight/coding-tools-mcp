use serde_json::{json, Map, Value};

use crate::integrations::paseo::PaseoIntegrationConfig;
use crate::tools::{
    is_allowed_tool, is_allowed_tool_for_context, is_mutating_tool_for_context, MUTATING_TOOLS,
};

pub fn build_openapi(
    tools: &[Value],
    public_base_url: &str,
    auth_type: &str,
    paseo: Option<&PaseoIntegrationConfig>,
) -> Value {
    let mut paths = Map::new();
    let use_api_key = auth_type == "api_key";

    for tool in tools {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        if !paseo.map_or_else(
            || is_allowed_tool(name),
            |config| is_allowed_tool_for_context(name, config),
        ) {
            continue;
        }

        let input_schema = tool
            .get("inputSchema")
            .filter(|schema| schema.is_object())
            .cloned()
            .unwrap_or_else(|| {
                json!({
                    "type": "object",
                    "additionalProperties": true
                })
            });

        let description_raw = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Call coding tool");
        let description: String = description_raw.chars().take(700).collect();
        let summary: String = description.chars().take(300).collect();

        let mut operation = json!({
            "operationId": format!("coding_{name}"),
            "summary": summary,
            "description": description,
            "requestBody": {
                "required": false,
                "content": {
                    "application/json": {
                        "schema": input_schema
                    }
                }
            },
            "responses": {
                "200": {
                    "description": "Tool execution result",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ToolExecutionResponse" }
                        }
                    }
                },
                "400": { "description": "Invalid request or policy rejection" },
                "401": { "description": "Invalid API key" },
                "422": { "description": "Tool execution failed" },
                "502": { "description": "MCP backend failure" }
            },
            "x-openai-isConsequential": paseo.map_or_else(
                || MUTATING_TOOLS.contains(&name),
                |config| is_mutating_tool_for_context(name, config)
            )
        });

        if use_api_key {
            operation
                .as_object_mut()
                .expect("operation object")
                .insert("security".to_string(), json!([{ "bearerAuth": [] }]));
        }

        paths.insert(format!("/actions/{name}"), json!({ "post": operation }));
    }

    let mut document = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Coding Tools Actions",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Read, modify and test a workspace through coding-tools-mcp."
        },
        "servers": [{ "url": public_base_url.trim_end_matches('/') }],
        "paths": paths,
        "components": {
            "schemas": {
                "ContentPart": content_part_schema(),
                "ToolError": tool_error_schema(),
                "StructuredContent": structured_content_schema(),
                "ToolExecutionResponse": {
                    "type": "object",
                    "properties": {
                        "ok": { "type": "boolean" },
                        "tool": { "type": "string" },
                        "structured_content": { "$ref": "#/components/schemas/StructuredContent" },
                        "content": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/ContentPart" }
                        },
                        "is_error": { "type": "boolean" }
                    },
                    "required": ["ok", "tool", "is_error"],
                    "additionalProperties": true
                }
            }
        }
    });

    if use_api_key {
        document
            .as_object_mut()
            .expect("document object")
            .get_mut("components")
            .and_then(Value::as_object_mut)
            .expect("components object")
            .insert(
                "securitySchemes".to_string(),
                json!({
                    "bearerAuth": {
                        "type": "http",
                        "scheme": "bearer"
                    }
                }),
            );
    }

    document
}

fn content_part_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": { "type": "string" },
            "text": { "type": "string" },
            "mimeType": { "type": "string" },
            "data": { "type": "string" }
        },
        "required": ["type"],
        "additionalProperties": true
    })
}

fn tool_error_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "code": { "type": "string" },
            "message": { "type": "string" },
            "category": { "type": "string" },
            "retryable": { "type": "boolean" },
            "details": {
                "type": "object",
                "properties": {},
                "additionalProperties": true
            }
        },
        "additionalProperties": true
    })
}

fn structured_content_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ok": { "type": "boolean" },
            "error": tool_error_schema(),
            "diagnostics": {
                "type": "object",
                "properties": {},
                "additionalProperties": true
            },
            "permission_request": {
                "type": "object",
                "properties": {
                    "tool_name": { "type": "string" },
                    "permission": { "type": "string" },
                    "status": { "type": "string" },
                    "retryable": { "type": "boolean" }
                },
                "additionalProperties": true
            }
        },
        "additionalProperties": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn openapi_without_auth_has_no_security_scheme() {
        let tools = [json!({
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": { "type": "object" }
        })];
        let schema = build_openapi(&tools, "https://actions.example.com", "none", None);
        assert!(schema["paths"]["/actions/read_file"]["post"]["security"].is_null());
        assert!(schema["components"]["securitySchemes"].is_null());
    }

    #[test]
    fn openapi_api_key_includes_bearer_security() {
        let tools = [json!({
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": { "type": "object" }
        })];
        let schema = build_openapi(&tools, "https://actions.example.com", "api_key", None);
        assert_eq!(
            schema["components"]["securitySchemes"]["bearerAuth"]["scheme"],
            "bearer"
        );
        assert_eq!(
            schema["paths"]["/actions/read_file"]["post"]["security"],
            json!([{ "bearerAuth": [] }])
        );
    }

    #[test]
    fn core_openapi_exposes_grep_text_as_read_only() {
        let tools = crate::tools::list_tools_for_profile("core");
        let schema = build_openapi(&tools, "https://actions.example.com", "none", None);
        let operation = &schema["paths"]["/actions/grep_text"]["post"];

        assert_eq!(operation["operationId"], "coding_grep_text");
        assert_eq!(operation["x-openai-isConsequential"], false);
        assert_eq!(
            operation["requestBody"]["content"]["application/json"]["schema"],
            crate::tools::registry::input_schema("grep_text")
        );
    }

    #[test]
    fn paseo_openapi_tracks_context_and_consequential_tools() {
        let mut config = PaseoIntegrationConfig {
            enabled: true,
            access_mode: crate::integrations::paseo::config::PaseoAccessMode::Assist,
            ..PaseoIntegrationConfig::default()
        };
        let tools = crate::tools::list_tools_for_context("advanced", &config);
        let schema = build_openapi(&tools, "https://actions.example.com", "none", Some(&config));
        assert!(schema["paths"]["/actions/paseo_health"].is_object());
        assert_eq!(
            schema["paths"]["/actions/paseo_send_agent_prompt"]["post"]["x-openai-isConsequential"],
            true
        );
        assert!(schema["paths"]["/actions/paseo_stop_agent"].is_null());

        config.access_mode = crate::integrations::paseo::config::PaseoAccessMode::Control;
        let tools = crate::tools::list_tools_for_context("advanced", &config);
        let schema = build_openapi(&tools, "https://actions.example.com", "none", Some(&config));
        assert_eq!(
            schema["paths"]["/actions/paseo_stop_agent"]["post"]["x-openai-isConsequential"],
            true
        );
    }
}
