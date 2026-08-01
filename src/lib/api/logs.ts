import { invoke } from "@tauri-apps/api/core";

export interface LogChunk {
  name: string;
  source: LogSource;
  content: string;
}

export type LogService = "mcp" | "actions";
export type LogSource = "access" | "request" | "cloudflare" | "frp" | "stdout" | "stderr";

export async function readWorkspaceLogs(
  workspaceId: string,
  service: LogService,
): Promise<LogChunk[]> {
  return invoke<LogChunk[]>("read_workspace_logs", { id: workspaceId, service });
}
