use std::path::{Path, PathBuf};

use serde_json::json;

use super::model::PaseoError;

pub(super) fn resolve_binary(configured: &str) -> Result<PathBuf, PaseoError> {
    let configured = configured.trim();
    let discovered = if configured.is_empty() {
        crate::platform::platform()
            .resolve_executable("paseo")
            .or_else(|| which::which("paseo").ok())
            .ok_or_else(cli_not_found)?
    } else {
        let path = PathBuf::from(configured);
        if !path.is_absolute() {
            return Err(untrusted_binary());
        }
        path
    };
    resolve_trusted_program(&discovered).ok_or_else(untrusted_binary)
}

fn cli_not_found() -> PaseoError {
    PaseoError::new(
        "PASEO_CLI_NOT_FOUND",
        "Paseo CLI was not found. Install Paseo or select its binary in workspace settings.",
        false,
        "discover",
        json!({}),
    )
}

fn untrusted_binary() -> PaseoError {
    PaseoError::new(
        "PASEO_CLI_NOT_FOUND",
        "The discovered Paseo CLI is not a trusted native executable file.",
        false,
        "discover",
        json!({}),
    )
}

#[cfg(windows)]
fn resolve_trusted_program(path: &Path) -> Option<PathBuf> {
    resolve_windows_program(path, 2)
}

#[cfg(windows)]
fn resolve_windows_program(path: &Path, remaining_launcher_hops: usize) -> Option<PathBuf> {
    if is_native_windows_executable(path) {
        return Some(path.to_path_buf());
    }
    if remaining_launcher_hops == 0 || !is_paseo_cmd(path) {
        return None;
    }
    if let Some(executable) = bundled_desktop_executable(path) {
        return Some(executable);
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 8_192 {
        return None;
    }
    let contents = std::str::from_utf8(&bytes).ok()?;
    let target = bundled_cli_target(contents)?;
    resolve_windows_program(&target, remaining_launcher_hops - 1)
}

#[cfg(windows)]
fn is_native_windows_executable(path: &Path) -> bool {
    path.is_absolute()
        && path.is_file()
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
}

#[cfg(windows)]
fn is_paseo_cmd(path: &Path) -> bool {
    path.is_absolute()
        && path.is_file()
        && path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("paseo.cmd"))
}

#[cfg(windows)]
fn bundled_desktop_executable(launcher: &Path) -> Option<PathBuf> {
    let bin = launcher.parent()?;
    let resources = bin.parent()?;
    if !bin
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("bin"))
        || !resources
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("resources"))
    {
        return None;
    }
    let executable = resources.parent()?.join("Paseo.exe");
    is_native_windows_executable(&executable).then_some(executable)
}

#[cfg(windows)]
fn bundled_cli_target(contents: &str) -> Option<PathBuf> {
    const PREFIX: &str = "set \"BUNDLED_CLI=";
    contents.lines().find_map(|line| {
        let line = line.trim();
        let prefix = line.get(..PREFIX.len())?;
        if !prefix.eq_ignore_ascii_case(PREFIX) {
            return None;
        }
        let value = line.get(PREFIX.len()..)?.strip_suffix('"')?;
        if value.is_empty()
            || value.len() > 1_024
            || value.contains('%')
            || value.chars().any(char::is_control)
        {
            return None;
        }
        let path = PathBuf::from(value);
        (path.is_absolute()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("paseo.cmd")))
        .then_some(path)
    })
}

#[cfg(not(windows))]
fn resolve_trusted_program(path: &Path) -> Option<PathBuf> {
    (path.is_absolute() && path.is_file()).then(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn bundled_cmd_is_discovery_only_and_resolves_to_native_executable() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Paseo With Spaces");
        let bin = install.join("resources").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = install.join("Paseo.exe");
        std::fs::write(&executable, "fixture").unwrap();
        let launcher = bin.join("paseo.cmd");
        std::fs::write(&launcher, "@echo off\r\n").unwrap();

        assert_eq!(resolve_trusted_program(&launcher), Some(executable));
    }

    #[cfg(windows)]
    #[test]
    fn path_shim_may_only_point_to_an_official_bundled_launcher() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Paseo");
        let bin = install.join("resources").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = install.join("Paseo.exe");
        std::fs::write(&executable, "fixture").unwrap();
        let bundled = bin.join("paseo.cmd");
        std::fs::write(&bundled, "@echo off\r\n").unwrap();
        let shim = temp.path().join("paseo.cmd");
        std::fs::write(
            &shim,
            format!("@echo off\r\nset \"BUNDLED_CLI={}\"\r\n", bundled.display()),
        )
        .unwrap();

        assert_eq!(resolve_binary(shim.to_str().unwrap()).unwrap(), executable);
        std::fs::write(&shim, "@echo off\r\ncall arbitrary.cmd %*\r\n").unwrap();
        assert!(resolve_trusted_program(&shim).is_none());
    }
}
