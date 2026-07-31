import { invoke } from "@tauri-apps/api/core";
import type { PaseoIntegrationConfig } from "$lib/types";

export interface PaseoIntegrationSettings {
  config: PaseoIntegrationConfig;
  connection_type: "local" | "remote";
  host_configured: boolean;
  host_display: string;
  host: string | null;
}

export interface PaseoIntegrationSettingsInput {
  config: PaseoIntegrationConfig;
  host?: string | null;
}

export interface PaseoConnectionTestResult {
  ok: boolean;
  cli_available?: boolean;
  cli_version?: string;
  daemon_reachable?: boolean;
  connection_type?: string;
  host_configured?: boolean;
  error?: {
    code?: string;
    message?: string;
  };
}

export interface PaseoFilterAgent {
  id: string;
  name: string | null;
  workspace: string | null;
  status: string;
}

export interface PaseoFilterOptions {
  agents: PaseoFilterAgent[];
  workspaces: string[];
}

export async function getPaseoIntegrationSettings(
  id: string,
): Promise<PaseoIntegrationSettings> {
  return invoke<PaseoIntegrationSettings>("get_paseo_integration_settings", { id });
}

export async function savePaseoIntegrationSettings(
  id: string,
  input: PaseoIntegrationSettingsInput,
): Promise<PaseoIntegrationSettings> {
  return invoke<PaseoIntegrationSettings>("save_paseo_integration_settings", { id, input });
}

export async function testPaseoConnection(
  id: string,
  input: PaseoIntegrationSettingsInput,
): Promise<PaseoConnectionTestResult> {
  return invoke<PaseoConnectionTestResult>("test_paseo_connection", { id, input });
}

export async function listPaseoFilterOptions(
  id: string,
  input: PaseoIntegrationSettingsInput,
): Promise<PaseoFilterOptions> {
  return invoke<PaseoFilterOptions>("list_paseo_filter_options", { id, input });
}

export async function clearPaseoMonitorSnapshots(id: string): Promise<number> {
  return invoke<number>("clear_paseo_monitor_snapshots", { id });
}
