use serde::{Deserialize, Serialize};

use crate::integrations::paseo::WorkspaceIntegrations;
use crate::settings::AppSettings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceProfile {
    pub id: String,
    pub name: String,
    pub path: String,
    pub tunnel: TunnelConfig,
    pub auth: AuthConfig,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub actions: ActionsConfig,
    #[serde(default)]
    pub integrations: WorkspaceIntegrations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    #[serde(rename = "type", default = "default_tunnel_type")]
    pub tunnel_type: String,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub frp_server: String,
    #[serde(default)]
    pub frp_subdomain: String,
    #[serde(default)]
    pub frp_profile_id: String,
    #[serde(default)]
    pub cloudflare_profile_id: String,
    #[serde(default = "default_frp_server_port")]
    pub frp_server_port: u16,
    #[serde(default = "default_cloudflare_mode")]
    pub cloudflare_mode: String,
    #[serde(default)]
    pub cloudflare_account_id: String,
    #[serde(default)]
    pub cloudflare_tunnel_id: String,
    #[serde(default)]
    pub cloudflare_zone_id: String,
    #[serde(default)]
    pub cloudflare_overwrite_dns: bool,
    /// When true, apply global proxy from Settings → General when starting the tunnel.
    #[serde(default = "default_use_proxy")]
    pub use_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(rename = "type", default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default = "default_oauth_client_id")]
    pub oauth_client_id: String,
    #[serde(default)]
    pub use_shared_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_mcp_port")]
    pub local_port: u16,
    #[serde(default = "default_tool_profile")]
    pub tool_profile: String,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    #[serde(default)]
    pub runtime_command: String,
    /// Workspace execution policy shared by MCP clients.
    #[serde(default = "default_allowed_commands")]
    pub allowed_commands: String,
    #[serde(default = "default_workspace_local_entries")]
    pub workspace_local_entries: bool,
    #[serde(default = "default_workspace_script_extensions")]
    pub workspace_script_extensions: String,
    #[serde(default)]
    pub filesystem: FilesystemPolicyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicyConfig {
    #[serde(default = "default_filesystem_mode")]
    pub mode: String,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub denied_paths: Vec<String>,
    #[serde(default)]
    pub denied_drives: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionsConfig {
    #[serde(default)]
    pub public_url: String,
    #[serde(default = "default_tunnel_type")]
    pub tunnel_type: String,
    #[serde(default)]
    pub frp_server: String,
    #[serde(default)]
    pub frp_subdomain: String,
    #[serde(default)]
    pub frp_profile_id: String,
    #[serde(default)]
    pub cloudflare_profile_id: String,
    #[serde(default = "default_frp_server_port")]
    pub frp_server_port: u16,
    #[serde(default = "default_cloudflare_mode")]
    pub cloudflare_mode: String,
    #[serde(default)]
    pub cloudflare_token: String,
    #[serde(default)]
    pub cloudflare_account_id: String,
    #[serde(default)]
    pub cloudflare_tunnel_id: String,
    #[serde(default)]
    pub cloudflare_zone_id: String,
    #[serde(default)]
    pub cloudflare_overwrite_dns: bool,
    #[serde(default = "default_use_proxy")]
    pub use_proxy: bool,
    #[serde(default = "default_actions_port")]
    pub local_port: u16,
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,
    #[serde(default)]
    pub runtime_command: String,
    #[serde(default = "default_actions_auth_type")]
    pub auth_type: String,
    #[serde(default = "default_actions_oauth_client_id")]
    pub oauth_client_id: String,
    #[serde(default)]
    pub oauth_scopes: String,
    #[serde(default = "default_allowed_commands")]
    pub allowed_commands: String,
    #[serde(default = "default_max_patch_bytes")]
    pub max_patch_bytes: u32,
    #[serde(default)]
    pub use_shared_secrets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatusDto {
    pub state: String,
    pub pid: Option<u32>,
    pub local_message: String,
    pub public_message: String,
    pub local_endpoint: String,
    pub public_endpoint: String,
}

fn default_tunnel_type() -> String {
    "frp".to_string()
}

fn default_cloudflare_mode() -> String {
    "quick".to_string()
}

fn default_use_proxy() -> bool {
    true
}

fn default_auth_type() -> String {
    "oauth".to_string()
}

fn default_frp_server_port() -> u16 {
    7000
}

fn default_actions_auth_type() -> String {
    "api_key".to_string()
}

fn default_actions_oauth_client_id() -> String {
    format!(
        "chatgpt-actions-{}",
        &uuid::Uuid::new_v4().to_string()[..12]
    )
}

fn default_oauth_client_id() -> String {
    format!("chatgpt-client-{}", &uuid::Uuid::new_v4().to_string()[..12])
}

fn default_mcp_port() -> u16 {
    28766
}

fn default_actions_port() -> u16 {
    8787
}

fn default_tool_profile() -> String {
    "core".to_string()
}

fn default_permission_mode() -> String {
    "trusted".to_string()
}

fn default_allowed_commands() -> String {
    "pytest,python,python3,npm,npx,node,pnpm,yarn,make,mvn,mvnw,gradle,gradlew,cargo,go,ruff,mypy,eslint,tsc,git,cmd,powershell,pwsh".to_string()
}

fn default_workspace_local_entries() -> bool {
    true
}

fn default_workspace_script_extensions() -> String {
    ".exe,.bat,.cmd,.ps1".to_string()
}

fn default_filesystem_mode() -> String {
    "workspace_only".to_string()
}

impl Default for FilesystemPolicyConfig {
    fn default() -> Self {
        Self {
            mode: default_filesystem_mode(),
            allowed_paths: Vec::new(),
            denied_paths: Vec::new(),
            denied_drives: Vec::new(),
        }
    }
}

fn default_max_patch_bytes() -> u32 {
    200_000
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            tunnel_type: default_tunnel_type(),
            public_url: String::new(),
            frp_server: String::new(),
            frp_subdomain: String::new(),
            frp_profile_id: String::new(),
            cloudflare_profile_id: String::new(),
            frp_server_port: default_frp_server_port(),
            cloudflare_mode: default_cloudflare_mode(),
            cloudflare_account_id: String::new(),
            cloudflare_tunnel_id: String::new(),
            cloudflare_zone_id: String::new(),
            cloudflare_overwrite_dns: false,
            use_proxy: default_use_proxy(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_type: default_auth_type(),
            oauth_client_id: default_oauth_client_id(),
            use_shared_secrets: false,
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            local_port: default_mcp_port(),
            tool_profile: default_tool_profile(),
            permission_mode: default_permission_mode(),
            runtime_command: String::new(),
            allowed_commands: default_allowed_commands(),
            workspace_local_entries: default_workspace_local_entries(),
            workspace_script_extensions: default_workspace_script_extensions(),
            filesystem: FilesystemPolicyConfig::default(),
        }
    }
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            public_url: String::new(),
            tunnel_type: default_tunnel_type(),
            frp_server: String::new(),
            frp_subdomain: String::new(),
            frp_profile_id: String::new(),
            cloudflare_profile_id: String::new(),
            frp_server_port: default_frp_server_port(),
            cloudflare_mode: default_cloudflare_mode(),
            cloudflare_token: String::new(),
            cloudflare_account_id: String::new(),
            cloudflare_tunnel_id: String::new(),
            cloudflare_zone_id: String::new(),
            cloudflare_overwrite_dns: false,
            use_proxy: default_use_proxy(),
            local_port: default_actions_port(),
            permission_mode: default_permission_mode(),
            runtime_command: String::new(),
            auth_type: default_actions_auth_type(),
            oauth_client_id: default_actions_oauth_client_id(),
            oauth_scopes: String::new(),
            allowed_commands: default_allowed_commands(),
            max_patch_bytes: default_max_patch_bytes(),
            use_shared_secrets: false,
        }
    }
}

