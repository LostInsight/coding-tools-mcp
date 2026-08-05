use tauri::State;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::settings::{AppSettings, FrpProfile, ProxyConfig};

const FRP_PROFILE_TOKEN_SCOPE: &str = "frp_profile_token";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrpProfileDto {
    pub id: String,
    pub name: String,
    pub server: String,
    pub server_port: u16,
    pub has_token: bool,
    pub is_default: bool,
}

fn profile_dto(profile: &FrpProfile, has_token: bool, is_default: bool) -> FrpProfileDto {
    FrpProfileDto {
        id: profile.id.clone(),
        name: profile.name.clone(),
        server: profile.server.clone(),
        server_port: profile.server_port,
        has_token,
        is_default,
    }
}

#[tauri::command]
pub fn list_frp_profiles(state: State<'_, AppState>) -> AppResult<Vec<FrpProfileDto>> {
    state.with_settings(|store| {
        Ok(store
            .data()
            .frp_profiles
            .iter()
            .map(|profile| {
                let has_token = store
                    .get_app_secret(FRP_PROFILE_TOKEN_SCOPE, &profile.id)
                    .is_some_and(|value| !value.trim().is_empty());
                profile_dto(
                    profile,
                    has_token,
                    profile.id == store.data().default_tunnel_profile_id,
                )
            })
            .collect())
    })
}

#[tauri::command]
pub fn save_frp_profile(
    state: State<'_, AppState>,
    profile: FrpProfile,
    token: Option<String>,
    make_default: Option<bool>,
) -> AppResult<FrpProfileDto> {
    if profile.name.trim().is_empty() {
        return Err(AppError::Message("FRP 配置名称不能为空。".into()));
    }

    let mut saved = profile;
    saved.name = saved.name.trim().to_string();
    saved.server = saved.server.trim().to_string();
    // Cloudflare fields can still arrive from an older bundled frontend. They
    // are migration-only and must never turn this back into a mixed profile.
    saved.cloudflare_account_id.clear();
    saved.cloudflare_tunnel_id.clear();
    saved.cloudflare_zone_id.clear();

    if saved.server.is_empty() {
        return Err(AppError::Message("FRP 服务器不能为空。".into()));
    }
    if saved.server_port == 0 {
        return Err(AppError::Message("FRP 服务器端口无效。".into()));
    }
    if saved.id.trim().is_empty() {
        saved.id = uuid::Uuid::new_v4().to_string().replace('-', "");
    }

    state.with_settings(|store| {
        let mut settings = store.settings();
        if let Some(existing) = settings
            .frp_profiles
            .iter_mut()
            .find(|item| item.id == saved.id)
        {
            *existing = saved.clone();
        } else {
            settings.frp_profiles.push(saved.clone());
        }

        let make_default =
            make_default.unwrap_or_else(|| settings.default_tunnel_profile_id.trim().is_empty());
        if make_default {
            settings.default_tunnel_profile_id = saved.id.clone();
        } else if settings.default_tunnel_profile_id == saved.id {
            settings.default_tunnel_profile_id.clear();
        }
        store.update_settings(settings)?;

        if let Some(token) = token.filter(|value| !value.trim().is_empty()) {
            store.set_app_secret(FRP_PROFILE_TOKEN_SCOPE, &saved.id, token.trim())?;
        }
        Ok(())
    })?;

    let (has_token, is_default) = state.with_settings(|store| {
        Ok((
            store
                .get_app_secret(FRP_PROFILE_TOKEN_SCOPE, &saved.id)
                .is_some_and(|value| !value.trim().is_empty()),
            store.data().default_tunnel_profile_id == saved.id,
        ))
    })?;
    Ok(profile_dto(&saved, has_token, is_default))
}

#[tauri::command]
pub fn delete_frp_profile(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.with_settings(|store| {
        let mut settings = store.settings();
        settings.frp_profiles.retain(|profile| profile.id != id);
        if settings.default_tunnel_profile_id == id {
            settings.default_tunnel_profile_id = settings
                .frp_profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_default();
        }
        store.update_settings(settings)?;
        store.delete_app_secret(FRP_PROFILE_TOKEN_SCOPE, &id)
    })
}

#[tauri::command]
pub fn get_app_settings(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state.with_settings(|store| Ok(store.settings()))
}

#[tauri::command]
pub fn get_proxy(state: State<'_, AppState>) -> AppResult<ProxyConfig> {
    state.with_settings(|store| Ok(store.settings().proxy))
}

#[tauri::command]
pub fn set_proxy(state: State<'_, AppState>, proxy: ProxyConfig) -> AppResult<()> {
    state.with_settings(|store| {
        let mut settings = store.settings();
        settings.proxy = proxy;
        store.update_settings(settings)
    })
}

#[tauri::command]
pub fn set_last_workspace(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.with_settings(|store| {
        let mut settings = store.settings();
        settings.last_workspace_id = id;
        store.update_settings(settings)
    })
}

#[tauri::command]
pub fn get_last_workspace_id(state: State<'_, AppState>) -> AppResult<String> {
    state.with_settings(|store| Ok(store.settings().last_workspace_id))
}
