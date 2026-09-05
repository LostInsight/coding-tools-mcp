use std::fs;
use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::platform::platform;
use crate::settings::{AppSettings, CloudflareProfile};

use super::model::{AppData, LegacyProfilesOnlyFile};

const LEGACY_PROFILES_FILE: &str = "profiles.json";
const LEGACY_SETTINGS_FILE: &str = "app_settings.json";

pub fn data_file_path() -> AppResult<PathBuf> {
    Ok(platform()
        .app_config_dir()?
        .join("data")
        .join("profiles.json"))
}

pub fn load_or_migrate() -> AppResult<AppData> {
    let path = data_file_path()?;
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let mut data = serde_json::from_str(&raw).unwrap_or_default();
        if migrate_separated_tunnel_profiles(&mut data) {
            write_data(&path, &data)?;
        }
        return Ok(data);
    }

    let app_root = platform().app_config_dir()?;
    let mut data = AppData::default();

    let legacy_profiles = app_root.join(LEGACY_PROFILES_FILE);
    if legacy_profiles.exists() {
        let raw = fs::read_to_string(&legacy_profiles)?;
        if let Ok(file) = serde_json::from_str::<LegacyProfilesOnlyFile>(&raw) {
            data.profiles = file.profiles;
        }
    }

    let legacy_settings = app_root.join(LEGACY_SETTINGS_FILE);
    if legacy_settings.exists() {
        let raw = fs::read_to_string(&legacy_settings)?;
        if let Ok(settings) = serde_json::from_str::<AppSettings>(&raw) {
            merge_settings(&mut data, settings);
        }
    }

    migrate_separated_tunnel_profiles(&mut data);

    Ok(data)
}

pub fn save(data: &AppData) -> AppResult<()> {
    let path = data_file_path()?;
    write_data(&path, data)
}

fn write_data(path: &Path, data: &AppData) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(data)?;
    fs::write(path, format!("{text}\n"))?;
    Ok(())
}

pub fn maybe_backup_legacy_files(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let app_root = platform().app_config_dir()?;
    for name in [LEGACY_PROFILES_FILE, LEGACY_SETTINGS_FILE] {
        let legacy = app_root.join(name);
        if legacy.exists() {
            let backup = app_root.join(format!("{name}.bak"));
            if !backup.exists() {
                let _ = fs::rename(&legacy, &backup);
            }
        }
    }
    Ok(())
}

fn merge_settings(data: &mut AppData, settings: AppSettings) {
    data.frp_profiles = settings.frp_profiles;
    data.default_tunnel_profile_id = settings.default_tunnel_profile_id;
    data.cloudflare_profiles = settings.cloudflare_profiles;
    data.default_cloudflare_profile_id = settings.default_cloudflare_profile_id;
    data.last_workspace_id = settings.last_workspace_id;
    data.download = settings.download;
    data.proxy = settings.proxy;
    data.shared_secrets = settings.shared_secrets;
    data.workspace_secrets = settings.workspace_secrets;
    data.app_secrets = settings.app_secrets;
}

fn migrate_separated_tunnel_profiles(data: &mut AppData) -> bool {
    let legacy_profiles = data
        .frp_profiles
        .iter_mut()
        .filter_map(|profile| {
            let has_cloudflare_values = !profile.cloudflare_account_id.trim().is_empty()
                || !profile.cloudflare_tunnel_id.trim().is_empty()
                || !profile.cloudflare_zone_id.trim().is_empty();
            if !has_cloudflare_values {
                return None;
            }

            let cloudflare = CloudflareProfile {
                id: profile.id.clone(),
                name: profile.name.clone(),
                account_id: std::mem::take(&mut profile.cloudflare_account_id),
                tunnel_id: std::mem::take(&mut profile.cloudflare_tunnel_id),
                zone_id: std::mem::take(&mut profile.cloudflare_zone_id),
                extra_args: Vec::new(),
            };
            Some(cloudflare)
        })
        .collect::<Vec<_>>();

    let mut changed = !legacy_profiles.is_empty();
    for legacy in legacy_profiles {
        if let Some(existing) = data
            .cloudflare_profiles
            .iter_mut()
            .find(|profile| profile.id == legacy.id)
        {
            changed |= merge_cloudflare_profile(existing, legacy);
        } else {
            data.cloudflare_profiles.push(legacy);
        }
    }

    let cloudflare_profile_ids = data
        .cloudflare_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();

    if data.default_cloudflare_profile_id.trim().is_empty()
        && cloudflare_profile_ids
            .iter()
            .any(|id| id == &data.default_tunnel_profile_id)
    {
        data.default_cloudflare_profile_id = data.default_tunnel_profile_id.clone();
        changed = true;
    }

    for workspace in &mut data.profiles {
        changed |= migrate_workspace_cloudflare_profile(
            &workspace.tunnel.tunnel_type,
            &mut workspace.tunnel.frp_profile_id,
            &mut workspace.tunnel.cloudflare_profile_id,
            &cloudflare_profile_ids,
        );
        changed |= migrate_workspace_cloudflare_profile(
            &workspace.actions.tunnel_type,
            &mut workspace.actions.frp_profile_id,
            &mut workspace.actions.cloudflare_profile_id,
            &cloudflare_profile_ids,
        );
    }

    changed |= migrate_profile_secret_scope(
        data,
        "tunnel_profile_cloudflare_tunnel_token",
        "cloudflare_profile_tunnel_token",
        &cloudflare_profile_ids,
    );
    changed |= migrate_profile_secret_scope(
        data,
        "tunnel_profile_cloudflare_api_token",
        "cloudflare_profile_api_token",
        &cloudflare_profile_ids,
    );

    changed
}