#[allow(dead_code)]
impl WorkspaceProfile {
    pub fn new(path: String, name: Option<String>) -> Self {
        let cleaned = path.trim_end_matches(['\\', '/']).to_string();
        let label = name.unwrap_or_else(|| {
            cleaned
                .replace('\\', "/")
                .split('/')
                .next_back()
                .unwrap_or("工作区")
                .to_string()
        });
        Self {
            id: uuid::Uuid::new_v4().to_string().replace('-', ""),
            name: label,
            path: cleaned,
            tunnel: TunnelConfig::default(),
            auth: AuthConfig::default(),
            runtime: RuntimeConfig::default(),
            actions: ActionsConfig::default(),
            integrations: WorkspaceIntegrations::default(),
        }
    }

    pub fn local_endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/mcp", self.runtime.local_port)
    }

    pub fn effective_public_url(&self) -> String {
        self.effective_public_url_with(&AppSettings::load_or_default())
    }

    pub fn effective_public_url_with(&self, settings: &AppSettings) -> String {
        computed_public_url(
            &self.tunnel.tunnel_type,
            &self.tunnel.frp_server,
            &self.tunnel.frp_subdomain,
            &self.tunnel.public_url,
            &self.tunnel.frp_profile_id,
            settings,
        )
    }

    pub fn public_endpoint(&self) -> String {
        let base = self.effective_public_url();
        if base.is_empty() {
            return String::new();
        }
        format!("{}/mcp", base.trim_end_matches('/'))
    }

    pub fn actions_local_base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.actions.local_port)
    }

    pub fn actions_effective_public_url(&self) -> String {
        self.actions_effective_public_url_with(&AppSettings::load_or_default())
    }

    pub fn actions_effective_public_url_with(&self, settings: &AppSettings) -> String {
        computed_public_url(
            &self.actions.tunnel_type,
            &self.actions.frp_server,
            &self.actions.frp_subdomain,
            &self.actions.public_url,
            &self.actions.frp_profile_id,
            settings,
        )
    }

    pub fn actions_openapi_url(&self) -> String {
        let base = self.actions_public_base_url();
        if base.is_empty() {
            return String::new();
        }
        format!("{}/openapi.json", base.trim_end_matches('/'))
    }

    pub fn actions_privacy_url(&self) -> String {
        let base = self.actions_public_base_url();
        if base.is_empty() {
            return String::new();
        }
        format!("{}/privacy", base.trim_end_matches('/'))
    }

    pub fn actions_oauth_authorization_url(&self) -> String {
        let base = self.actions_public_base_url();
        if base.is_empty() {
            return String::new();
        }
        format!("{}/oauth/authorize", base.trim_end_matches('/'))
    }

    pub fn actions_oauth_token_url(&self) -> String {
        let base = self.actions_public_base_url();
        if base.is_empty() {
            return String::new();
        }
        format!("{}/oauth/token", base.trim_end_matches('/'))
    }

    /// Public URL for GPT schema import; falls back to localhost when no tunnel is configured.
    pub fn actions_public_base_url(&self) -> String {
        let public = self.actions_effective_public_url();
        if public.is_empty() {
            self.actions_local_base_url()
        } else {
            public
        }
    }
}

