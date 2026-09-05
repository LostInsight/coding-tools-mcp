use tauri::State;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::settings::CloudflareProfile;

const CLOUDFLARE_PROFILE_TUNNEL_TOKEN_SCOPE: &str = "cloudflare_profile_tunnel_token";
const CLOUDFLARE_PROFILE_API_TOKEN_SCOPE: &str = "cloudflare_profile_api_token";

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudflareProfileDto {
    pub id: String,
    pub name: String,
    pub account_id: String,
    pub tunnel_id: String,
    pub zone_id: String,
    pub extra_args: Vec<String>,
    pub has_tunnel_token: bool,
    pub has_api_token: bool,
    pub is_default: bool,
}

fn profile_dto(
    profile: &CloudflareProfile,
    has_tunnel_token: bool,
    has_api_token: bool,
    is_default: bool,
) -> CloudflareProfileDto {
    CloudflareProfileDto {
        id: profile.id.clone(),
        name: profile.name.clone(),
        account_id: profile.account_id.clone(),
        tunnel_id: profile.tunnel_id.clone(),
        zone_id: profile.zone_id.clone(),
        extra_args: profile.extra_args.clone(),
        has_tunnel_token,
        has_api_token,
        is_default,
    }
}

#[tauri::command]
pub fn list_cloudflare_profiles(
    state: State<'_, AppState>,
) -> AppResult<Vec<CloudflareProfileDto>> {
    state.with_settings(|store| {
        Ok(store
            .data()
            .cloudflare_profiles
            .iter()
            .map(|profile| {
                let has_tunnel_token = store
                    .get_app_secret(CLOUDFLARE_PROFILE_TUNNEL_TOKEN_SCOPE, &profile.id)
                    .is_some_and(|value| !value.trim().is_empty());
                let has_api_token = store
                    .get_app_secret(CLOUDFLARE_PROFILE_API_TOKEN_SCOPE, &profile.id)
                    .is_some_and(|value| !value.trim().is_empty());
                profile_dto(
                    profile,
                    has_tunnel_token,
                    has_api_token,
                    profile.id == store.data().default_cloudflare_profile_id,
                )
            })
            .collect())
    })
}

#[tauri::command]
pub fn save_cloudflare_profile(
    state: State<'_, AppState>,
    profile: CloudflareProfile,
    tunnel_token: Option<String>,
    api_token: Option<String>,
    make_default: Option<bool>,
) -> AppResult<CloudflareProfileDto> {
    if profile.name.trim().is_empty() {
        return Err(AppError::Message("Cloudflare 配置名称不能为空。".into()));
    }

    let mut saved = profile;
    saved.name = saved.name.trim().to_string();
    saved.account_id = saved.account_id.trim().to_string();
    saved.tunnel_id = saved.tunnel_id.trim().to_string();
    saved.zone_id = saved.zone_id.trim().to_string();
    crate::tunnel::validate_cloudflared_extra_args(&saved.extra_args)?;
    if saved.account_id.is_empty() || saved.tunnel_id.is_empty() || saved.zone_id.is_empty() {
        return Err(AppError::Message(
            "Cloudflare Account ID、Tunnel ID 与 Zone ID 必须同时填写。".into(),
        ));
    }
    if saved.id.trim().is_empty() {
        saved.id = uuid::Uuid::new_v4().to_string().replace('-', "");
    }

    state.with_settings(|store| {
        let mut settings = store.settings();
        if let Some(existing) = settings
            .cloudflare_profiles
            .iter_mut()
            .find(|item| item.id == saved.id)
        {
            *existing = saved.clone();
        } else {
            settings.cloudflare_profiles.push(saved.clone());
        }

        let make_default = make_default
            .unwrap_or_else(|| settings.default_cloudflare_profile_id.trim().is_empty());
        if make_default {
            settings.default_cloudflare_profile_id = saved.id.clone();
        } else if settings.default_cloudflare_profile_id == saved.id {
            settings.default_cloudflare_profile_id.clear();
        }
        store.update_settings(settings)?;

        if let Some(token) = tunnel_token.filter(|value| !value.trim().is_empty()) {
            store.set_app_secret(
                CLOUDFLARE_PROFILE_TUNNEL_TOKEN_SCOPE,
                &saved.id,
                token.trim(),
            )?;
        }
        if let Some(token) = api_token.filter(|value| !value.trim().is_empty()) {
            store.set_app_secret(CLOUDFLARE_PROFILE_API_TOKEN_SCOPE, &saved.id, token.trim())?;
        }
        Ok(())
    })?;

    let (has_tunnel_token, has_api_token, is_default) = state.with_settings(|store| {
        Ok((
            store
                .get_app_secret(CLOUDFLARE_PROFILE_TUNNEL_TOKEN_SCOPE, &saved.id)
                .is_some_and(|value| !value.trim().is_empty()),
            store
                .get_app_secret(CLOUDFLARE_PROFILE_API_TOKEN_SCOPE, &saved.id)
                .is_some_and(|value| !value.trim().is_empty()),
            store.data().default_cloudflare_profile_id == saved.id,
        ))
    })?;
    Ok(profile_dto(
        &saved,
        has_tunnel_token,
        has_api_token,
        is_default,
    ))
}

#[tauri::command]
pub fn delete_cloudflare_profile(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.with_settings(|store| {
        let mut settings = store.settings();
        settings
            .cloudflare_profiles
            .retain(|profile| profile.id != id);
        if settings.default_cloudflare_profile_id == id {
            settings.default_cloudflare_profile_id = settings
                .cloudflare_profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_default();
        }
        store.update_settings(settings)?;
        store.delete_app_secret(CLOUDFLARE_PROFILE_TUNNEL_TOKEN_SCOPE, &id)?;
        store.delete_app_secret(CLOUDFLARE_PROFILE_API_TOKEN_SCOPE, &id)
    })
}
