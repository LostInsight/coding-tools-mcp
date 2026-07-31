export type RuntimeState = "stopped" | "starting" | "running" | "stopping" | "error";

export const DEFAULT_SERVICE_PORT = 28766;
export const DEFAULT_ACTIONS_PORT = 8787;

export interface TunnelConfig {
  type: string;
  public_url: string;
  frp_server: string;
  frp_subdomain: string;
  frp_profile_id?: string;
  frp_server_port?: number;
  cloudflare_mode: string;
  use_proxy?: boolean;
}

export interface AuthConfig {
  type: string;
  oauth_client_id: string;
  use_shared_secrets?: boolean;
}

export interface RuntimeConfig {
  local_port: number;
  tool_profile: string;
  permission_mode: string;
  runtime_command?: string;
  allowed_commands?: string;
  workspace_local_entries?: boolean;
  workspace_script_extensions?: string;
  filesystem?: FilesystemPolicyConfig;
}

export interface FilesystemPolicyConfig {
  mode: "workspace_only" | "workspace_and_skills" | "allowlist" | string;
  allowed_paths: string[];
  denied_paths: string[];
  denied_drives: string[];
}

export interface ActionsConfig {
  public_url: string;
  tunnel_type: string;
  frp_server: string;
  frp_subdomain: string;
  frp_profile_id?: string;
  frp_server_port?: number;
  cloudflare_mode: string;
  cloudflare_token?: string;
  use_proxy?: boolean;
  local_port: number;
  permission_mode: string;
  runtime_command?: string;
  auth_type: string;
  oauth_client_id?: string;
  oauth_scopes?: string;
  allowed_commands?: string;
  max_patch_bytes?: number;
  use_shared_secrets?: boolean;
}

export type PaseoAccessMode = "read_only" | "assist" | "control";

export interface PaseoMonitorConfig {
  enabled: boolean;
  workspace_only: boolean;
  agent_ids: string[];
  workspaces: string[];
  agent_name_patterns: string[];
  agent_labels: string[];
  exclude_name_patterns: string[];
  stalled_after_minutes: number;
  repeat_error_threshold: number;
  activity_tail: number;
}

export interface PaseoIntegrationConfig {
  enabled: boolean;
  access_mode: PaseoAccessMode;
  binary_path: string;
  command_timeout_ms: number;
  max_output_bytes: number;
  host_configured: boolean;
  monitor: PaseoMonitorConfig;
}

export interface WorkspaceIntegrations {
  paseo: PaseoIntegrationConfig;
}

export interface WorkspaceProfile {
  id: string;
  name: string;
  path: string;
  tunnel: TunnelConfig;
  auth: AuthConfig;
  runtime: RuntimeConfig;
  actions?: ActionsConfig;
  integrations?: WorkspaceIntegrations;
}

export function paseoIntegrationConfig(profile?: WorkspaceProfile | null): PaseoIntegrationConfig {
  const configured = profile?.integrations?.paseo;
  return {
    enabled: false,
    access_mode: "read_only",
    binary_path: "",
    command_timeout_ms: 15_000,
    max_output_bytes: 262_144,
    host_configured: false,
    ...configured,
    monitor: {
      enabled: true,
      workspace_only: false,
      agent_ids: [],
      workspaces: [],
      agent_name_patterns: [],
      agent_labels: [],
      exclude_name_patterns: ["paseo-supervisor"],
      stalled_after_minutes: 45,
      repeat_error_threshold: 2,
      activity_tail: 30,
      ...configured?.monitor,
    },
  };
}

export interface RuntimeStatus {
  state: RuntimeState;
  pid: number | null;
  localMessage: string;
  publicMessage: string;
  localEndpoint: string;
  publicEndpoint: string;
}

export function actionsConfig(profile: WorkspaceProfile): ActionsConfig {
  return {
    public_url: "",
    tunnel_type: "frp",
    frp_server: "",
    frp_subdomain: "",
    cloudflare_mode: "quick",
    local_port: DEFAULT_ACTIONS_PORT,
    permission_mode: "trusted",
    auth_type: "api_key",
    allowed_commands:
      "pytest,python,python3,npm,npx,node,pnpm,yarn,make,mvn,mvnw,gradle,gradlew,cargo,go,ruff,mypy,eslint,tsc",
    max_patch_bytes: 200_000,
    ...profile.actions,
  };
}

export function mcpLocalEndpoint(port: number): string {
  return `http://127.0.0.1:${port}/mcp`;
}

export function actionsLocalEndpoint(port: number): string {
  return `http://127.0.0.1:${port}`;
}

export interface ActionsAuthDraft {
  authType: string;
  oauthClientId: string;
  oauthScopes: string;
  useSharedSecrets?: boolean;
}

export interface FrpProfileSummary {
  id: string;
  name: string;
  server: string;
  serverPort: number;
}

export function frpPublicUrl(
  tunnelType: string,
  frpSubdomain: string,
  frpServer: string,
  frpProfileId: string | undefined,
  profiles: FrpProfileSummary[],
  publicUrl = "",
): string {
  if (tunnelType !== "frp" || !frpSubdomain) {
    return publicUrl.replace(/\/$/, "");
  }
  const server =
    profiles.find((profile) => profile.id === frpProfileId)?.server ?? frpServer;
  if (!server) return publicUrl.replace(/\/$/, "");
  return `https://${frpSubdomain}.${server}`;
}

export function actionsPublicBaseUrl(
  profile: WorkspaceProfile,
  frpProfiles: FrpProfileSummary[] = [],
): string {
  const actions = actionsConfig(profile);
  const publicUrl = frpPublicUrl(
    actions.tunnel_type,
    actions.frp_subdomain,
    actions.frp_server,
    actions.frp_profile_id,
    frpProfiles,
    actions.public_url,
  );
  if (publicUrl) return publicUrl;
  return actionsLocalEndpoint(actions.local_port);
}

export function actionsOpenApiUrl(
  profile: WorkspaceProfile,
  frpProfiles: FrpProfileSummary[] = [],
): string {
  const base = actionsPublicBaseUrl(profile, frpProfiles);
  return base ? `${base.replace(/\/$/, "")}/openapi.json` : "";
}

export function actionsPrivacyUrl(
  profile: WorkspaceProfile,
  frpProfiles: FrpProfileSummary[] = [],
): string {
  const base = actionsPublicBaseUrl(profile, frpProfiles);
  return base ? `${base.replace(/\/$/, "")}/privacy` : "";
}

export function actionsOAuthAuthorizeUrl(
  profile: WorkspaceProfile,
  frpProfiles: FrpProfileSummary[] = [],
): string {
  const base = actionsPublicBaseUrl(profile, frpProfiles);
  return base ? `${base.replace(/\/$/, "")}/oauth/authorize` : "";
}

export function actionsOAuthTokenUrl(
  profile: WorkspaceProfile,
  frpProfiles: FrpProfileSummary[] = [],
): string {
  const base = actionsPublicBaseUrl(profile, frpProfiles);
  return base ? `${base.replace(/\/$/, "")}/oauth/token` : "";
}
