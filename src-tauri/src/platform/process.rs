use std::ffi::OsStr;
use std::process::Command;

/// Build a child process that must not allocate a console window on Windows.
pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    configure_background_command(&mut command);
    command
}

pub fn configure_background_command(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    #[cfg(not(windows))]
    let _ = command;
}

pub fn configure_background_tokio_command(command: &mut tokio::process::Command) {
    configure_background_command(command.as_std_mut());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_command_preserves_the_requested_program() {
        let command = background_command("git");
        assert_eq!(command.get_program(), "git");
    }

    #[cfg(windows)]
    #[test]
    fn background_command_does_not_allocate_a_console_window() {
        let script = r#"
Add-Type -Name NativeConsole -Namespace Win32 -MemberDefinition '[System.Runtime.InteropServices.DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();'
[Win32.NativeConsole]::GetConsoleWindow().ToInt64()
"#;
        let output = background_command("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .expect("run hidden PowerShell probe");

        assert!(
            output.status.success(),
            "PowerShell probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
    }
}
