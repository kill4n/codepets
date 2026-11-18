//! Says who is holding our port, so a bind failure reads "in use by buddy (pid 66288)" instead of a bare OS error.

/// Parses `lsof -F pc` output (`p<pid>` then `c<command>` lines) into the first listener.
pub fn parse_lsof_owner(out: &str) -> Option<(u32, String)> {
    let mut pid = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') { pid = p.parse().ok(); }
        else if let (Some(c), Some(p)) = (line.strip_prefix('c'), pid) { return Some((p, c.to_string())); }
    }
    None
}

pub fn bind_failure_message(addr: &str, err: &str, owner: Option<(u32, String)>) -> String {
    match owner {
        Some((pid, cmd)) => format!("port {addr} is in use by {cmd} (pid {pid}): quit it and relaunch CodePets"),
        None => format!("hooks server could not bind {addr}: {err}"),
    }
}

/// Best effort (macOS and Linux have lsof; elsewhere the owner stays unknown).
pub fn find_owner(port: u16) -> Option<(u32, String)> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpc"]).output().ok()?;
        parse_lsof_owner(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(unix))]
    { let _ = port; None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn lsof_output_gives_the_first_listener() {
        assert_eq!(parse_lsof_owner("p66288\ncbuddy\np7\ncother\n"), Some((66288, "buddy".into())));
    }
    #[test] fn empty_or_malformed_lsof_output_gives_nobody() {
        assert_eq!(parse_lsof_owner(""), None);
        assert_eq!(parse_lsof_owner("pabc\ncx\n"), None);
    }
    #[test] fn the_message_names_the_owner_when_known() {
        let m = bind_failure_message("127.0.0.1:7433", "Address in use", Some((66288, "buddy".into())));
        assert!(m.contains("buddy") && m.contains("66288") && m.contains("7433"), "{m}");
    }
    #[test] fn the_message_falls_back_to_the_os_error() {
        assert_eq!(bind_failure_message("127.0.0.1:7433", "denied", None), "hooks server could not bind 127.0.0.1:7433: denied");
    }
}
