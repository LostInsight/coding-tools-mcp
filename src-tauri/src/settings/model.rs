use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::data::AppData;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrpProfile {
    pub id: String,
    pub name: String,
    pub server: String,
    #[serde(default = "default_frp_server_port", alias = "serverPort")]
    pub server_port: u16,
    /// Read-only compatibility fields for profiles written before Cloudflare
    /// settings were separated. `data::migrate` moves these values into a
    /// CloudflareProfile and they are never serialized again.
    #[serde(default, alias = "cloudflareAccountId")]
    #[serde(skip_serializing)]
    pub cloudflare_account_id: String,
    #[serde(default, alias = "cloudflareTunnelId")]
    #[serde(skip_serializing)]
    pub cloudflare_tunnel_id: String,
    #[serde(default, alias = "cloudflareZoneId")]
    #[serde(skip_serializing)]
    pub cloudflare_zone_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareProfile {
    pub id: String,
    pub name: String,
    #[serde(default, alias = "accountId")]
    pub account_id: String,
    #[serde(default, alias = "tunnelId")]
    pub tunnel_id: String,
    #[serde(default, alias = "zoneId")]
    pub zone_id: String,
}

/// Download settings for fetching frpc / cloudflared binaries.
///
/// GitHub is slow/unreliable from some networks, so downloads try a mirror
/// prefix first (ghproxy-style: `{mirror}/{full_github_url}`) and fall back to
/// the direct GitHub URL. An optional proxy can be layered on top.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadConfig {
    /// Mirror prefix applied before the full GitHub URL. Empty = direct.
    #[serde(default = "default_github_mirror")]
    pub github_mirror: String,
    /// "none" (no proxy) | "system" (env HTTP(S)_PROXY) | "manual".
    #[serde(default = "default_proxy_mode")]
    pub proxy_mode: String,
    /// Proxy URL used when `proxy_mode == "manual"` (e.g. http://127.0.0.1:7890).
    #[serde(default)]
    pub proxy_url: String,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            github_mirror: default_github_mirror(),
            proxy_mode: default_proxy_mode(),
            proxy_url: String::new(),
        }
    }
}

/// Global outbound proxy used by network-facing operations such as the
/// Cloudflare quick tunnel. Binary downloads use `download.proxy` separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyConfig {
    /// "none" (no proxy) | "system" (env HTTP(S)_PROXY) | "manual".
    #[serde(default = "default_proxy_mode")]
    pub mode: String,
    /// Proxy URL used when `mode == "manual"` (e.g. http://127.0.0.1:7890).
    #[serde(default)]
    pub url: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            mode: default_proxy_mode(),
            url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)]
    pub frp_profiles: Vec<FrpProfile>,
    #[serde(default, alias = "defaultTunnelProfileId")]
    pub default_tunnel_profile_id: String,
    #[serde(default)]
    pub cloudflare_profiles: Vec<CloudflareProfile>,
    #[serde(default, alias = "defaultCloudflareProfileId")]
    pub default_cloudflare_profile_id: String,
    #[serde(default)]
    pub last_workspace_id: String,
    #[serde(default)]
    pub download: DownloadConfig,
    /// Global outbound proxy (Cloudflare tunnel, etc.).
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// Shared secrets indexed by key name (e.g. "bearer_token").
    /// Persisted alongside other app settings in app_settings.json.
    #[serde(default)]
    pub shared_secrets: HashMap<String, String>,
    /// Per-workspace secrets: workspace_id -> secret_key -> value.
    #[serde(default)]
    pub workspace_secrets: HashMap<String, HashMap<String, String>>,
    /// App-scoped secrets: scope -> item_id -> value (e.g. frp profile tokens).
    #[serde(default)]
    pub app_secrets: HashMap<String, HashMap<String, String>>,
}

fn default_frp_server_port() -> u16 {
    7000
}

fn default_github_mirror() -> String {
    "https://gh-proxy.com".to_string()
}

fn default_proxy_mode() -> String {
    "system".to_string()
}

impl AppSettings {
    pub fn from_data(data: &AppData) -> Self {
        Self {
            frp_profiles: data.frp_profiles.clone(),
            default_tunnel_profile_id: data.default_tunnel_profile_id.clone(),
            cloudflare_profiles: data.cloudflare_profiles.clone(),
            default_cloudflare_profile_id: data.default_cloudflare_profile_id.clone(),
            last_workspace_id: data.last_workspace_id.clone(),
            download: data.download.clone(),
            proxy: data.proxy.clone(),
            shared_secrets: data.shared_secrets.clone(),
            workspace_secrets: data.workspace_secrets.clone(),
            app_secrets: data.app_secrets.clone(),
        }
    }

