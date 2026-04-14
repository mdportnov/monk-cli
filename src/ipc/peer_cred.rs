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
    check_windows_peer_sids(pid)
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn check_windows_peer_sids(peer_pid: u32) -> Result<bool> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LocalFree, HLOCAL};
    use windows::Win32::Security::{GetTokenInformation, EqualSid, TokenUser, TOKEN_USER, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
    use std::mem;
    use std::ptr;

    unsafe {
        let current_process = GetCurrentProcess();
        let mut current_token: HANDLE = HANDLE::default();
        if !OpenProcessToken(current_process, TOKEN_QUERY, &mut current_token).as_bool() {
            return Err(crate::Error::Ipc("Failed to open current process token".to_string()));
        }

        let current_sid = match get_process_user_sid(current_token) {
            Ok(sid) => sid,
            Err(e) => {
                CloseHandle(current_token);
                return Err(e);
            }
        };

        let peer_process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, peer_pid) {
            Ok(handle) if !handle.is_invalid() => handle,
            _ => {
                if !current_sid.is_null() {
                    LocalFree(HLOCAL(current_sid as isize));
                }
                CloseHandle(current_token);
                return Err(crate::Error::Ipc(format!("Failed to open peer process {}", peer_pid)));
            }
        };

        let mut peer_token: HANDLE = HANDLE::default();
        if !OpenProcessToken(peer_process, TOKEN_QUERY, &mut peer_token).as_bool() {
            if !current_sid.is_null() {
                LocalFree(HLOCAL(current_sid as isize));
            }
            CloseHandle(current_token);
            CloseHandle(peer_process);
            return Err(crate::Error::Ipc("Failed to open peer process token".to_string()));
        }

        let peer_sid = match get_process_user_sid(peer_token) {
            Ok(sid) => sid,
            Err(e) => {
                if !current_sid.is_null() {
                    LocalFree(HLOCAL(current_sid as isize));
                }
                CloseHandle(current_token);
                CloseHandle(peer_process);
                CloseHandle(peer_token);
                return Err(e);
            }
        };

        let is_same_user = !current_sid.is_null() && !peer_sid.is_null() &&
            EqualSid(current_sid, peer_sid).as_bool();

        if !current_sid.is_null() {
            LocalFree(HLOCAL(current_sid as isize));
        }
        if !peer_sid.is_null() {
            LocalFree(HLOCAL(peer_sid as isize));
        }
        CloseHandle(current_token);
        CloseHandle(peer_process);
        CloseHandle(peer_token);

        debug!(
            peer_pid = peer_pid,
            same_user = is_same_user,
            "Windows SID peer check"
        );

        Ok(is_same_user)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
unsafe fn get_process_user_sid(token: HANDLE) -> Result<*mut core::ffi::c_void> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_USER};
    use std::mem;
    use std::ptr;

    let mut token_info_length = 0u32;
    GetTokenInformation(token, TokenUser, Some(ptr::null_mut()), 0, &mut token_info_length);

    if token_info_length == 0 {
        return Err(crate::Error::Ipc("Failed to get token info length".to_string()));
    }

    let token_info = libc::malloc(token_info_length as usize);
    if token_info.is_null() {
        return Err(crate::Error::Ipc("Memory allocation failed".to_string()));
    }

    if !GetTokenInformation(
        token,
        TokenUser,
        Some(token_info),
        token_info_length,
        &mut token_info_length,
    ).as_bool() {
        libc::free(token_info);
        return Err(crate::Error::Ipc("Failed to get token information".to_string()));
    }

    let token_user = &*(token_info as *const TOKEN_USER);
    let sid = token_user.User.Sid;
    libc::free(token_info);

    Ok(sid)
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

