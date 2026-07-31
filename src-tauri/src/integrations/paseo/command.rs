use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde_json::json;

use super::model::{PaseoCommandOutput, PaseoCommandSpec, PaseoError};
use super::redaction::redact;

pub trait PaseoCommandRunner: Send + Sync {
    fn run(&self, spec: &PaseoCommandSpec) -> Result<PaseoCommandOutput, PaseoError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemPaseoCommandRunner;

impl PaseoCommandRunner for SystemPaseoCommandRunner {
    fn run(&self, spec: &PaseoCommandSpec) -> Result<PaseoCommandOutput, PaseoError> {
        #[cfg(windows)]
        if !spec
            .program
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
        {
            return Err(PaseoError::new(
                "PASEO_CLI_NOT_FOUND",
                "Paseo must resolve to a native Windows executable.",
                false,
                "spawn",
                json!({}),
            ));
        }
        let started = Instant::now();
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        apply_minimal_environment(&mut command);

        #[cfg(windows)]
        command.creation_flags(0x0800_0000);

        let mut child = command.spawn().map_err(|error| {
            PaseoError::new(
                "PASEO_CLI_NOT_FOUND",
                "Paseo CLI could not be started. Configure an installed Paseo binary.",
                false,
                "spawn",
                json!({"reason": redact(&error.to_string())}),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            PaseoError::new(
                "PASEO_COMMAND_FAILED",
                "Paseo stdout capture could not be initialized.",
                true,
                "capture",
                json!({}),
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            PaseoError::new(
                "PASEO_COMMAND_FAILED",
                "Paseo stderr capture could not be initialized.",
                true,
                "capture",
                json!({}),
            )
        })?;
        let stdout_reader = spawn_bounded_reader(stdout, spec.max_output_bytes);
        let stderr_reader = spawn_bounded_reader(stderr, spec.max_output_bytes);
        let timeout = Duration::from_millis(spec.timeout_ms);

        let (status, timed_out) = loop {
            match child.try_wait() {
                Ok(Some(status)) => break (Some(status), false),
                Ok(None) if started.elapsed() < timeout => {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = crate::platform::platform().terminate_process_tree(child.id());
                    let _ = child.kill();
                    let status = child.wait().ok();
                    break (status, true);
                }
                Err(error) => {
                    let _ = crate::platform::platform().terminate_process_tree(child.id());
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PaseoError::new(
                        "PASEO_COMMAND_FAILED",
                        "Unable to observe the Paseo process.",
                        true,
                        "wait",
                        json!({"reason": redact(&error.to_string())}),
                    ));
                }
            }
        };

        let stdout = stdout_reader.join().unwrap_or_default();
        let stderr = stderr_reader.join().unwrap_or_default();
        let output = PaseoCommandOutput {
            stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
            exit_code: status.and_then(|status| status.code()),
            duration_ms: started.elapsed().as_millis(),
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        };
        if timed_out {
            return Err(PaseoError::new(
                "PASEO_COMMAND_TIMEOUT",
                "Paseo command exceeded the configured timeout.",
                true,
                "execute",
                json!({"timeout_ms": spec.timeout_ms}),
            ));
        }
        Ok(output)
    }
}

#[derive(Default)]
struct BoundedCapture {
    bytes: Vec<u8>,
    truncated: bool,
}

fn spawn_bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    max_bytes: usize,
) -> thread::JoinHandle<BoundedCapture> {
    thread::spawn(move || {
        let mut capture = BoundedCapture::default();
        let mut buffer = [0_u8; 8_192];
        loop {
            let read = match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let remaining = max_bytes.saturating_sub(capture.bytes.len());
            let retained = remaining.min(read);
            capture.bytes.extend_from_slice(&buffer[..retained]);
            capture.truncated |= retained < read;
        }
        capture
    })
}

fn apply_minimal_environment(command: &mut Command) {
    const KEYS: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SystemRoot",
        "WINDIR",
        "ComSpec",
        "USERPROFILE",
        "HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
        "LANG",
        "LC_ALL",
    ];
    for key in KEYS {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("NO_COLOR", "1").env("TERM", "dumb");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[cfg(windows)]
    #[test]
    fn binary_path_with_spaces_runs_without_shell() {
        let temp = tempfile::tempdir().expect("tempdir");
        let directory = temp.path().join("binary with spaces");
        std::fs::create_dir(&directory).expect("create binary directory");
        let copied = directory.join("where.exe");
        std::fs::copy(r"C:\Windows\System32\where.exe", &copied).expect("copy executable");
        let spec = PaseoCommandSpec {
            program: copied,
            args: vec!["where".into()],
            timeout_ms: 5_000,
            max_output_bytes: 4_096,
        };
        let output = SystemPaseoCommandRunner.run(&spec).expect("direct process");
        assert_eq!(output.exit_code, Some(0));
    }

    #[test]
    fn bounded_reader_marks_truncation_and_drains_input() {
        let data = vec![b'x'; 16_384];
        let capture = spawn_bounded_reader(std::io::Cursor::new(data), 32)
            .join()
            .expect("reader");
        assert_eq!(capture.bytes.len(), 32);
        assert!(capture.truncated);
    }

    #[cfg(windows)]
    #[test]
    fn command_scripts_are_rejected_before_process_creation() {
        let temp = tempfile::tempdir().expect("tempdir");
        let launcher = temp.path().join("paseo.cmd");
        std::fs::write(&launcher, "@echo off\r\nexit /b 0\r\n").unwrap();
        let error = SystemPaseoCommandRunner
            .run(&PaseoCommandSpec {
                program: launcher,
                args: vec!["--version".into()],
                timeout_ms: 1_000,
                max_output_bytes: 1_024,
            })
            .unwrap_err();
        assert_eq!(error.code, "PASEO_CLI_NOT_FOUND");
    }

    #[test]
    fn timeout_terminates_the_direct_child() {
        #[cfg(windows)]
        let (program, args) = (
            PathBuf::from(r"C:\Windows\System32\ping.exe"),
            vec!["127.0.0.1".into(), "-n".into(), "10".into()],
        );
        #[cfg(not(windows))]
        let (program, args) = (PathBuf::from("/bin/sleep"), vec!["2".into()]);
        let error = SystemPaseoCommandRunner
            .run(&PaseoCommandSpec {
                program,
                args,
                timeout_ms: 20,
                max_output_bytes: 1_024,
            })
            .expect_err("timeout");
        assert_eq!(error.code, "PASEO_COMMAND_TIMEOUT");
    }
}
