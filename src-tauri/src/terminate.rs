//! Ending a Claude Code session from the pet: ask its `claude` process to exit.

/// Ask the process to terminate (SIGTERM on Unix, `taskkill` without `/F` on Windows).
/// Refuses pids that could never be a session (0, 1) or this very app.
pub fn terminate(pid: u32) -> Result<(), String> {
    if pid <= 1 || pid == std::process::id() { return Err(format!("refusing to terminate pid {pid}")); }
    #[cfg(unix)]
    let status = std::process::Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    #[cfg(windows)]
    let status = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("taskkill").args(["/PID", &pid.to_string()]).creation_flags(0x0800_0000).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("could not terminate pid {pid} ({s})")),
        Err(e) => Err(format!("could not run the terminate command: {e}")),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test] fn terminates_a_running_process() {
        let mut child = std::process::Command::new("sleep").arg("30").spawn().unwrap();
        terminate(child.id()).unwrap();
        let st = child.wait().unwrap();
        assert!(!st.success(), "killed by the signal");
    }
    #[test] fn refuses_init_and_itself() {
        assert!(terminate(0).is_err());
        assert!(terminate(1).is_err());
        assert!(terminate(std::process::id()).is_err());
    }
}