fn merge_cloudflare_profile(target: &mut CloudflareProfile, source: CloudflareProfile) -> bool {
    let mut changed = false;
    for (target_value, source_value) in [
        (&mut target.name, source.name),
        (&mut target.account_id, source.account_id),
        (&mut target.tunnel_id, source.tunnel_id),
        (&mut target.zone_id, source.zone_id),
    ] {
        if target_value.trim().is_empty() && !source_value.trim().is_empty() {
            *target_value = source_value;
            changed = true;
        }
    }
    changed
}

fn migrate_workspace_cloudflare_profile(
    tunnel_type: &str,
    frp_profile_id: &mut String,
    cloudflare_profile_id: &mut String,
    cloudflare_profile_ids: &[String],
) -> bool {
    if tunnel_type != "cloudflare" {
        return false;
    }

    let mut changed = false;
    if cloudflare_profile_id.trim().is_empty()
        && cloudflare_profile_ids.iter().any(|id| id == frp_profile_id)
    {
        *cloudflare_profile_id = frp_profile_id.clone();
        changed = true;
    }
    if !frp_profile_id.trim().is_empty() {
        frp_profile_id.clear();
        changed = true;
    }
    changed
}

fn migrate_profile_secret_scope(
    data: &mut AppData,
    legacy_scope: &str,
    new_scope: &str,
    cloudflare_profile_ids: &[String],
) -> bool {
    let legacy_values = data
        .app_secrets
        .get(legacy_scope)
        .cloned()
        .unwrap_or_default();
    let mut changed = false;

    for id in cloudflare_profile_ids {
        let Some(value) = legacy_values
            .get(id)
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        let destination = data.app_secrets.entry(new_scope.to_string()).or_default();
        if !destination.contains_key(id) {
            destination.insert(id.clone(), value.clone());
            changed = true;
        }
    }

    let mut remove_scope = false;
    if let Some(values) = data.app_secrets.get_mut(legacy_scope) {
        for id in cloudflare_profile_ids {
            changed |= values.remove(id).is_some();
        }
        remove_scope = values.is_empty();
    }
    if remove_scope {
        data.app_secrets.remove(legacy_scope);
    }
    changed
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::migrate_separated_tunnel_profiles;
    use crate::data::AppData;
    use crate::settings::FrpProfile;
    use crate::workspace::WorkspaceProfile;

    #[test]
    fn migrates_mixed_profiles_workspace_selections_and_secret_scopes() {
        let mut workspace = WorkspaceProfile::new("C:/workspace".into(), Some("Workspace".into()));
        workspace.tunnel.tunnel_type = "cloudflare".into();
        workspace.tunnel.frp_profile_id = "shared".into();
        workspace.actions.tunnel_type = "cloudflare".into();
        workspace.actions.frp_profile_id = "shared".into();

        let mut app_secrets = HashMap::new();
        app_secrets.insert(
            "tunnel_profile_cloudflare_tunnel_token".into(),
            HashMap::from([("shared".into(), "tunnel-token".into())]),
        );
        app_secrets.insert(
            "tunnel_profile_cloudflare_api_token".into(),
            HashMap::from([("shared".into(), "api-token".into())]),
        );
        let mut data = AppData {
            frp_profiles: vec![FrpProfile {
                id: "shared".into(),
                name: "Shared tunnel".into(),
                server: "frp.example.com".into(),
                server_port: 7000,
                cloudflare_account_id: "account".into(),
                cloudflare_tunnel_id: "tunnel".into(),
                cloudflare_zone_id: "zone".into(),
            }],
            default_tunnel_profile_id: "shared".into(),
            app_secrets,
            profiles: vec![workspace],
            ..AppData::default()
        };

        assert!(migrate_separated_tunnel_profiles(&mut data));
        assert_eq!(data.cloudflare_profiles.len(), 1);
        assert_eq!(data.cloudflare_profiles[0].id, "shared");
        assert_eq!(data.cloudflare_profiles[0].account_id, "account");
        assert_eq!(data.default_cloudflare_profile_id, "shared");
        assert!(data.frp_profiles[0].cloudflare_account_id.is_empty());
        assert_eq!(data.profiles[0].tunnel.cloudflare_profile_id, "shared");
        assert!(data.profiles[0].tunnel.frp_profile_id.is_empty());
        assert_eq!(data.profiles[0].actions.cloudflare_profile_id, "shared");
        assert!(data.profiles[0].actions.frp_profile_id.is_empty());
        assert_eq!(
            data.app_secrets["cloudflare_profile_tunnel_token"]["shared"],
            "tunnel-token"
        );
        assert_eq!(
            data.app_secrets["cloudflare_profile_api_token"]["shared"],
            "api-token"
        );
        assert!(!data
            .app_secrets
            .contains_key("tunnel_profile_cloudflare_tunnel_token"));
        assert!(!data
            .app_secrets
            .contains_key("tunnel_profile_cloudflare_api_token"));
    }
}
