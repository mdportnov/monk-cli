use std::path::PathBuf;

use directories::ProjectDirs;
use once_cell::sync::Lazy;

use crate::{Error, Result};

static DIRS: Lazy<Option<ProjectDirs>> = Lazy::new(|| {
    #[cfg(unix)]
    apply_sudo_user_home();
    ProjectDirs::from("dev", "monk", "monk")
});

#[cfg(unix)]
fn apply_sudo_user_home() {
    if std::env::var_os("SUDO_USER").is_none() {
        return;
    }
    let Ok(user) = std::env::var("SUDO_USER") else { return };
    if user == "root" {
        return;
    }
    let home = nix::unistd::User::from_name(&user)
        .ok()
        .flatten()
        .map(|u| u.dir.to_string_lossy().to_string())
        .filter(|s| !s.is_empty());
    if let Some(home) = home {
        std::env::set_var("HOME", &home);
        std::env::set_var("USER", &user);
        std::env::remove_var("XDG_CONFIG_HOME");
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_RUNTIME_DIR");
    }
}

#[cfg(unix)]
pub fn sudo_user_ids() -> Option<(u32, u32)> {
    let uid = std::env::var("SUDO_UID").ok()?.parse().ok()?;
    let gid = std::env::var("SUDO_GID").ok()?.parse().ok()?;
    Some((uid, gid))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn chown_to_sudo_user(path: &std::path::Path) {
    use std::os::unix::ffi::OsStrExt;
    let Some((uid, gid)) = sudo_user_ids() else { return };
    if let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
        unsafe {
            let result = libc::chown(c.as_ptr(), uid, gid);
            if result != 0 {
                tracing::warn!(
                    "chown failed for {}: errno {}",
                    path.display(),
                    std::io::Error::last_os_error()
                );
            }
        }
    }
}

#[cfg(not(unix))]
fn chown_to_sudo_user(_: &std::path::Path) {}

#[cfg(windows)]
#[allow(unsafe_code)]
fn set_windows_acl_current_user(path: &std::path::Path) -> Result<()> {
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
            return Err(Error::Other(format!("SetEntriesInAclW failed: {}", result).into()));
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
            return Err(Error::Other(format!("SetNamedSecurityInfoW failed: {}", result).into()));
        }

        Ok(())
    }
}

fn dirs() -> Result<&'static ProjectDirs> {
    DIRS.as_ref().ok_or_else(|| Error::Other("could not resolve user directories".into()))
}

#[cfg(target_os = "macos")]
const SYSTEM_LAUNCHD_PLIST: &str = "/Library/LaunchDaemons/dev.monk.monkd.plist";
#[cfg(target_os = "macos")]
const SYSTEM_DATA_DIR: &str = "/Library/Application Support/monk";
#[cfg(target_os = "macos")]
const SYSTEM_RUNTIME_DIR: &str = "/var/run/monk";

/// True when the system-wide LaunchDaemon is installed: all paths resolve to
/// system locations regardless of who's calling.
pub fn system_mode() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::path::Path::new(SYSTEM_LAUNCHD_PLIST).exists()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Try to create dir; swallow permission errors so CLI as user doesn't fail
/// when the daemon owns the path. Caller still gets the path back, and
/// downstream open/read calls produce a clear error.
fn ensure_dir_best_effort(p: &std::path::Path) {
    if let Err(e) = fs_err::create_dir_all(p) {
        if e.kind() != std::io::ErrorKind::AlreadyExists && !p.exists() {
            tracing::debug!(?e, path = %p.display(), "create_dir_all failed (continuing)");
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    if system_mode() {
        let p = PathBuf::from(SYSTEM_DATA_DIR);
        ensure_dir_best_effort(&p);
        return Ok(p);
    }
    let p = dirs()?.config_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    chown_to_sudo_user(&p);
    Ok(p)
}

pub fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    if system_mode() {
        let p = PathBuf::from(SYSTEM_DATA_DIR);
        ensure_dir_best_effort(&p);
        return Ok(p);
    }
    let p = dirs()?.data_dir().to_path_buf();
    fs_err::create_dir_all(&p)?;
    chown_to_sudo_user(&p);
    Ok(p)
}

pub fn runtime_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    if system_mode() {
        let p = PathBuf::from(SYSTEM_RUNTIME_DIR);
        ensure_dir_best_effort(&p);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs_err::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
        return Ok(p);
    }
    let p = dirs()?
        .runtime_dir()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("monk"));
    fs_err::create_dir_all(&p)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs_err::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
    }

    #[cfg(windows)]
    {
        if let Err(e) = set_windows_acl_current_user(&p) {
            tracing::warn!("Failed to set Windows ACL on runtime dir: {}", e);
        }
    }

    chown_to_sudo_user(&p);
    Ok(p)
}

