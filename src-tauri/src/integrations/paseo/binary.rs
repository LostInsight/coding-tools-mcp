use std::path::{Path, PathBuf};

use serde_json::json;

use super::model::PaseoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PaseoLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub environment: Vec<(String, String)>,
}

pub(super) fn resolve_binary(configured: &str) -> Result<PaseoLaunch, PaseoError> {
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
fn resolve_trusted_program(path: &Path) -> Option<PaseoLaunch> {
    resolve_windows_program(path, 2)
}

#[cfg(windows)]
fn resolve_windows_program(path: &Path, remaining_launcher_hops: usize) -> Option<PaseoLaunch> {
    if is_native_windows_executable(path) {
        return bundled_launch_from_executable(path).or_else(|| {
            Some(PaseoLaunch {
                program: path.to_path_buf(),
                args: Vec::new(),
                environment: Vec::new(),
            })
        });
    }
    if remaining_launcher_hops == 0 || !is_paseo_cmd(path) {
        return None;
    }
    if let Some(launch) = bundled_desktop_launch(path) {
        return Some(launch);
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
fn bundled_desktop_launch(launcher: &Path) -> Option<PaseoLaunch> {
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
    let node_runner = resources
        .join("app.asar.unpacked")
        .join("dist")
        .join("daemon")
        .join("node-entrypoint-runner.js");
    let app_asar = resources.join("app.asar");
    if !is_native_windows_executable(&executable) || !node_runner.is_file() || !app_asar.is_file() {
        return None;
    }
    let cli_entrypoint = app_asar
        .join("node_modules")
        .join("@getpaseo")
        .join("cli")
        .join("dist")
        .join("index.js");
    Some(PaseoLaunch {
        program: executable,
        args: vec![
            "--disable-warning=DEP0040".into(),
            node_runner.display().to_string(),
            "node-script".into(),
            cli_entrypoint.display().to_string(),
        ],
        environment: vec![
            ("ELECTRON_RUN_AS_NODE".into(), "1".into()),
            ("PASEO_NODE_ENV".into(), "production".into()),
            ("PASEO_DESKTOP_MANAGED".into(), "1".into()),
            ("PASEO_CLI".into(), launcher.display().to_string()),
        ],
    })
}

#[cfg(windows)]
fn bundled_launch_from_executable(executable: &Path) -> Option<PaseoLaunch> {
    let launcher = executable
        .parent()?
        .join("resources")
        .join("bin")
        .join("paseo.cmd");
    let launch = bundled_desktop_launch(&launcher)?;
    (launch.program == executable).then_some(launch)
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
fn resolve_trusted_program(path: &Path) -> Option<PaseoLaunch> {
    (path.is_absolute() && path.is_file()).then(|| PaseoLaunch {
        program: path.to_path_buf(),
        args: Vec::new(),
        environment: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn bundled_fixture(root: &Path) -> (PathBuf, PathBuf) {
        let bin = root.join("resources").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let executable = root.join("Paseo.exe");
        std::fs::write(&executable, "fixture").unwrap();
        let resources = root.join("resources");
        let node_runner = resources
            .join("app.asar.unpacked")
            .join("dist")
            .join("daemon")
            .join("node-entrypoint-runner.js");
        std::fs::create_dir_all(node_runner.parent().unwrap()).unwrap();
        std::fs::write(&node_runner, "fixture").unwrap();
        std::fs::write(resources.join("app.asar"), "fixture").unwrap();
        let launcher = bin.join("paseo.cmd");
        std::fs::write(&launcher, "@echo off\r\n").unwrap();
        (executable, launcher)
    }

    #[cfg(windows)]
    #[test]
    fn bundled_cmd_is_discovery_only_and_resolves_to_native_executable() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Paseo With Spaces");
        let (executable, launcher) = bundled_fixture(&install);

        let launch = resolve_trusted_program(&launcher).unwrap();
        assert_eq!(launch.program, executable);
        assert_eq!(launch.args.len(), 4);
        assert!(launch
            .environment
            .iter()
            .any(|(key, value)| key == "ELECTRON_RUN_AS_NODE" && value == "1"));
    }

    #[cfg(windows)]
    #[test]
    fn path_shim_may_only_point_to_an_official_bundled_launcher() {
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("Paseo");
        let (executable, bundled) = bundled_fixture(&install);
        let shim = temp.path().join("paseo.cmd");
        std::fs::write(
            &shim,
            format!("@echo off\r\nset \"BUNDLED_CLI={}\"\r\n", bundled.display()),
        )
        .unwrap();

        assert_eq!(
            resolve_binary(shim.to_str().unwrap()).unwrap().program,
            executable
        );
        std::fs::write(&shim, "@echo off\r\ncall arbitrary.cmd %*\r\n").unwrap();
        assert!(resolve_trusted_program(&shim).is_none());
    }
}