    pub fn apply_to(&self, data: &mut AppData) {
        data.frp_profiles = self.frp_profiles.clone();
        data.default_tunnel_profile_id = self.default_tunnel_profile_id.clone();
        data.cloudflare_profiles = self.cloudflare_profiles.clone();
        data.default_cloudflare_profile_id = self.default_cloudflare_profile_id.clone();
        data.last_workspace_id = self.last_workspace_id.clone();
        data.download = self.download.clone();
        data.proxy = self.proxy.clone();
        data.shared_secrets = self.shared_secrets.clone();
        data.workspace_secrets = self.workspace_secrets.clone();
        data.app_secrets = self.app_secrets.clone();
    }

    pub fn load_or_default() -> Self {
        crate::data::DataStore::read_file(|data| Ok(Self::from_data(data))).unwrap_or_default()
    }

    pub fn find_frp_profile(&self, id: &str) -> Option<&FrpProfile> {
        if id.trim().is_empty() {
            return None;
        }
        self.frp_profiles.iter().find(|profile| profile.id == id)
    }

    /// Use the workspace-selected profile when present; otherwise use the app default.
    pub fn tunnel_profile(&self, id: &str) -> Option<&FrpProfile> {
        let id = if id.trim().is_empty() {
            self.default_tunnel_profile_id.as_str()
        } else {
            id
        };
        self.find_frp_profile(id)
    }

    pub fn find_cloudflare_profile(&self, id: &str) -> Option<&CloudflareProfile> {
        if id.trim().is_empty() {
            return None;
        }
        self.cloudflare_profiles
            .iter()
            .find(|profile| profile.id == id)
    }

    /// Use the workspace-selected Cloudflare profile when present; otherwise
    /// use the app default Cloudflare profile.
    pub fn cloudflare_profile(&self, id: &str) -> Option<&CloudflareProfile> {
        let id = if id.trim().is_empty() {
            self.default_cloudflare_profile_id.as_str()
        } else {
            id
        };
        self.find_cloudflare_profile(id)
    }
}

#[allow(dead_code)]
impl FrpProfile {
    pub fn new(name: String, server: String, server_port: u16) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string().replace('-', ""),
            name,
            server: server.trim().to_string(),
            server_port,
            cloudflare_account_id: String::new(),
            cloudflare_tunnel_id: String::new(),
            cloudflare_zone_id: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, CloudflareProfile, FrpProfile};

    #[test]
    fn accepts_frontend_camel_case_server_port() {
        let profile: FrpProfile = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "公司 FRP",
            "server": "frp.example.com",
            "serverPort": 7004
        }))
        .expect("FRP profile should deserialize");

        assert_eq!(profile.server_port, 7004);
    }

    #[test]
    fn keeps_legacy_snake_case_server_port_compatible() {
        let profile: FrpProfile = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "公司 FRP",
            "server": "frp.example.com",
            "server_port": 7005
        }))
        .expect("legacy FRP profile should deserialize");

        assert_eq!(profile.server_port, 7005);
    }

    #[test]
    fn accepts_legacy_cloudflare_fields_for_migration_without_reserializing_them() {
        let profile: FrpProfile = serde_json::from_value(serde_json::json!({
            "id": "p1",
            "name": "Cloudflare",
            "server": "",
            "cloudflareAccountId": "account",
            "cloudflareTunnelId": "tunnel",
            "cloudflareZoneId": "zone"
        }))
        .expect("tunnel profile should deserialize");

        assert_eq!(profile.cloudflare_account_id, "account");
        assert_eq!(profile.cloudflare_tunnel_id, "tunnel");
        assert_eq!(profile.cloudflare_zone_id, "zone");

        let serialized = serde_json::to_value(profile).expect("profile should serialize");
        assert!(serialized.get("cloudflare_account_id").is_none());
        assert!(serialized.get("cloudflare_tunnel_id").is_none());
        assert!(serialized.get("cloudflare_zone_id").is_none());
    }

    #[test]
    fn cloudflare_profile_accepts_frontend_camel_case_fields() {
        let profile: CloudflareProfile = serde_json::from_value(serde_json::json!({
            "id": "cloudflare",
            "name": "Cloudflare",
            "accountId": "account",
            "tunnelId": "tunnel",
            "zoneId": "zone"
        }))
        .expect("Cloudflare profile should deserialize");

        assert_eq!(profile.account_id, "account");
        assert_eq!(profile.tunnel_id, "tunnel");
        assert_eq!(profile.zone_id, "zone");
    }

    #[test]
    fn blank_workspace_profile_selection_resolves_the_default_tunnel_profile() {
        let settings = AppSettings {
            default_tunnel_profile_id: "default".into(),
            frp_profiles: vec![FrpProfile {
                id: "default".into(),
                name: "Default tunnel".into(),
                server: "frp.example.com".into(),
                server_port: 7000,
                cloudflare_account_id: "account".into(),
                cloudflare_tunnel_id: "tunnel".into(),
                cloudflare_zone_id: "zone".into(),
            }],
            ..AppSettings::default()
        };

        assert_eq!(
            settings
                .tunnel_profile("")
                .map(|profile| profile.name.as_str()),
            Some("Default tunnel")
        );
    }
}
