use crate::Result;
use tracing::{debug, warn};

pub fn check_peer_auth(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    #[cfg(debug_assertions)]
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
    #[cfg(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        check_bsd_peer_creds(stream)
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    {
        debug!("unix peer auth check: relying on filesystem permissions on unsupported platform");
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn check_linux_peer_creds(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon;

    let peer_creds = stream
        .peer_creds()
        .map_err(|e| crate::Error::Ipc(format!("Failed to get peer credentials: {}", e)))?;

    let peer_uid = peer_creds
        .euid()
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

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn check_bsd_peer_creds(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon;

    let peer_creds = stream
        .peer_creds()
        .map_err(|e| crate::Error::Ipc(format!("Failed to get peer credentials: {}", e)))?;

    let peer_uid = peer_creds
        .euid()
        .ok_or_else(|| crate::Error::Ipc("Could not get peer effective UID".to_string()))?;

    // system_mode: socket is 0660 root:admin, so connect() already gated on
    // admin-group membership at the kernel level. trust it.
    if crate::paths::system_mode() {
        debug!(peer_uid, "peer credential check passed (system mode, fs-gated)");
        return Ok(());
    }

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
    } else if let Ok(config_path) = crate::paths::config_dir() {
        if let Ok(metadata) = std::fs::metadata(&config_path) {
            use std::os::unix::fs::MetadataExt;
            metadata.uid()
        } else {
            nix::unistd::getuid().as_raw()
        }
    } else {
        nix::unistd::getuid().as_raw()
    }
}

#[cfg(windows)]
fn check_windows_peer(stream: &interprocess::local_socket::tokio::Stream) -> Result<()> {
    use interprocess::local_socket::traits::StreamCommon;

    let peer_creds = stream
        .peer_creds()
        .map_err(|e| crate::Error::Ipc(format!("Failed to get peer credentials: {}", e)))?;

    if let Some(peer_pid) = peer_creds.pid() {
        if is_same_user_process(peer_pid)? {
            debug!(peer_pid = peer_pid, "peer credential check passed");
            Ok(())
        } else {
            warn!(peer_pid = peer_pid, "unauthorized peer connection attempt blocked");
            Err(crate::Error::Ipc(format!("Peer process {} belongs to different user", peer_pid)))
        }
    } else {
        warn!("could not determine peer process ID");
        Err(crate::Error::Ipc("Could not determine peer process ID".to_string()))
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn is_same_user_process(pid: u32) -> Result<bool> {
    check_windows_peer_sids(pid)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn check_windows_peer_sids(peer_pid: u32) -> Result<bool> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{EqualSid, TOKEN_QUERY};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let current_process = GetCurrentProcess();
        let mut current_token = HANDLE::default();
        OpenProcessToken(current_process, TOKEN_QUERY, &mut current_token)
            .map_err(|e| crate::Error::Ipc(e.to_string()))?;

        let current_sid = match get_process_user_sid(current_token) {
            Ok(sid) => sid,
            Err(e) => {
                let _ = CloseHandle(current_token);
                return Err(e);
            }
        };

        let peer_process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, peer_pid) {
            Ok(handle) if !handle.is_invalid() => handle,
            _ => {
                let _ = CloseHandle(current_token);
                return Err(crate::Error::Ipc(format!("Failed to open peer process {}", peer_pid)));
            }
        };

        let mut peer_token = HANDLE::default();
        if let Err(e) = OpenProcessToken(peer_process, TOKEN_QUERY, &mut peer_token) {
            let _ = CloseHandle(current_token);
            let _ = CloseHandle(peer_process);
            return Err(crate::Error::Ipc(format!("Failed to open peer process token: {}", e)));
        }

        let peer_sid = match get_process_user_sid(peer_token) {
            Ok(sid) => sid,
            Err(e) => {
                let _ = CloseHandle(current_token);
                let _ = CloseHandle(peer_process);
                let _ = CloseHandle(peer_token);
                return Err(e);
            }
        };

        let is_same_user = EqualSid(current_sid, peer_sid).is_ok();

        let _ = CloseHandle(current_token);
        let _ = CloseHandle(peer_process);
        let _ = CloseHandle(peer_token);

        debug!(peer_pid = peer_pid, same_user = is_same_user, "Windows SID peer check");

        Ok(is_same_user)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
unsafe fn get_process_user_sid(
    token: windows::Win32::Foundation::HANDLE,
) -> Result<windows::Win32::Security::PSID> {
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_USER};

    let mut token_info_length = 0u32;
    let _ = GetTokenInformation(token, TokenUser, None, 0, &mut token_info_length);

    if token_info_length == 0 {
        return Err(crate::Error::Ipc("Failed to get token info length".to_string()));
    }

    let mut buffer = vec![0u8; token_info_length as usize];
    GetTokenInformation(
        token,
        TokenUser,
        Some(buffer.as_mut_ptr() as *mut _),
        token_info_length,
        &mut token_info_length,
    )
    .map_err(|e| crate::Error::Ipc(e.to_string()))?;

    let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
    Ok(token_user.User.Sid)
}

#[cfg(test)]
mod tests {
    // Only the `#[cfg(unix)]` test body references `super` items; importing it
    // unconditionally is an unused-import error on Windows under `-D warnings`.
    #[cfg(unix)]
    use super::*;

    #[test]
    fn test_get_allowed_uid() {
        #[cfg(unix)]
        {
            // Valid uid (root=0 is legitimate when daemon owns config_dir in
            // system mode). Just exercise the resolver.
            let _ = get_allowed_uid();
        }
    }
}
