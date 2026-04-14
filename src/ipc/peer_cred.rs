use crate::Result;
use tracing::{debug, warn};

pub fn check_peer_auth(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    if std::env::var("MONK_DISABLE_PEER_CHECK").as_deref() == Ok("1") {
        warn!("peer auth check disabled via MONK_DISABLE_PEER_CHECK=1");
        return Ok(());
    }

    #[cfg(unix)]
    {
        check_unix_peer(stream)
    }
    #[cfg(windows)]
    {
        check_windows_peer(stream)
    }
}

#[cfg(unix)]
fn check_unix_peer(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        check_linux_peer_creds(stream)
    }
    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
    {
        check_bsd_peer_creds(stream)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd")))]
    {
        debug!("unix peer auth check: relying on filesystem permissions on unsupported platform");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn check_linux_peer_creds(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon;

    let peer_creds = stream.peer_creds()
        .map_err(|e| crate::Error::Ipc(format!("Failed to get peer credentials: {}", e)))?;

    let peer_uid = peer_creds.euid()
        .ok_or_else(|| crate::Error::Ipc("Could not get peer effective UID".to_string()))?;
    let allowed_uid = get_allowed_uid();

    if peer_uid != allowed_uid {
        warn!(
            peer_uid = peer_uid,
            allowed_uid = allowed_uid,
            "unauthorized peer connection attempt blocked"
        );
        return Err(crate::Error::Ipc(format!(
            "Peer UID {} does not match allowed UID {}",
            peer_uid, allowed_uid
        )));
    }

    debug!(peer_uid = peer_uid, "peer credential check passed");
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
fn check_bsd_peer_creds(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon;

    let peer_creds = stream.peer_creds()
        .map_err(|e| crate::Error::Ipc(format!("Failed to get peer credentials: {}", e)))?;

    let peer_uid = peer_creds.euid()
        .ok_or_else(|| crate::Error::Ipc("Could not get peer effective UID".to_string()))?;
    let allowed_uid = get_allowed_uid();

    if peer_uid != allowed_uid {
        warn!(
            peer_uid = peer_uid,
            allowed_uid = allowed_uid,
            "unauthorized peer connection attempt blocked"
        );
        return Err(crate::Error::Ipc(format!(
            "Peer UID {} does not match allowed UID {}",
            peer_uid, allowed_uid
        )));
    }

    debug!(peer_uid = peer_uid, "peer credential check passed");
    Ok(())
}

#[cfg(unix)]
#[allow(dead_code)]
fn get_allowed_uid() -> u32 {
    if let Some((sudo_uid, _)) = crate::paths::sudo_user_ids() {
        sudo_uid
    } else {
        nix::unistd::getuid().as_raw()
    }
}


#[cfg(windows)]
fn check_windows_peer(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon;

    let peer_creds = stream.peer_creds()
        .map_err(|e| crate::Error::Ipc(format!("Failed to get peer credentials: {}", e)))?;

    if let Some(peer_pid) = peer_creds.pid() {
        if is_same_user_process(peer_pid)? {
            debug!(peer_pid = peer_pid, "peer credential check passed");
            Ok(())
        } else {
            warn!(peer_pid = peer_pid, "unauthorized peer connection attempt blocked");
            Err(crate::Error::Ipc(format!(
                "Peer process {} belongs to different user",
                peer_pid
            )))
        }
    } else {
        warn!("could not determine peer process ID");
        Err(crate::Error::Ipc("Could not determine peer process ID".to_string()))
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn is_same_user_process(pid: u32) -> Result<bool> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let output = Command::new("wmic")
        .args(&["process", "where", &format!("processid={}", pid), "get", "name,owner", "/format:csv"])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| crate::Error::Ipc(format!("Failed to execute wmic: {}", e)))?;

    if !output.status.success() {
        return Err(crate::Error::Ipc(format!("wmic failed with status: {}", output.status)));
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    let current_user = std::env::var("USERNAME")
        .map_err(|_| crate::Error::Ipc("Could not get current username".to_string()))?;

    let is_same_user = output_str.lines()
        .any(|line| line.contains(&current_user) && line.contains(&pid.to_string()));

    debug!(
        peer_pid = pid,
        current_user = %current_user,
        same_user = is_same_user,
        "Windows peer user check"
    );

    Ok(is_same_user)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_allowed_uid() {
        #[cfg(unix)]
        {
            let uid = get_allowed_uid();
            assert!(uid > 0, "UID should be positive");
        }
    }
}