fn computed_public_url(
    tunnel_type: &str,
    frp_server: &str,
    frp_subdomain: &str,
    public_url: &str,
    frp_profile_id: &str,
    settings: &AppSettings,
) -> String {
    if tunnel_type == "frp" {
        let use_tunnel_profile = !frp_profile_id.trim().is_empty() || frp_server.trim().is_empty();
        let server = if use_tunnel_profile {
            settings
                .tunnel_profile(frp_profile_id)
                .map(|profile| profile.server.as_str())
                .filter(|server| !server.trim().is_empty())
                .unwrap_or(frp_server)
        } else {
            frp_server
        };
        if !server.is_empty() && !frp_subdomain.is_empty() {
            return format!("https://{frp_subdomain}.{server}");
        }
    }
    public_url.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod paseo_config_tests {
    use serde_json::json;

    use super::WorkspaceProfile;
    use crate::settings::{AppSettings, FrpProfile};

    #[test]
    fn legacy_workspace_without_integrations_defaults_paseo_to_disabled() {
        let value = json!({
            "id": "workspace",
            "name": "Workspace",
            "path": ".",
            "tunnel": {},
            "auth": {},
            "runtime": {},
            "actions": {}
        });
        let profile: WorkspaceProfile = serde_json::from_value(value).expect("legacy profile");
        assert!(!profile.integrations.paseo.enabled);
        assert_eq!(profile.integrations.paseo.access_mode.as_str(), "read_only");
    }

    #[test]
    fn paseo_access_mode_roundtrips() {
        let mut profile = WorkspaceProfile::new(".".into(), Some("Workspace".into()));
        profile.integrations.paseo.enabled = true;
        profile.integrations.paseo.access_mode =
            crate::integrations::paseo::config::PaseoAccessMode::Assist;
        let loaded: WorkspaceProfile =
            serde_json::from_value(serde_json::to_value(profile).unwrap()).unwrap();
        assert_eq!(loaded.integrations.paseo.access_mode.as_str(), "assist");
    }

    #[test]
    fn blank_workspace_frp_settings_use_the_default_tunnel_profile_for_public_urls() {
        let mut workspace = WorkspaceProfile::new(".".into(), Some("Workspace".into()));
        workspace.tunnel.frp_subdomain = "mcp".into();
        let settings = AppSettings {
            default_tunnel_profile_id: "default".into(),
            frp_profiles: vec![FrpProfile {
                id: "default".into(),
                name: "Default".into(),
                server: "frp.example.com".into(),
                server_port: 7000,
                cloudflare_account_id: String::new(),
                cloudflare_tunnel_id: String::new(),
                cloudflare_zone_id: String::new(),
            }],
            ..AppSettings::default()
        };

        assert_eq!(
            workspace.effective_public_url_with(&settings),
            "https://mcp.frp.example.com"
        );
    }
}
