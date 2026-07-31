use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use super::model::{PaseoError, StoredMonitorSnapshot};

const MAX_SNAPSHOTS: usize = 24;
const MAX_TOTAL_BYTES: u64 = 5 * 1024 * 1024;

pub struct PaseoMonitorStore {
    directory: PathBuf,
}

pub struct LoadedSnapshot {
    pub snapshot: Option<StoredMonitorSnapshot>,
    pub corrupted_files: usize,
}

impl PaseoMonitorStore {
    pub fn for_workspace(workspace_id: &str) -> Result<Self, PaseoError> {
        let root = crate::platform::platform()
            .app_config_dir()
            .map_err(|error| storage_error("resolve", error.to_string()))?
            .join("data")
            .join("paseo-monitor");
        Ok(Self::new(root, workspace_id))
    }

    pub fn new(root: PathBuf, workspace_id: &str) -> Self {
        let digest = Sha256::digest(workspace_id.as_bytes());
        let key = format!("{:x}", digest)[..24].to_string();
        Self {
            directory: root.join(key),
        }
    }

    pub fn load_latest(&self) -> Result<LoadedSnapshot, PaseoError> {
        if !self.directory.exists() {
            return Ok(LoadedSnapshot {
                snapshot: None,
                corrupted_files: 0,
            });
        }
        let mut paths = snapshot_paths(&self.directory)?;
        paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        let mut corrupted_files = 0;
        for path in paths {
            match fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<StoredMonitorSnapshot>(&bytes).ok())
            {
                Some(snapshot) => {
                    return Ok(LoadedSnapshot {
                        snapshot: Some(snapshot),
                        corrupted_files,
                    })
                }
                None => corrupted_files += 1,
            }
        }
        Ok(LoadedSnapshot {
            snapshot: None,
            corrupted_files,
        })
    }

    pub fn save(&self, snapshot: &StoredMonitorSnapshot) -> Result<(), PaseoError> {
        fs::create_dir_all(&self.directory)
            .map_err(|error| storage_error("create", error.to_string()))?;
        let bytes = serde_json::to_vec(snapshot)
            .map_err(|error| storage_error("serialize", error.to_string()))?;
        if bytes.len() as u64 > MAX_TOTAL_BYTES {
            return Err(storage_error("serialize", "snapshot exceeds storage limit"));
        }
        let temp = self
            .directory
            .join(format!(".{}.tmp", snapshot.snapshot_id));
        let final_path = self.directory.join(format!(
            "snapshot-{}-{}.json",
            sortable_time(&snapshot.created_at),
            snapshot.snapshot_id
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| storage_error("write", error.to_string()))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| storage_error("write", error.to_string()))?;
        fs::rename(&temp, &final_path)
            .map_err(|error| storage_error("rename", error.to_string()))?;
        self.prune()
    }

    pub fn clear(&self) -> Result<usize, PaseoError> {
        if !self.directory.exists() {
            return Ok(0);
        }
        let mut removed = 0;
        for path in snapshot_paths(&self.directory)? {
            if fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        for entry in fs::read_dir(&self.directory)
            .map_err(|error| storage_error("clear", error.to_string()))?
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("tmp") {
                let _ = fs::remove_file(path);
            }
        }
        let _ = fs::remove_dir(&self.directory);
        Ok(removed)
    }

    fn prune(&self) -> Result<(), PaseoError> {
        let mut paths = snapshot_paths(&self.directory)?;
        paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        let mut retained_bytes = 0_u64;
        for (index, path) in paths.into_iter().enumerate() {
            let size = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            retained_bytes = retained_bytes.saturating_add(size);
            if index >= MAX_SNAPSHOTS || retained_bytes > MAX_TOTAL_BYTES {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>, PaseoError> {
    Ok(fs::read_dir(directory)
        .map_err(|error| storage_error("read", error.to_string()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with("snapshot-") && name.ends_with(".json"))
        })
        .collect())
}

fn sortable_time(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_digit)
        .take(20)
        .collect()
}

fn storage_error(stage: &'static str, reason: impl Into<String>) -> PaseoError {
    let reason = reason.into();
    PaseoError::new(
        "PASEO_SNAPSHOT_CORRUPTED",
        "Paseo monitor snapshot storage could not be updated.",
        true,
        stage,
        json!({"reason": super::redaction::bounded(&super::redaction::redact(&reason), 240)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::paseo::model::{DiagnosisClassification, StoredAgentState};

    fn snapshot(id: &str) -> StoredMonitorSnapshot {
        StoredMonitorSnapshot {
            snapshot_id: id.into(),
            created_at: format!("2026-07-26T00:00:0{id}Z"),
            agents: vec![StoredAgentState {
                agent_id: "agent".into(),
                name: Some("name".into()),
                status: "RUNNING".into(),
                classification: DiagnosisClassification::Healthy,
                activity_fingerprint: "fingerprint".into(),
                last_effective_progress_at: None,
                observed_at: "2026-07-26T00:00:00Z".into(),
            }],
        }
    }

    #[test]
    fn first_snapshot_and_no_change_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let store = PaseoMonitorStore::new(temp.path().into(), "workspace");
        assert!(store.load_latest().unwrap().snapshot.is_none());
        store.save(&snapshot("1")).unwrap();
        assert_eq!(
            store.load_latest().unwrap().snapshot.unwrap().snapshot_id,
            "1"
        );
    }

    #[test]
    fn corruption_does_not_block_latest_valid_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let store = PaseoMonitorStore::new(temp.path().into(), "workspace");
        store.save(&snapshot("1")).unwrap();
        std::fs::write(store.directory.join("snapshot-999-bad.json"), "not json").unwrap();
        let loaded = store.load_latest().unwrap();
        assert_eq!(loaded.corrupted_files, 1);
        assert_eq!(loaded.snapshot.unwrap().snapshot_id, "1");
    }

    #[test]
    fn clear_stays_inside_workspace_store() {
        let temp = tempfile::tempdir().unwrap();
        let project_file = temp.path().join("project.txt");
        std::fs::write(&project_file, "keep").unwrap();
        let store = PaseoMonitorStore::new(temp.path().join("app-data"), "workspace");
        store.save(&snapshot("1")).unwrap();
        assert_eq!(store.clear().unwrap(), 1);
        assert!(project_file.exists());
    }
}
