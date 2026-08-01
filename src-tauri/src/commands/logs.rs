use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::app_state::AppState;
use crate::error::{AppError, AppResult};
use crate::tunnel::log_dir_for_profile;
use crate::workspace::WorkspaceProfile;

const MAX_LOG_BYTES: usize = 8192;
const MAX_LOG_CHARS: usize = 4000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub name: String,
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy)]
struct LogFile {
    name: &'static str,
    source: &'static str,
}

fn profile_by_id(state: &AppState, id: &str) -> AppResult<WorkspaceProfile> {
    state.with_workspaces(|store| {
        store
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::Message(format!("workspace not found: {id}")))
    })
}

fn log_file_names(profile: &WorkspaceProfile, service: &str) -> AppResult<Vec<LogFile>> {
    match service {
        "mcp" => {
            let mut names = vec![
                LogFile {
                    name: "mcp-access.log",
                    source: "access",
                },
                LogFile {
                    name: "mcp-requests.log",
                    source: "request",
                },
            ];
            if profile.tunnel.tunnel_type == "cloudflare" {
                names.push(LogFile {
                    name: "cloudflared.log",
                    source: "cloudflare",
                });
            }
            if profile.tunnel.tunnel_type == "frp" {
                names.push(LogFile {
                    name: "frpc-mcp.log",
                    source: "frp",
                });
            }
            names.extend([
                LogFile {
                    name: "stdout.log",
                    source: "stdout",
                },
                LogFile {
                    name: "stderr.log",
                    source: "stderr",
                },
            ]);
            Ok(names)
        }
        "actions" => {
            let mut names = vec![LogFile {
                name: "actions-access.log",
                source: "access",
            }];
            if profile.actions.tunnel_type == "cloudflare" {
                names.push(LogFile {
                    name: "actions-cloudflared.log",
                    source: "cloudflare",
                });
            }
            if profile.actions.tunnel_type == "frp" {
                names.push(LogFile {
                    name: "frpc-actions.log",
                    source: "frp",
                });
            }
            names.extend([
                LogFile {
                    name: "actions-stdout.log",
                    source: "stdout",
                },
                LogFile {
                    name: "actions-stderr.log",
                    source: "stderr",
                },
            ]);
            Ok(names)
        }
        other => Err(AppError::Message(format!("unknown log service: {other}"))),
    }
}

fn read_log_tail(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    let size = file.seek(SeekFrom::End(0))?;
    let start = size.saturating_sub(MAX_LOG_BYTES as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(if text.chars().count() > MAX_LOG_CHARS {
        text.chars()
            .rev()
            .take(MAX_LOG_CHARS)
            .collect::<String>()
            .chars()
            .rev()
            .collect()
    } else {
        text
    })
}

#[tauri::command]
pub async fn read_workspace_logs(
    state: State<'_, AppState>,
    id: String,
    service: String,
) -> AppResult<Vec<LogChunk>> {
    let profile = profile_by_id(&state, &id)?;
    let log_dir = log_dir_for_profile(&profile.id);
    let names = log_file_names(&profile, &service)?;

    let mut chunks = Vec::new();
    for file in names {
        let path = log_dir.join(file.name);
        if !path.exists() {
            continue;
        }
        let content = read_log_tail(&path)?;
        chunks.push(LogChunk {
            name: file.name.to_string(),
            source: file.source.to_string(),
            content,
        });
    }

    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::log_file_names;
    use crate::workspace::WorkspaceProfile;

    #[test]
    fn mcp_logs_include_access_and_rpc_sources() {
        let profile = WorkspaceProfile::new("C:/workspace/demo".into(), Some("Demo".into()));

        let files = log_file_names(&profile, "mcp").expect("MCP log files");
        let sources = files.iter().map(|file| file.source).collect::<Vec<_>>();

        assert!(sources.contains(&"access"));
        assert!(sources.contains(&"request"));
        assert!(sources.contains(&"stdout"));
        assert!(sources.contains(&"stderr"));
    }

    #[test]
    fn cloudflare_and_frp_logs_keep_distinct_sources() {
        let mut cloudflare =
            WorkspaceProfile::new("C:/workspace/cloudflare".into(), Some("Cloudflare".into()));
        cloudflare.tunnel.tunnel_type = "cloudflare".into();
        let cloudflare_files = log_file_names(&cloudflare, "mcp").expect("Cloudflare log files");
        assert!(cloudflare_files
            .iter()
            .any(|file| file.name == "cloudflared.log" && file.source == "cloudflare"));

        let mut frp = WorkspaceProfile::new("C:/workspace/frp".into(), Some("FRP".into()));
        frp.tunnel.tunnel_type = "frp".into();
        let frp_files = log_file_names(&frp, "mcp").expect("FRP log files");
        assert!(frp_files
            .iter()
            .any(|file| file.name == "frpc-mcp.log" && file.source == "frp"));
    }
}
