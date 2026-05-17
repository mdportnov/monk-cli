use std::path::Path;

use crate::{Error, Result};

pub fn apply_sudo_user_home() {}

pub fn sudo_user_ids() -> Option<(u32, u32)> {
    None
}

pub fn chown_to_sudo_user(_path: &Path) {}

pub fn system_mode() -> bool {
    false
}

pub fn system_data_dir() -> Option<&'static str> {
    None
}

pub fn system_runtime_dir() -> Option<&'static str> {
    None
}

pub fn migrate_legacy_if_needed() {}

#[allow(unsafe_code)]
pub fn set_acl_current_user(path: &Path) -> Result<()> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL, PSID};
    use windows::Win32::Security::{
        GetTokenInformation, SetEntriesInAclW, SetNamedSecurityInfoW, TokenUser,
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, EXPLICIT_ACCESSW, GENERIC_ALL,
        GRANT_ACCESS, NO_MULTIPLE_TRUSTEE, OBJECT_INHERIT_ACE, SE_FILE_OBJECT, TOKEN_QUERY,
        TOKEN_USER, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let current_process = GetCurrentProcess();
        let mut token: HANDLE = HANDLE::default();
        if !OpenProcessToken(current_process, TOKEN_QUERY, &mut token).as_bool() {
            return Err(Error::Other("Failed to open current process token".into()));
        }

        let mut token_info_length = 0u32;
        GetTokenInformation(token, TokenUser, Some(ptr::null_mut()), 0, &mut token_info_length);

        if token_info_length == 0 {
            CloseHandle(token);
            return Err(Error::Other("Failed to get token info length".into()));
        }

        let token_info = libc::malloc(token_info_length as usize);
        if token_info.is_null() {
            CloseHandle(token);
            return Err(Error::Other("Memory allocation failed".into()));
        }

        if !GetTokenInformation(
            token,
            TokenUser,
            Some(token_info),
            token_info_length,
            &mut token_info_length,
        )
        .as_bool()
        {
            libc::free(token_info);
            CloseHandle(token);
            return Err(Error::Other("Failed to get token information".into()));
        }

        let token_user = &*(token_info as *const TOKEN_USER);
        let user_sid = token_user.User.Sid;

        let mut ea = EXPLICIT_ACCESSW {
            grfAccessPermissions: GENERIC_ALL,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: PWSTR(user_sid as *mut u16),
            },
        };

        let mut new_acl = ptr::null_mut();
        let result = SetEntriesInAclW(1, &mut ea, None, &mut new_acl);
        if result != 0 {
            libc::free(token_info);
            CloseHandle(token);
            return Err(Error::Other(format!("SetEntriesInAclW failed: {}", result)));
        }

        let path_wide: Vec<u16> = OsString::from(path).encode_wide().chain(Some(0)).collect();
        let result = SetNamedSecurityInfoW(
            PWSTR(path_wide.as_ptr() as *mut u16),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            PSID::default(),
            PSID::default(),
            Some(new_acl),
            None,
        );

        if !new_acl.is_null() {
            LocalFree(HLOCAL(new_acl as isize));
        }
        libc::free(token_info);
        CloseHandle(token);

        if result != 0 {
            return Err(Error::Other(format!("SetNamedSecurityInfoW failed: {}", result)));
        }

        Ok(())
    }
}

pub fn install_service(bin: &str) -> Result<String> {
    let status = std::process::Command::new("schtasks")
        .args(["/Create", "/F", "/SC", "ONLOGON", "/RL", "HIGHEST", "/TN", "monkd", "/TR"])
        .arg(format!("\"{bin}\" daemon run"))
        .status()?;
    if !status.success() {
        return Err(Error::Other("schtasks /Create failed".into()));
    }
    Ok("installed scheduled task `monkd` (runs at logon, admin)".into())
}

pub fn uninstall_service(purge: bool) -> Result<String> {
    let mut msgs = Vec::new();

    if super::try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    let _ = std::process::Command::new("schtasks").args(["/End", "/TN", "monkd"]).status();
    let status =
        std::process::Command::new("schtasks").args(["/Delete", "/F", "/TN", "monkd"]).status()?;
    if !status.success() {
        return Err(Error::Other("schtasks /Delete failed".into()));
    }
    msgs.push("removed scheduled task `monkd`".into());

    super::cleanup_hosts();

    if let Ok(runtime_dir) = crate::paths::runtime_dir() {
        let _ = fs_err::remove_dir_all(&runtime_dir);
    }

    if purge {
        if let Ok(data_dir) = crate::paths::data_dir() {
            let _ = fs_err::remove_dir_all(&data_dir);
            msgs.push("purged user data".into());
        }
        if let Ok(config_dir) = crate::paths::config_dir() {
            let _ = fs_err::remove_dir_all(&config_dir);
            msgs.push("purged config".into());
        }
    }

    Ok(msgs.join(", "))
}
