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

/// True when this process can write the canonical config file itself.
///
/// In system mode the file is root-owned (macOS keeps it under
/// `/Library/Application Support/monk`), so the user's CLI must not try —
/// the daemon owns it.
fn owns_config_file() -> bool {
    #[cfg(unix)]
    {
        !crate::paths::system_mode() || nix::unistd::geteuid().is_root()
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Persist a config from a sync action.
///
/// When the daemon owns the file we hand the save to it over IPC — a direct
/// write would fail with EACCES. The IPC call needs a runtime, and this is
/// called from sync code that may itself run inside one, so it goes to a
/// dedicated thread with its own current-thread runtime.
fn persist_config(cfg: crate::config::Config) -> std::result::Result<(), String> {
    if owns_config_file() {
        return cfg.save().map_err(|e| e.to_string());
    }
    std::thread::spawn(move || -> std::result::Result<(), String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let req = crate::ipc::Request::SaveConfig { config: Box::new(cfg) };
            match crate::ipc::send(&req).await {
                Ok(crate::ipc::Response::Ok) => Ok(()),
                Ok(crate::ipc::Response::Error { message }) => Err(message),
                Ok(_) => Err("unexpected response from daemon".to_string()),
                Err(e) => Err(format!(
                    "{e} — the daemon owns the config file; start it, or rerun as `sudo monk doctor --fix`"
                )),
            }
        })
    })
    .join()
    .map_err(|_| "config save thread panicked".to_string())?
}

pub(crate) fn reinstall_service() -> std::result::Result<String, String> {
    crate::platform::elevate_install_service()
        .map(|msg| crate::platform::strip_service_markers(&msg))
        .map_err(|e| e.to_string())
}

pub(crate) fn install_completions() -> std::result::Result<String, String> {
    let shell = detect_shell();
    let home = dirs_home().ok_or("HOME not set")?;
    let (shell_enum, target_path, post_action): (
        clap_complete::Shell,
        std::path::PathBuf,
        Option<String>,
    ) = match shell.as_deref() {
        Some("zsh") => {
            // Canonical XDG site-functions dir; works with the default zsh
            // fpath on macOS and on most Linux setups. Falls back to a
            // user-specific dir + .zshrc append if not already in fpath.
            let dir = home.join(".local/share/zsh/site-functions");
            fs_err::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join("_monk");
            let post = ensure_zshrc_fpath(&home, &dir);
            (clap_complete::Shell::Zsh, target, Some(post))
        }
        Some("bash") => {
            let dir = home.join(".local/share/bash-completion/completions");
            fs_err::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join("monk");
            // That directory is only read when bash-completion v2 is
            // installed *and* sourced — which is never the case on a stock
            // macOS (bash 3.2, no bash-completion). Source the file
            // explicitly so the completions actually work everywhere.
            let post = ensure_bashrc_source(&home, &target);
            (clap_complete::Shell::Bash, target, Some(post))
        }
        Some("fish") => {
            let dir = home.join(".config/fish/completions");
            fs_err::create_dir_all(&dir).map_err(|e| e.to_string())?;
            (clap_complete::Shell::Fish, dir.join("monk.fish"), None)
        }
        // Windows leaves SHELL unset (Git Bash does set it, and lands in the
        // bash arm above). PowerShell keeps completions in a plain script
        // dot-sourced from the profile.
        #[cfg(windows)]
        _ => {
            let profile = powershell_profile()
                .ok_or("could not locate the PowerShell profile — run `monk completions powershell` manually")?;
            let dir =
                profile.parent().ok_or("PowerShell profile has no parent directory")?.to_path_buf();
            fs_err::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = dir.join("monk.completion.ps1");
            let post = ensure_profile_source(&profile, &target);
            (clap_complete::Shell::PowerShell, target, Some(post))
        }
        #[cfg(not(windows))]
        other => {
            return Err(format!(
                "unsupported shell: {} — run `monk completions <SHELL>` manually",
                other.unwrap_or("?")
            ));
        }
    };
    let mut cmd = crate::cli::cmd_factory();
    let mut buf: Vec<u8> = Vec::new();
    clap_complete::generate(shell_enum, &mut cmd, "monk", &mut buf);
    fs_err::write(&target_path, &buf).map_err(|e| e.to_string())?;
    let mut msg = format!("wrote completions → {}", target_path.display());
    if let Some(extra) = post_action {
        msg.push('\n');
        msg.push_str(&extra);
    }
    Ok(msg)
}

const RC_MARKER: &str = "# added by monk doctor --fix";

