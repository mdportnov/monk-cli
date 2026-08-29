use std::path::Path;

use crate::{Error, Result};

// sudo/chown concepts don't exist on Windows — no-ops for API symmetry
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

/// Fire-and-forget Windows toast via PowerShell WinRT bridge.
/// Uses the PowerShell AppUserModelId so no app registration is needed.
/// Silently ignored on failure — notifications are best-effort.
pub fn notify(title: &str, body: &str) {
    let title = title.replace('"', "'");
    let body = body.replace('"', "'");
    // PowerShell.exe has a known registered AUMID, so toasts show without
    // requiring monk to be registered in the Start menu.
    let ps_id = r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";
    let script = format!(
        r#"try {{
$app = '{ps_id}'
[void][Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]
$t = [Windows.UI.Notifications.ToastTemplateType]::ToastText02
$xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent($t)
$xml.GetElementsByTagName('text').Item(0).AppendChild($xml.CreateTextNode("{title}")) | Out-Null
$xml.GetElementsByTagName('text').Item(1).AppendChild($xml.CreateTextNode("{body}")) | Out-Null
$n = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($app).Show($n)
}} catch {{}}
"#
    );
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn elevate_install_service() -> Result<String> {
    let bin = std::env::current_exe()?.to_string_lossy().into_owned();
    install_service(&bin)
}

/// Windows has no in-process elevation path here — the caller is expected to
/// already run in an Administrator terminal, same as for the install.
pub fn elevate_uninstall_service(purge: bool) -> Result<String> {
    uninstall_service(purge)
}

/// Restrict the ACL on `path` to the current user only (no inheriting from
/// parent, replaces any existing DACL). Called on the runtime socket dir so
/// other local users cannot connect to the IPC socket.
#[allow(unsafe_code)]
pub fn set_acl_current_user(path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL};
    use windows::Win32::Security::Authorization::{
        SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W, GRANT_ACCESS,
        NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    };
    use windows::Win32::Security::{
        GetTokenInformation, TokenUser, ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION,
        OBJECT_INHERIT_ACE, TOKEN_QUERY, TOKEN_USER,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // GENERIC_ALL (0x10000000) — not re-exported as a constant in Win32::Security
    // in windows-rs 0.58, so use the raw ACCESS_MASK value.
    const GENERIC_ALL: u32 = 0x1000_0000;

    unsafe {
        let current_process = GetCurrentProcess();
        let mut token = HANDLE::default();
        OpenProcessToken(current_process, TOKEN_QUERY, &mut token)
            .map_err(|e| Error::Other(e.to_string()))?;

        // First call: get required buffer size (expected to fail with
        // ERROR_INSUFFICIENT_BUFFER — ignore the error, check length below).
        let mut token_info_length = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut token_info_length);

        if token_info_length == 0 {
            let _ = CloseHandle(token);
            return Err(Error::Other("GetTokenInformation: zero length".into()));
        }

        let mut buffer = vec![0u8; token_info_length as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut _),
            token_info_length,
            &mut token_info_length,
        )
        .map_err(|e| Error::Other(e.to_string()))?;

        let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
        // PSID is *mut c_void; reinterpret as *mut u16 for TRUSTEE_W.ptstrName
        // when TrusteeForm == TRUSTEE_IS_SID (standard Win32 pattern).
        let user_sid = token_user.User.Sid;

        let ea = EXPLICIT_ACCESS_W {
            grfAccessPermissions: GENERIC_ALL,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: PWSTR(user_sid.0 as *mut u16),
            },
        };

        // windows crate 0.58: slice-based API (count is implicit in slice len).
        // Returns WIN32_ERROR — convert to Result via .ok().
        let mut new_acl: *mut ACL = ptr::null_mut();
        SetEntriesInAclW(Some(&[ea]), None, &mut new_acl)
            .ok()
            .map_err(|e| Error::Other(e.to_string()))?;

        let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let set_result = SetNamedSecurityInfoW(
            PCWSTR(path_wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(new_acl as *const ACL),
            None,
        )
        .ok()
        .map_err(|e| Error::Other(e.to_string()));

        // Always free the ACL and token handle regardless of SetNamedSecurityInfoW outcome.
        if !new_acl.is_null() {
            // windows-rs 0.58: LocalFree takes `P: Param<HLOCAL>`, satisfied by
            // HLOCAL directly (not Option<HLOCAL>). Returned handle is ignored.
            LocalFree(HLOCAL(new_acl as *mut _));
        }
        let _ = CloseHandle(token);

        set_result
    }
}

pub fn install_service(bin: &str) -> Result<String> {
    // Quote the binary path to handle spaces (e.g. "C:\Program Files\...").
    let out = std::process::Command::new("schtasks")
        .args(["/Create", "/F", "/SC", "ONLOGON", "/RL", "HIGHEST", "/TN", "monkd", "/TR"])
        .arg(format!("\"{bin}\" daemon run"))
        .output()?;
    if !out.status.success() {
        // `/RL HIGHEST` needs an elevated terminal — by far the most common
        // reason to land here, and schtasks' own wording is localized.
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason =
            stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("schtasks /Create failed");
        return Err(Error::Permission(format!(
            "{reason} — open PowerShell as Administrator and run `monk service install` again"
        )));
    }
    let mut msgs = vec!["installed scheduled task `monkd` (runs at logon, elevated)".to_string()];

    // ONLOGON alone would leave the daemon down until the next sign-in, so
    // the install would "succeed" with nothing running — start it now, the
    // way launchd's bootstrap and systemd's `enable --now` do.
    match run_task() {
        Ok(()) => msgs.push("started it now".into()),
        Err(e) => {
            tracing::warn!(?e, "schtasks /Run failed");
            msgs.push(format!("NOT started: {e} — start it with `schtasks /Run /TN monkd`"));
        }
    }
    Ok(msgs.join("\n"))
}

/// Start the logon task immediately.
fn run_task() -> Result<()> {
    let out = std::process::Command::new("schtasks").args(["/Run", "/TN", "monkd"]).output()?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(Error::Other(
        stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("schtasks /Run failed").to_string(),
    ))
}

/// True when the logon task exists. Used to keep uninstall idempotent and to
/// decide whether a restart makes sense.
fn task_exists() -> bool {
    std::process::Command::new("schtasks")
        .args(["/Query", "/TN", "monkd"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Bounce the task so it picks up a replaced binary.
pub fn restart_service() -> Option<Result<String>> {
    if !task_exists() {
        return None;
    }
    let _ = std::process::Command::new("schtasks").args(["/End", "/TN", "monkd"]).output();
    Some(run_task().map(|()| "restarted scheduled task `monkd`".to_string()))
}

pub fn uninstall_service(purge: bool) -> Result<String> {
    let mut msgs: Vec<String> = Vec::new();

    if super::try_shutdown_daemon().is_err() {
        tracing::debug!("daemon shutdown failed during uninstall");
    }

    // Removing what is not there is a no-op, not an error: `uninstall` has to
    // stay runnable after a partial or failed install.
    if task_exists() {
        let _ = std::process::Command::new("schtasks").args(["/End", "/TN", "monkd"]).output();
        let out = std::process::Command::new("schtasks")
            .args(["/Delete", "/F", "/TN", "monkd"])
            .output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let reason =
                stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("schtasks /Delete failed");
            return Err(Error::Permission(format!(
                "{reason} — open PowerShell as Administrator and run `monk service uninstall` again"
            )));
        }
        msgs.push("removed scheduled task `monkd`".into());
    }

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
