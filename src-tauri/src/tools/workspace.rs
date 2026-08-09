use std::path::{Component, Path, PathBuf};

use serde_json::{json, Value};
use thiserror::Error;

pub const DEFAULT_EXCLUDED_NAMES: &[&str] = &[
    ".git",
    ".reference",
    "node_modules",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    "__pycache__",
];

#[derive(Debug, Clone)]
pub struct ResolvedPath {
    pub display: String,
    pub path: PathBuf,
    pub existed: bool,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("{message}")]
    Tool {
        code: &'static str,
        message: String,
        category: &'static str,
        retryable: bool,
    },
    #[error("{message}")]
    ToolDetails {
        code: &'static str,
        message: String,
        category: &'static str,
        retryable: bool,
        details: Value,
    },
}

impl WorkspaceError {
    pub fn message(&self) -> String {
        match self {
            Self::Tool { message, .. } | Self::ToolDetails { message, .. } => message.clone(),
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::Tool {
            code: "INVALID_ARGUMENT",
            message: message.into(),
            category: "validation",
            retryable: false,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::Tool {
            code: "NOT_FOUND",
            message: message.into(),
            category: "not_found",
            retryable: false,
        }
    }

    pub fn absolute_path_denied() -> Self {
        Self::Tool {
            code: "ABSOLUTE_PATH_DENIED",
            message: "Absolute paths are denied.".into(),
            category: "security",
            retryable: false,
        }
    }

    pub fn path_outside_workspace() -> Self {
        Self::Tool {
            code: "PATH_OUTSIDE_WORKSPACE",
            message: "Path escapes the configured workspace.".into(),
            category: "security",
            retryable: false,
        }
    }

    pub fn symlink_escape() -> Self {
        Self::Tool {
            code: "SYMLINK_ESCAPE",
            message: "Path escapes the configured workspace.".into(),
            category: "security",
            retryable: false,
        }
    }

    pub fn not_a_directory(message: impl Into<String>) -> Self {
        Self::Tool {
            code: "NOT_A_DIRECTORY",
            message: message.into(),
            category: "validation",
            retryable: false,
        }
    }

    pub fn to_error_value(&self) -> Value {
        match self {
            Self::Tool {
                code,
                message,
                category,
                retryable,
            } => json!({
                "code": code,
                "message": message,
                "category": category,
                "retryable": retryable,
                "details": {}
            }),
            Self::ToolDetails {
                code,
                message,
                category,
                retryable,
                details,
            } => json!({
                "code": code,
                "message": message,
                "category": category,
                "retryable": retryable,
                "details": details
            }),
        }
    }
}

pub type WorkspaceResult<T> = Result<T, WorkspaceError>;

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    read_policy: FilesystemReadPolicy,
}

#[derive(Debug, Clone, Default)]
struct FilesystemReadPolicy {
    mode: String,
    allowed_roots: Vec<PathBuf>,
    skill_roots: Vec<PathBuf>,
    denied_roots: Vec<PathBuf>,
    denied_drives: Vec<String>,
}

impl Workspace {
    pub fn new(root: PathBuf) -> WorkspaceResult<Self> {
        Self::with_filesystem_policy(root, &crate::workspace::FilesystemPolicyConfig::default())
    }

