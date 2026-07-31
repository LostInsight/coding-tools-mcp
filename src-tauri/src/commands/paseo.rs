use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::integrations::paseo::cli::PaseoCliClient;
use crate::integrations::paseo::client::PaseoClient;
use crate::integrations::paseo::model::PaseoRuntimeContext;
use crate::integrations::paseo::monitor_store::PaseoMonitorStore;
use crate::integrations::paseo::policy::validate_host;
use crate::integrations::paseo::redaction::redact_host;
use crate::integrations::paseo::tools::test_health;
use crate::integrations::paseo::PaseoIntegrationConfig;

#[derive(Debug, Clone, Serialize)]
pub struct PaseoIntegrationSettingsDto {
    pub config: PaseoIntegrationConfig,
    pub connection_type: String,
    pub host_configured: bool,
    pub host_display: String,
    pub host: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaseoIntegrationSettingsInput {
    pub config: PaseoIntegrationConfig,
    /// `None` retains the stored host; an empty string clears it.
    pub host: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaseoFilterAgentDto {
    pub id: String,
    pub name: Option<String>,
    pub workspace: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PaseoFilterOptionsDto {
    pub agents: Vec<PaseoFilterAgentDto>,
    pub workspaces: Vec<String>,
}

#[tauri::command]
pub fn get_paseo_integration_settings(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<PaseoIntegrationSettingsDto> {
    state.with_data(|store| {
        let profile = store
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("workspace not found: {id}")))?;
        let host = if profile.integrations.paseo.host_configured {
            store.get_workspace_secret(&id, "paseo_host")?
        } else {
            None
        };
        Ok(settings_dto(profile.integrations.paseo, host))
    })
}

#[tauri::command]
pub fn save_paseo_integration_settings(
    state: State<'_, AppState>,
    id: String,
    input: PaseoIntegrationSettingsInput,
) -> AppResult<PaseoIntegrationSettingsDto> {
    input.config.validate().map_err(paseo_app_error)?;
    state.with_data(|store| {
        let mut profile = store
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("workspace not found: {id}")))?;
        let current_host = store.get_workspace_secret(&id, "paseo_host")?;
        let host = match input.host {
            Some(value) => {
                let value = value.trim().to_string();
                validate_host((!value.is_empty()).then_some(value.as_str()))
                    .map_err(paseo_app_error)?;
                store.set_workspace_secret(&id, "paseo_host", &value)?;
                (!value.is_empty()).then_some(value)
            }
            None => current_host,
        };
        let mut config = input.config;
        config.host_configured = host.is_some();
        profile.integrations.paseo = config.clone();
        store.update(profile)?;
        Ok(settings_dto(config, host))
    })
}

#[tauri::command]
pub async fn test_paseo_connection(
    state: State<'_, AppState>,
    id: String,
    input: PaseoIntegrationSettingsInput,
) -> AppResult<Value> {
    input.config.validate().map_err(paseo_app_error)?;
    let (workspace_path, stored_host) = state.with_data(|store| {
        let profile = store
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("workspace not found: {id}")))?;
        let host = store.get_workspace_secret(&id, "paseo_host")?;
        Ok((PathBuf::from(profile.path), host))
    })?;
    let host = resolve_test_host(input.host, stored_host);
    validate_host(host.as_deref()).map_err(paseo_app_error)?;
    let mut config = input.config;
    config.host_configured = host.is_some();
    let context = PaseoRuntimeContext::new(id, workspace_path, config, host);
    tauri::async_runtime::spawn_blocking(move || test_health(&context, true))
        .await
        .map_err(|error| AppError::Message(format!("Paseo connection test worker failed: {error}")))
}

#[tauri::command]
pub async fn list_paseo_filter_options(
    state: State<'_, AppState>,
    id: String,
    input: PaseoIntegrationSettingsInput,
) -> AppResult<PaseoFilterOptionsDto> {
    input.config.validate().map_err(paseo_app_error)?;
    let (workspace_path, stored_host) = state.with_data(|store| {
        let profile = store
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("workspace not found: {id}")))?;
        let host = store.get_workspace_secret(&id, "paseo_host")?;
        Ok((PathBuf::from(profile.path), host))
    })?;
    let host = resolve_test_host(input.host, stored_host);
    validate_host(host.as_deref()).map_err(paseo_app_error)?;
    let mut config = input.config;
    config.host_configured = host.is_some();
    let context = PaseoRuntimeContext::new(id, workspace_path, config, host);
    tauri::async_runtime::spawn_blocking(move || {
        let parsed = PaseoCliClient::production(context).list_agents()?;
        let mut workspaces = parsed
            .data
            .iter()
            .filter_map(|agent| agent.workspace.clone())
            .collect::<Vec<_>>();
        workspaces.sort_by_key(|value| value.to_ascii_lowercase());
        workspaces.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        let agents = parsed
            .data
            .into_iter()
            .map(|agent| PaseoFilterAgentDto {
                id: agent.id,
                name: agent.name,
                workspace: agent.workspace,
                status: agent.status.as_str().into(),
            })
            .collect();
        Ok::<_, crate::integrations::paseo::model::PaseoError>(PaseoFilterOptionsDto {
            agents,
            workspaces,
        })
    })
    .await
    .map_err(|error| AppError::Message(format!("Paseo filter worker failed: {error}")))?
    .map_err(paseo_app_error)
}

#[tauri::command]
pub fn clear_paseo_monitor_snapshots(state: State<'_, AppState>, id: String) -> AppResult<usize> {
    state.with_workspaces(|store| {
        if store.get(&id).is_none() {
            return Err(AppError::Message(format!("workspace not found: {id}")));
        }
        Ok(())
    })?;
    PaseoMonitorStore::for_workspace(&id)
        .and_then(|store| store.clear())
        .map_err(paseo_app_error)
}

fn settings_dto(
    config: PaseoIntegrationConfig,
    host: Option<String>,
) -> PaseoIntegrationSettingsDto {
    PaseoIntegrationSettingsDto {
        connection_type: if config.host_configured {
            "remote"
        } else {
            "local"
        }
        .into(),
        host_configured: host.is_some(),
        host_display: redact_host(host.as_deref()),
        host,
        config,
    }
}

fn resolve_test_host(requested: Option<String>, stored: Option<String>) -> Option<String> {
    match requested {
        Some(value) => {
            let value = value.trim().to_string();
            (!value.is_empty()).then_some(value)
        }
        None => stored,
    }
}

fn paseo_app_error(error: crate::integrations::paseo::model::PaseoError) -> AppError {
    AppError::Message(format!("{}: {}", error.code, error.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_dto_masks_host_but_preserves_local_ui_value() {
        let config = PaseoIntegrationConfig {
            host_configured: true,
            ..PaseoIntegrationConfig::default()
        };
        let dto = settings_dto(
            config,
            Some("tcp://remote.example:6767?ssl=true&password=secret".into()),
        );
        assert!(dto.host_configured);
        assert!(!dto.host_display.contains("secret"));
        assert!(dto.host.as_deref().unwrap().contains("password=secret"));
    }

    #[test]
    fn explicit_empty_test_host_selects_local_instead_of_stored_remote() {
        assert!(resolve_test_host(Some("  ".into()), Some("remote:6767".into())).is_none());
        assert_eq!(
            resolve_test_host(None, Some("remote:6767".into())).as_deref(),
            Some("remote:6767")
        );
    }
}
