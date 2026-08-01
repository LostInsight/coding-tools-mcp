use tauri::State;



use crate::app_state::AppState;

use crate::error::{AppError, AppResult};

use crate::settings::{AppSettings, FrpProfile, ProxyConfig};



#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrpProfileDto {

    pub id: String,

    pub name: String,

    pub server: String,

    pub server_port: u16,

    pub cloudflare_account_id: String,

    pub cloudflare_tunnel_id: String,

    pub cloudflare_zone_id: String,

    pub has_token: bool,

    pub has_cloudflare_tunnel_token: bool,

    pub has_cloudflare_api_token: bool,

    pub is_default: bool,

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

                    .get_app_secret("frp_profile_token", &profile.id)

                    .is_some_and(|value| !value.trim().is_empty());

                let has_cloudflare_tunnel_token = store

                    .get_app_secret("tunnel_profile_cloudflare_tunnel_token", &profile.id)

                    .is_some_and(|value| !value.trim().is_empty());

                let has_cloudflare_api_token = store

                    .get_app_secret("tunnel_profile_cloudflare_api_token", &profile.id)

                    .is_some_and(|value| !value.trim().is_empty());

                FrpProfileDto {

                    id: profile.id.clone(),

                    name: profile.name.clone(),

                    server: profile.server.clone(),

                    server_port: profile.server_port,

                    cloudflare_account_id: profile.cloudflare_account_id.clone(),

                    cloudflare_tunnel_id: profile.cloudflare_tunnel_id.clone(),

                    cloudflare_zone_id: profile.cloudflare_zone_id.clone(),

                    has_token,

                    has_cloudflare_tunnel_token,

                    has_cloudflare_api_token,

                    is_default: profile.id == store.data().default_tunnel_profile_id,

                }

            })

            .collect())

    })

}



#[tauri::command]

pub fn save_frp_profile(

    state: State<'_, AppState>,

    profile: FrpProfile,

    token: Option<String>,

    cloudflare_tunnel_token: Option<String>,

    cloudflare_api_token: Option<String>,

    make_default: Option<bool>,

) -> AppResult<FrpProfileDto> {

    if profile.name.trim().is_empty() {

        return Err(AppError::Message("隧道配置名称不能为空。".into()));

    }

    let mut saved = profile;

    saved.name = saved.name.trim().to_string();

    saved.server = saved.server.trim().to_string();

    saved.cloudflare_account_id = saved.cloudflare_account_id.trim().to_string();

    saved.cloudflare_tunnel_id = saved.cloudflare_tunnel_id.trim().to_string();

    saved.cloudflare_zone_id = saved.cloudflare_zone_id.trim().to_string();

    let cloudflare_values = [
        saved.cloudflare_account_id.as_str(),
        saved.cloudflare_tunnel_id.as_str(),
        saved.cloudflare_zone_id.as_str(),
    ];

    let cloudflare_value_count = cloudflare_values

        .iter()

        .filter(|value| !value.is_empty())

        .count();

    if saved.server.is_empty() && cloudflare_value_count == 0 {

        return Err(AppError::Message(

            "隧道配置至少需要填写 FRP 服务器或完整的 Cloudflare Named Tunnel 标识。".into(),

        ));

    }

    if cloudflare_value_count != 0 && cloudflare_value_count != cloudflare_values.len() {

        return Err(AppError::Message(

            "Cloudflare Account ID、Tunnel ID 与 Zone ID 必须同时填写。".into(),

        ));

    }

    if !saved.server.is_empty() && saved.server_port == 0 {

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

        let make_default = make_default

            .unwrap_or_else(|| settings.default_tunnel_profile_id.trim().is_empty());

        if make_default {

            settings.default_tunnel_profile_id = saved.id.clone();

        } else if settings.default_tunnel_profile_id == saved.id {

            settings.default_tunnel_profile_id.clear();

        }

        store.update_settings(settings)?;

        if let Some(token) = token.filter(|value| !value.trim().is_empty()) {

            store.set_app_secret("frp_profile_token", &saved.id, token.trim())?;

        }

        if let Some(token) = cloudflare_tunnel_token.filter(|value| !value.trim().is_empty()) {

            store.set_app_secret(

                "tunnel_profile_cloudflare_tunnel_token",

                &saved.id,

                token.trim(),

            )?;

        }

        if let Some(token) = cloudflare_api_token.filter(|value| !value.trim().is_empty()) {

            store.set_app_secret(

                "tunnel_profile_cloudflare_api_token",

                &saved.id,

                token.trim(),

            )?;

        }

        Ok(())

    })?;



    let (has_token, has_cloudflare_tunnel_token, has_cloudflare_api_token, is_default) = state.with_settings(|store| {

        Ok((store

            .get_app_secret("frp_profile_token", &saved.id)

            .is_some_and(|value| !value.trim().is_empty()),

            store

                .get_app_secret("tunnel_profile_cloudflare_tunnel_token", &saved.id)

                .is_some_and(|value| !value.trim().is_empty()),

            store

                .get_app_secret("tunnel_profile_cloudflare_api_token", &saved.id)

                .is_some_and(|value| !value.trim().is_empty()),

            store.data().default_tunnel_profile_id == saved.id))

    })?;



    Ok(FrpProfileDto {

        id: saved.id.clone(),

        name: saved.name,

        server: saved.server,

        server_port: saved.server_port,

        cloudflare_account_id: saved.cloudflare_account_id,

        cloudflare_tunnel_id: saved.cloudflare_tunnel_id,

        cloudflare_zone_id: saved.cloudflare_zone_id,

        has_token,

        has_cloudflare_tunnel_token,

        has_cloudflare_api_token,

        is_default,

    })

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

        store.delete_app_secret("frp_profile_token", &id)?;

        store.delete_app_secret("tunnel_profile_cloudflare_tunnel_token", &id)?;

        store.delete_app_secret("tunnel_profile_cloudflare_api_token", &id)

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