/// Append a guarded block to a shell rc file, idempotently.
///
/// Idempotence uses the maintenance marker line — checking only for the
/// directory string is unsafe: a comment like `# skip ~/.local/share/zsh/...`
/// would falsely look like an install. `already_present` additionally covers
/// the case where the user wired it up by hand.
fn append_rc_block(
    rc: &std::path::Path,
    block: &str,
    manual_hint: &str,
    already_present: impl Fn(&str) -> bool,
) -> String {
    let existing = fs_err::read_to_string(rc).unwrap_or_default();
    if existing.contains(RC_MARKER) {
        return format!("({} block already installed by monk)", rc.display());
    }
    if existing.lines().any(already_present) {
        return format!("({} already wired up)", rc.display());
    }
    let payload = format!("\n{RC_MARKER}\n{block}\n");
    match fs_err::OpenOptions::new().create(true).append(true).open(rc) {
        Ok(mut f) => {
            use std::io::Write;
            if let Err(e) = f.write_all(payload.as_bytes()) {
                return format!(
                    "could not append to {} ({e}); add manually:\n{manual_hint}",
                    rc.display()
                );
            }
            format!("appended a completions block to {} — restart your shell", rc.display())
        }
        Err(e) => {
            format!("could not open {} ({e}); add manually:\n{manual_hint}", rc.display())
        }
    }
}

/// Ask the host PowerShell where its profile lives; `pwsh` (7+) and
/// `powershell` (5.1) use different paths, so we do not guess.
#[cfg(windows)]
pub(crate) fn powershell_profile() -> Option<std::path::PathBuf> {
    for exe in ["pwsh", "powershell"] {
        let out = std::process::Command::new(exe)
            .args(["-NoProfile", "-NonInteractive", "-Command", "$PROFILE"])
            .output();
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(std::path::PathBuf::from(path));
        }
    }
    None
}

/// Make sure the PowerShell profile dot-sources the generated completion
/// script.
#[cfg(windows)]
fn ensure_profile_source(profile: &std::path::Path, file: &std::path::Path) -> String {
    let file_s = file.display().to_string();
    let line = format!(". \"{file_s}\"");
    let manual = format!("  {line}");
    append_rc_block(profile, &line, &manual, |l| dot_source_line_references(l, &file_s))
}

/// Make sure `.zshrc` contains an `fpath` entry covering `dir`.
fn ensure_zshrc_fpath(home: &std::path::Path, dir: &std::path::Path) -> String {
    let dir_s = dir.display().to_string();
    let block = format!("fpath=({dir_s} $fpath)\nautoload -U compinit && compinit");
    let manual = format!("  fpath=({dir_s} $fpath)\n  autoload -U compinit && compinit");
    append_rc_block(&home.join(".zshrc"), &block, &manual, |l| zshrc_line_uses_fpath_dir(l, &dir_s))
}

/// Make sure the bash rc file sources the generated completion file.
///
/// On macOS an interactive login shell reads `~/.bash_profile`, on Linux
/// `~/.bashrc`; prefer whichever already exists.
fn ensure_bashrc_source(home: &std::path::Path, file: &std::path::Path) -> String {
    let file_s = file.display().to_string();
    let profile = home.join(".bash_profile");
    let bashrc = home.join(".bashrc");
    let rc = if cfg!(target_os = "macos") && (profile.exists() || !bashrc.exists()) {
        profile
    } else {
        bashrc
    };
    let line = format!("[ -f \"{file_s}\" ] && . \"{file_s}\"");
    let manual = format!("  {line}");
    append_rc_block(&rc, &line, &manual, |l| dot_source_line_references(l, &file_s))
}

/// True if `line` is a non-commented `source`/`.` statement referencing
/// `file`. Shared by the bash rc and the PowerShell profile — both use `.`
/// for dot-sourcing and `#` for comments.
fn dot_source_line_references(line: &str, file: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    (trimmed.starts_with("source ") || trimmed.starts_with(". ") || trimmed.starts_with("[ -f"))
        && trimmed.contains(file)
}

/// True if `line` is a non-commented zsh statement that contains an `fpath`
/// assignment or append referencing `dir`. Best-effort — covers the common
/// forms `fpath=(/dir $fpath)`, `fpath+=(/dir)`, `fpath=("/dir" $fpath)`.
fn zshrc_line_uses_fpath_dir(line: &str, dir: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    if !trimmed.starts_with("fpath") {
        return false;
    }
    let eq = trimmed.find('=').or_else(|| trimmed.find('+'));
    let Some(idx) = eq else { return false };
    trimmed[idx..].contains(dir)
}