pub fn config_file() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

pub fn db_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("monk.sqlite3"))
}

pub fn log_file() -> Result<PathBuf> {
    Ok(data_dir()?.join("monk.log"))
}

pub fn pid_file() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("monkd.pid"))
}

pub fn ipc_socket() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        Ok(PathBuf::from(r"\\.\pipe\monkd"))
    }
    #[cfg(not(windows))]
    {
        Ok(runtime_dir()?.join("monkd.sock"))
    }
}

/// Legacy per-user paths (pre-LaunchDaemon). Used at migration time.
#[cfg(target_os = "macos")]
pub fn legacy_user_config_file_for(user: &str) -> PathBuf {
    PathBuf::from(format!("/Users/{user}/Library/Application Support/dev.monk.monk/config.toml"))
}

#[cfg(target_os = "macos")]
pub fn legacy_user_data_dir_for(user: &str) -> PathBuf {
    PathBuf::from(format!("/Users/{user}/Library/Application Support/dev.monk.monk"))
}

/// One-shot migration: when the system-mode daemon starts for the first time
/// and the system data dir is empty, copy a legacy per-user config + audit
/// log. Idempotent and never overwrites existing files. Preference order:
/// SUDO_USER (the user who ran `sudo monk service install`), then the user
/// with the most-recently-modified config.toml under `/Users/*`.
#[cfg(target_os = "macos")]
pub fn migrate_legacy_if_needed() {
    if !system_mode() {
        return;
    }
    let Ok(sys_dir) = data_dir() else { return };
    let sys_config = sys_dir.join("config.toml");
    if sys_config.exists() {
        return;
    }
    let Some(user) = pick_legacy_user() else { return };
    let legacy_cfg = legacy_user_config_file_for(&user);
    let legacy_data = legacy_user_data_dir_for(&user);
    if !legacy_cfg.exists() {
        return;
    }
    tracing::info!(%user, "migrating legacy per-user data to system path");
    if let Err(e) = fs_err::copy(&legacy_cfg, &sys_config) {
        tracing::warn!(?e, "config migrate failed");
        return;
    }
    let _ = checkpoint_legacy_sqlite(&legacy_data);
    for name in [
        "audit.sqlite3",
        "audit.sqlite3-wal",
        "audit.sqlite3-shm",
        "audit.log",
        "session.lock",
        "apps.json",
    ] {
        let src = legacy_data.join(name);
        let dst = sys_dir.join(name);
        if src.exists() && !dst.exists() {
            if let Err(e) = fs_err::copy(&src, &dst) {
                tracing::warn!(?e, file = %src.display(), "migrate copy failed (continuing)");
            }
        }
    }
    let marker = legacy_data.join(".migrated");
    if fs_err::write(&marker, chrono::Utc::now().to_rfc3339()).is_ok() {
        chown_path_to_user(&marker, &user);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn migrate_legacy_if_needed() {}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn chown_path_to_user(path: &std::path::Path, user: &str) {
    let Ok(Some(u)) = nix::unistd::User::from_name(user) else { return };
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
    unsafe {
        let _ = libc::chown(c.as_ptr(), u.uid.as_raw(), u.gid.as_raw());
    }
}

#[cfg(target_os = "macos")]
fn checkpoint_legacy_sqlite(legacy_data: &std::path::Path) -> Result<()> {
    let db = legacy_data.join("audit.sqlite3");
    if !db.exists() {
        return Ok(());
    }
    let conn = rusqlite::Connection::open(&db).map_err(Error::from)?;
    let _ = conn.pragma_update(None, "wal_checkpoint", "TRUNCATE");
    Ok(())
}

#[cfg(target_os = "macos")]
fn pick_legacy_user() -> Option<String> {
    if let Ok(user) = std::env::var("SUDO_USER") {
        if user != "root" {
            let cfg = legacy_user_config_file_for(&user);
            if cfg.exists() {
                return Some(user);
            }
        }
    }
    let entries = fs_err::read_dir("/Users").ok()?;
    let mut candidates: Vec<(std::time::SystemTime, String)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "Shared" {
            continue;
        }
        let cfg = entry.path().join("Library/Application Support/dev.monk.monk/config.toml");
        if let Ok(meta) = fs_err::metadata(&cfg) {
            let mtime = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            candidates.push((mtime, name));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().next().map(|(_, u)| u)
}
