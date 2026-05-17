
pub(crate) fn start_daemon() -> std::result::Result<String, String> {
    if let Ok(pf) = crate::daemon::PidFile::new() {
        if let Ok(Some(pid)) = pf.is_alive() {
            return Err(format!("daemon already running (pid {pid})"));
        }
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    use std::process::{Command, Stdio};
    Command::new(exe)
        .args(["daemon", "run"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;
    Ok("spawned `monk daemon run` in background".into())
}

pub(crate) fn stop_daemon() -> std::result::Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let out = std::process::Command::new(exe)
        .args(["daemon", "stop"])
        .output()
        .map_err(|e| format!("spawn failed: {e}"))?;
    if out.status.success() {
        Ok("daemon stop requested".into())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("monk daemon stop exited with {}", out.status)
        } else {
            err
        })
    }
}

pub(crate) fn open_path_action(p: crate::Result<std::path::PathBuf>) -> std::result::Result<String, String> {
    let path = p.map_err(|e| e.to_string())?;
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let status =
        std::process::Command::new(cmd).arg(&path).status().map_err(|e| format!("{cmd}: {e}"))?;
    if !status.success() {
        return Err(format!("{cmd} exited with {status}"));
    }
    Ok(format!("opened {}", path.display()))
}