/// Remove references to uninstalled apps from every profile in config.toml.
///
/// Forces a fresh app scan first — pruning against a stale cache could drop
/// ids for apps the user reinstalled since the last scan. Only direct
/// `profile.apps` entries are touched; brand-derived ids live in bundled
/// presets, not in the user's config.
pub(crate) fn prune_stale_app_refs() -> std::result::Result<String, String> {
    let mut cfg = crate::config::Config::load().map_err(|e| e.to_string())?;
    let cache = crate::apps::load_or_scan(true).map_err(|e| format!("app scan failed: {e}"))?;
    let mut removed = Vec::new();
    for (name, profile) in cfg.profiles.iter_mut() {
        let stale = crate::apps::resolve(&profile.apps, &cache).stale;
        if stale.is_empty() {
            continue;
        }
        profile.apps.retain(|id| !stale.contains(id));
        removed.push(format!("{name}: removed {}", stale.join(", ")));
    }
    if removed.is_empty() {
        return Ok("nothing to remove — a fresh scan found all referenced apps installed".into());
    }
    persist_config(cfg)?;
    let mut msg = removed.join("\n");
    msg.push_str("\nconfig.toml updated");
    Ok(msg)
}

/// Move a broken config.toml aside (timestamped backup) and write a fresh
/// default config so monk becomes usable again without hand-editing toml.
pub(crate) fn reset_config() -> std::result::Result<String, String> {
    let path = crate::paths::config_file().map_err(|e| e.to_string())?;
    if !path.exists() {
        return Err("config file does not exist — nothing to reset".into());
    }
    // A broken config usually means the daemon never came up, so IPC is not
    // an option here: the file has to be rewritten in place, and in system
    // mode only root may do that.
    if !owns_config_file() {
        return Err(format!(
            "{} is owned by the system daemon — rerun as root: `sudo monk doctor --fix`",
            path.display()
        ));
    }
    let backup =
        path.with_extension(format!("toml.bak-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")));
    fs_err::rename(&path, &backup).map_err(|e| e.to_string())?;
    crate::config::Config::default().save().map_err(|e| e.to_string())?;
    Ok(format!(
        "backed up old config → {}\nwrote a fresh default config — your profiles from the backup can be copied back by hand",
        backup.display()
    ))
}

pub(crate) fn print_path_hint() -> std::result::Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("binary has no parent dir")?.to_path_buf();
    let shell = detect_shell();
    let line = format!("export PATH=\"{}:$PATH\"", parent.display());
    let target = match shell.as_deref() {
        Some("zsh") => "~/.zshrc",
        Some("bash") => "~/.bashrc",
        Some("fish") => return Ok(format!("run: fish_add_path {}", parent.display())),
        _ => "your shell rc file",
    };
    Ok(format!("add this to {target}:\n  {line}"))
}

fn detect_shell() -> Option<String> {
    std::env::var("SHELL").ok().and_then(|s| {
        std::path::Path::new(&s).file_name().map(|n| n.to_string_lossy().to_lowercase())
    })
}

fn dirs_home() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf())
}

pub(crate) fn open_path_action(
    p: crate::Result<std::path::PathBuf>,
) -> std::result::Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fpath_line_detection_accepts_canonical_forms() {
        let d = "/home/u/.local/share/zsh/site-functions";
        assert!(zshrc_line_uses_fpath_dir(&format!("fpath=({d} $fpath)"), d));
        assert!(zshrc_line_uses_fpath_dir(&format!("  fpath+=({d})"), d));
        assert!(zshrc_line_uses_fpath_dir(&format!("fpath=(\"{d}\" $fpath)"), d));
    }

    #[test]
    fn bash_source_line_detection() {
        let f = "/home/u/.local/share/bash-completion/completions/monk";
        assert!(dot_source_line_references(&format!(". \"{f}\""), f));
        assert!(dot_source_line_references(&format!("source {f}"), f));
        assert!(dot_source_line_references(&format!("[ -f \"{f}\" ] && . \"{f}\""), f));
        assert!(!dot_source_line_references(&format!("# source {f}"), f));
        assert!(!dot_source_line_references("source /other/file", f));
        assert!(!dot_source_line_references(&format!("echo {f}"), f));
    }

    #[test]
    fn fpath_line_detection_rejects_comments_and_unrelated_lines() {
        let d = "/home/u/.local/share/zsh/site-functions";
        assert!(!zshrc_line_uses_fpath_dir(&format!("# skip {d}"), d));
        assert!(!zshrc_line_uses_fpath_dir(&format!("export FOO={d}"), d));
        assert!(!zshrc_line_uses_fpath_dir("fpath=(/other/dir $fpath)", d));
        assert!(!zshrc_line_uses_fpath_dir("alias fpath='echo'", d));
    }
}