    pub fn with_filesystem_policy(
        root: PathBuf,
        config: &crate::workspace::FilesystemPolicyConfig,
    ) -> WorkspaceResult<Self> {
        let root = root
            .canonicalize()
            .map_err(|_| WorkspaceError::invalid_argument("Workspace root must exist"))?;
        if !root.is_dir() {
            return Err(WorkspaceError::invalid_argument(
                "Workspace root must be a directory",
            ));
        }
        if !matches!(
            config.mode.as_str(),
            "workspace_only" | "workspace_and_skills" | "allowlist"
        ) {
            return Err(WorkspaceError::invalid_argument(
                "filesystem.mode must be workspace_only, workspace_and_skills, or allowlist",
            ));
        }
        let resolve_roots = |values: &[String]| -> WorkspaceResult<Vec<PathBuf>> {
            values
                .iter()
                .filter(|value| !value.trim().is_empty())
                .map(|value| {
                    let path = PathBuf::from(value.trim());
                    let path = if path.is_absolute() {
                        path
                    } else {
                        root.join(path)
                    };
                    let resolved = path.canonicalize().map_err(|_| {
                        WorkspaceError::invalid_argument(format!(
                            "Configured filesystem path does not exist: {}",
                            path.display()
                        ))
                    })?;
                    if !resolved.is_dir() {
                        return Err(WorkspaceError::invalid_argument(format!(
                            "Configured filesystem path is not a directory: {}",
                            path.display()
                        )));
                    }
                    Ok(resolved)
                })
                .collect()
        };
        if config.allowed_paths.len() > 100
            || config.denied_paths.len() > 100
            || config.denied_drives.len() > 26
        {
            return Err(WorkspaceError::invalid_argument(
                "Filesystem policy contains too many entries",
            ));
        }
        let mut allowed_roots = resolve_roots(&config.allowed_paths)?;
        let mut skill_roots = if config.mode == "workspace_and_skills" {
            skill_roots(&root)
        } else {
            Vec::new()
        };
        allowed_roots.sort();
        allowed_roots.dedup();
        skill_roots.sort();
        skill_roots.dedup();
        let denied_roots = resolve_roots(&config.denied_paths)?;
        let denied_drives = config
            .denied_drives
            .iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                normalize_drive(value).ok_or_else(|| {
                    WorkspaceError::invalid_argument(format!(
                        "Invalid denied drive: {}",
                        value.trim()
                    ))
                })
            })
            .collect::<WorkspaceResult<Vec<_>>>()?;
        Ok(Self {
            root,
            read_policy: FilesystemReadPolicy {
                mode: config.mode.clone(),
                allowed_roots,
                skill_roots,
                denied_roots,
                denied_drives,
            },
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn root_display(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    pub fn reject_unsafe_text(&self, raw_path: &str) -> WorkspaceResult<()> {
        if raw_path.is_empty() {
            return Err(WorkspaceError::invalid_argument(
                "Path must be a non-empty string",
            ));
        }
        if raw_path.contains('\0') {
            return Err(WorkspaceError::invalid_argument("Path contains a NUL byte"));
        }
        if raw_path.starts_with('/') || raw_path.starts_with('\\') {
            return Err(WorkspaceError::absolute_path_denied());
        }
        if raw_path.len() >= 2 {
            let bytes = raw_path.as_bytes();
            if bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
                return Err(WorkspaceError::absolute_path_denied());
            }
        }
        for part in Path::new(raw_path).components() {
            if matches!(part, Component::ParentDir) {
                return Err(WorkspaceError::path_outside_workspace());
            }
        }
        Ok(())
    }

    pub fn resolve_existing(&self, raw_path: &str) -> WorkspaceResult<ResolvedPath> {
        self.resolve_existing_at(&self.root, raw_path)
    }

    /// 解析只读路径。显式的绝对路径和 `..` 路径允许指向 Workspace 外部，
    /// 但不会被任何写入工具复用。
    pub fn resolve_read_path(&self, raw_path: &str) -> WorkspaceResult<ResolvedPath> {
        let raw = if raw_path.is_empty() { "." } else { raw_path };
        self.validate_read_text(raw)?;
        let input = Path::new(raw);
        let candidate = if input.is_absolute() {
            input.to_path_buf()
        } else {
            self.root
                .join(raw.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        let resolved = candidate
            .canonicalize()
            .map_err(|_| WorkspaceError::not_found(format!("Path not found: {raw}")))?;
        let explicit_external = input.is_absolute()
            || input
                .components()
                .any(|part| matches!(part, Component::ParentDir));
        if !explicit_external && candidate.starts_with(&self.root) {
            self.ensure_inside_workspace(&candidate, &resolved)?;
        }
        self.ensure_read_allowed(&resolved)?;
        Ok(ResolvedPath {
            display: relative_display(&self.root, &resolved),
            path: resolved,
            existed: true,
        })
    }

    pub fn resolve_existing_at(
        &self,
        base: &Path,
        raw_path: &str,
    ) -> WorkspaceResult<ResolvedPath> {
        let raw = if raw_path.is_empty() { "." } else { raw_path };
        self.reject_unsafe_text(raw)?;
        let base = self.validate_base(base)?;
        let candidate = base.join(raw.replace('/', std::path::MAIN_SEPARATOR_STR));
        let resolved = candidate
            .canonicalize()
            .map_err(|_| WorkspaceError::not_found(format!("Path not found: {raw}")))?;
        self.ensure_inside_workspace(&candidate, &resolved)?;
        self.ensure_read_allowed(&resolved)?;
        Ok(ResolvedPath {
            display: relative_display(&self.root, &resolved),
            path: resolved,
            existed: true,
        })
    }

    pub fn resolve_for_write(&self, raw_path: &str) -> WorkspaceResult<ResolvedPath> {
        if raw_path.is_empty() {
            return Err(WorkspaceError::invalid_argument(
                "Path must be a non-empty string",
            ));
        }
        if raw_path.contains('\0') {
            return Err(WorkspaceError::invalid_argument("Path contains a NUL byte"));
        }
        self.reject_protected_write_path(raw_path)?;
        let pure = Path::new(raw_path);
        if pure.file_name().is_none() || raw_path == "." || raw_path == ".." {
            return Err(WorkspaceError::invalid_argument("Invalid write target"));
        }
        let explicit_absolute = pure.is_absolute();
        if !explicit_absolute {
            self.reject_unsafe_text(raw_path)?;
        }
        let candidate = if explicit_absolute {
            pure.to_path_buf()
        } else {
            self.root
                .join(raw_path.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        if candidate.exists() || candidate.is_symlink() {
            let resolved = candidate
                .canonicalize()
                .map_err(|_| WorkspaceError::not_found(format!("Path not found: {raw_path}")))?;
            self.ensure_write_allowed(&resolved)?;
            return Ok(ResolvedPath {
                display: relative_display(&self.root, &resolved),
                path: resolved,
                existed: true,
            });
        }
        let parent = candidate.parent().unwrap_or(&self.root);
        let resolved_parent = if parent.exists() {
            parent
                .canonicalize()
                .map_err(|_| WorkspaceError::not_found("Parent directory not found"))?
        } else {
            self.resolve_existing_parent(parent)?
        };
        self.ensure_write_allowed(&resolved_parent)?;
        Ok(ResolvedPath {
            display: relative_display(&self.root, &candidate),
            path: candidate,
            existed: false,
        })
    }

    fn resolve_existing_parent(&self, parent: &Path) -> WorkspaceResult<PathBuf> {
        let mut cursor = parent;
        while !cursor.exists() {
            let Some(next) = cursor.parent() else {
                return Err(WorkspaceError::not_found("Parent directory not found"));
            };
            if next == cursor {
                return Err(WorkspaceError::not_found("Parent directory not found"));
            }
            cursor = next;
        }
        cursor
            .canonicalize()
            .map_err(|_| WorkspaceError::not_found("Parent directory not found"))
    }

    fn validate_base(&self, base: &Path) -> WorkspaceResult<PathBuf> {
        let resolved = base
            .canonicalize()
            .map_err(|_| WorkspaceError::not_found("Base path not found"))?;
        if !resolved.is_dir() {
            return Err(WorkspaceError::not_a_directory("Base is not a directory"));
        }
        if !resolved.starts_with(&self.root) {
            return Err(WorkspaceError::path_outside_workspace());
        }
        Ok(resolved)
    }

    fn ensure_inside_workspace(&self, candidate: &Path, resolved: &Path) -> WorkspaceResult<()> {
        if !resolved.starts_with(&self.root) {
            if candidate.is_symlink() {
                return Err(WorkspaceError::symlink_escape());
            }
            return Err(WorkspaceError::path_outside_workspace());
        }
        Ok(())
    }

    pub fn reject_write_symlink(&self, raw_path: &str) -> WorkspaceResult<()> {
        if raw_path.contains('\0') {
            return Err(WorkspaceError::invalid_argument("Path contains a NUL byte"));
        }
        let candidate = if Path::new(raw_path).is_absolute() {
            PathBuf::from(raw_path)
        } else {
            self.reject_unsafe_text(raw_path)?;
            self.root
                .join(raw_path.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        if candidate.is_symlink() {
            return Err(WorkspaceError::symlink_escape());
        }
        Ok(())
    }

    pub fn reject_protected_write_path(&self, raw_path: &str) -> WorkspaceResult<()> {
        let normalized = raw_path.replace('\\', "/");
        if normalized
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git") || part.eq_ignore_ascii_case(".github"))
        {
            return Err(WorkspaceError::Tool {
                code: "PROTECTED_PATH",
                message: format!("禁止普通文件操作写入受保护目录: {raw_path}"),
                category: "security",
                retryable: false,
            });
        }
        Ok(())
    }

    fn validate_read_text(&self, raw_path: &str) -> WorkspaceResult<()> {
        if raw_path.contains('\0') {
            return Err(WorkspaceError::invalid_argument("Path contains a NUL byte"));
        }
        Ok(())
    }

    pub fn is_ignored_path(
        &self,
        path: &Path,
        include_hidden: bool,
        include_ignored: bool,
    ) -> bool {
        let Ok(scan_path) = path.strip_prefix(&self.root) else {
            // Workspace 外的读取路径不套用 Workspace 内部的隐藏/构建目录过滤，
            // 否则 Windows 临时目录等路径会被误判为隐藏目录而无法读取。
            return false;
        };
        let parts: Vec<String> = scan_path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        if !include_hidden {
            for part in &parts {
                if part.starts_with('.') && part != "." {
                    return true;
                }
            }
        }
        if !include_ignored {
            for part in &parts {
                if DEFAULT_EXCLUDED_NAMES.contains(&part.as_str()) {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_safe_existing_path(&self, path: &Path) -> bool {
        path.canonicalize()
            .is_ok_and(|p| self.ensure_read_allowed(&p).is_ok())
    }

    pub fn is_safe_read_path(&self, path: &Path) -> bool {
        path.canonicalize()
            .ok()
            .is_some_and(|resolved| self.ensure_read_allowed(&resolved).is_ok())
    }

    fn ensure_read_allowed(&self, path: &Path) -> WorkspaceResult<()> {
        if self.is_under_any(path, &self.read_policy.allowed_roots) {
            return Ok(());
        }
        self.ensure_not_denied(path)?;
        let allowed_by_mode = path.starts_with(&self.root)
            || (self.read_policy.mode == "workspace_and_skills"
                && self.is_under_any(path, &self.read_policy.skill_roots));
        if !allowed_by_mode {
            return Err(WorkspaceError::Tool {
                code: "FILESYSTEM_ACCESS_DENIED",
                message: format!("Filesystem policy denied access to {}", path.display()),
                category: "security",
                retryable: false,
            });
        }
        Ok(())
    }

    fn ensure_write_allowed(&self, path: &Path) -> WorkspaceResult<()> {
        if self.is_under_any(path, &self.read_policy.allowed_roots) {
            return Ok(());
        }
        self.ensure_not_denied(path)?;
        if path.starts_with(&self.root) {
            return Ok(());
        }
        Err(WorkspaceError::Tool {
            code: "FILESYSTEM_ACCESS_DENIED",
            message: format!("Filesystem policy denied access to {}", path.display()),
            category: "security",
            retryable: false,
        })
    }

    fn ensure_not_denied(&self, path: &Path) -> WorkspaceResult<()> {
        let denied_root = self
            .read_policy
            .denied_roots
            .iter()
            .any(|root| path.starts_with(root));
        let denied_drive = path_drive(path).is_some_and(|drive| {
            self.read_policy
                .denied_drives
                .iter()
                .any(|denied| denied.eq_ignore_ascii_case(&drive))
        });
        if denied_root || denied_drive {
            return Err(WorkspaceError::Tool {
                code: "FILESYSTEM_ACCESS_DENIED",
                message: format!("Filesystem policy denied access to {}", path.display()),
                category: "security",
                retryable: false,
            });
        }
        Ok(())
    }

    fn is_under_any(&self, path: &Path, roots: &[PathBuf]) -> bool {
        roots.iter().any(|root| path.starts_with(root))
    }

    /// Check an absolute or workdir-relative path mentioned in a command.
    /// This is a policy check only; the child process is not OS-sandboxed.
    pub fn is_allowed_command_path(&self, raw_path: &str, base: &Path) -> bool {
        let raw = raw_path
            .trim_matches(|ch: char| matches!(ch, '\'' | '"' | '`' | ',' | ';' | ')' | ']' | '}'))
            .trim();
        if raw.is_empty() || raw.contains('\0') {
            return false;
        }
        let input = Path::new(raw);
        let candidate = if input.is_absolute() {
            input.to_path_buf()
        } else {
            base.join(raw.replace('/', std::path::MAIN_SEPARATOR_STR))
        };
        if let Ok(resolved) = candidate.canonicalize() {
            return self.ensure_read_allowed(&resolved).is_ok();
        }
        let Some(parent) = candidate.parent() else {
            return false;
        };
        self.resolve_existing_parent(parent)
            .is_ok_and(|resolved| self.ensure_read_allowed(&resolved).is_ok())
    }
}

fn skill_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![
        workspace_root.join(".agents/skills"),
        workspace_root.join(".claude/skills"),
        workspace_root.join(".codex/skills"),
    ];
    if let Some(home) = dirs::home_dir() {
        candidates.extend([
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            home.join(".codex/skills"),
        ]);
    }
    candidates
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .filter(|path| path.is_dir())
        .collect()
}

fn normalize_drive(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches(['\\', '/']);
    let bytes = trimmed.as_bytes();
    if !matches!(bytes, [letter] if letter.is_ascii_alphabetic())
        && !matches!(bytes, [letter, b':'] if letter.is_ascii_alphabetic())
    {
        return None;
    }
    let letter = bytes[0] as char;
    Some(format!("{}:", letter.to_ascii_uppercase()))
}

#[cfg(windows)]
fn path_drive(path: &Path) -> Option<String> {
    let value = path.to_string_lossy();
    let value = value.strip_prefix(r"\\?\").unwrap_or(&value);
    let bytes = value.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        .then(|| format!("{}:", (bytes[0] as char).to_ascii_uppercase()))
}

#[cfg(not(windows))]
fn path_drive(_path: &Path) -> Option<String> {
    None
}

pub fn relative_display(root: &Path, path: &Path) -> String {
    let display = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
    #[cfg(windows)]
    {
        if let Some(unc) = display.strip_prefix("//?/UNC/") {
            return format!("//{unc}");
        }
        if let Some(normal) = display.strip_prefix("//?/") {
            return normal.to_string();
        }
    }
    display
}

pub fn tool_ok(mut value: Value) -> Value {
    if value.get("ok").is_none() {
        value
            .as_object_mut()
            .expect("tool result object")
            .insert("ok".into(), Value::Bool(true));
    }
    value
}

pub fn tool_err(error: WorkspaceError) -> Value {
    json!({
        "ok": false,
        "status": "error",
        "summary": error.message(),
        "error": error.to_error_value()
    })
}

pub fn tool_err_code(
    code: &'static str,
    message: impl Into<String>,
    category: &'static str,
) -> Value {
    let message = message.into();
    json!({
        "ok": false,
        "status": "error",
        "summary": message.clone(),
        "error": {
            "code": code,
            "message": message,
            "category": category,
            "retryable": false,
            "details": {}
        }
    })
}

pub fn wrap_tool_result(structured: Value) -> Value {
    wrap_mcp_tool_result("", &serde_json::json!({}), structured)
}

pub fn wrap_mcp_tool_result(tool_name: &str, args: &Value, structured: Value) -> Value {
    let is_error = structured.get("ok").and_then(Value::as_bool) == Some(false);
    let content = if tool_name == "view_image"
        && args
            .get("output")
            .and_then(Value::as_str)
            .unwrap_or("mcp_image")
            == "mcp_image"
        && !is_error
    {
        vec![json!({
            "type": "image",
            "data": structured.get("base64").and_then(Value::as_str).unwrap_or(""),
            "mimeType": structured
                .get("mime_type")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream")
        })]
    } else {
        vec![json!({
            "type": "text",
            "text": structured.to_string()
        })]
    };
    json!({
        "content": content,
        "structuredContent": structured,
        "isError": is_error
    })
}

#[cfg(test)]
mod mcp_result_tests {
    use super::wrap_mcp_tool_result;
    use serde_json::json;

    #[test]
    fn mcp_wrapper_preserves_specific_paseo_error_codes() {
        let wrapped = wrap_mcp_tool_result(
            "paseo_get_agent_activity",
            &json!({"agent_id": "agent-1"}),
            json!({
                "ok": false,
                "error": {
                    "code": "PASEO_DAEMON_UNREACHABLE",
                    "message": "daemon unavailable",
                    "category": "paseo",
                    "retryable": true,
                    "details": {"stage": "activity"}
                }
            }),
        );

        assert_eq!(wrapped["isError"], true);
        assert_eq!(
            wrapped["structuredContent"]["error"]["code"],
            "PASEO_DAEMON_UNREACHABLE"
        );
        assert!(wrapped["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("PASEO_DAEMON_UNREACHABLE"));
    }
}

#[cfg(test)]
mod filesystem_policy_tests {
    use super::*;
    use crate::workspace::FilesystemPolicyConfig;

    #[test]
    fn workspace_only_rejects_explicit_external_reads() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        std::fs::create_dir(&root).expect("workspace");
        let external = temp.path().join("external.txt");
        std::fs::write(&external, "secret").expect("external file");
        let workspace = Workspace::new(root).expect("workspace");

        let error = workspace
            .resolve_read_path(&external.to_string_lossy())
            .expect_err("external read must be denied");

        assert_eq!(error.to_error_value()["code"], "FILESYSTEM_ACCESS_DENIED");
    }

    #[test]
    fn explicit_allow_root_overrides_denied_root_for_reads_and_writes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        let allowed = temp.path().join("allowed");
        let denied = allowed.join("private");
        std::fs::create_dir(&root).expect("workspace");
        std::fs::create_dir_all(&denied).expect("allowed tree");
        let public_file = allowed.join("public.txt");
        let private_file = denied.join("private.txt");
        std::fs::write(&public_file, "public").expect("public file");
        std::fs::write(&private_file, "private").expect("private file");
        let policy = FilesystemPolicyConfig {
            mode: "workspace_only".into(),
            allowed_paths: vec![allowed.to_string_lossy().into_owned()],
            denied_paths: vec![denied.to_string_lossy().into_owned()],
            denied_drives: Vec::new(),
        };
        let workspace = Workspace::with_filesystem_policy(root, &policy).expect("workspace");

        assert!(workspace
            .resolve_read_path(&public_file.to_string_lossy())
            .is_ok());
        assert!(workspace
            .resolve_read_path(&private_file.to_string_lossy())
            .is_ok());
        assert!(workspace
            .resolve_for_write(&denied.join("created.txt").to_string_lossy())
            .is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn explicit_allow_root_overrides_denied_drive() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("workspace");
        let allowed = temp.path().join("allowed");
        std::fs::create_dir(&root).expect("workspace");
        std::fs::create_dir(&allowed).expect("allowed");
        let allowed_file = allowed.join("read.txt");
        std::fs::write(&allowed_file, "allowed").expect("allowed file");
        let drive = path_drive(&root.canonicalize().expect("canonical root")).expect("drive");
        let policy = FilesystemPolicyConfig {
            mode: "workspace_only".into(),
            allowed_paths: vec![allowed.to_string_lossy().into_owned()],
            denied_paths: Vec::new(),
            denied_drives: vec![drive],
        };
        let workspace = Workspace::with_filesystem_policy(root, &policy).expect("workspace");

        assert!(workspace.resolve_read_path(".").is_err());
        assert!(workspace
            .resolve_read_path(&allowed_file.to_string_lossy())
            .is_ok());
        assert!(workspace
            .resolve_for_write(&allowed.join("created.txt").to_string_lossy())
            .is_ok());
    }
}
