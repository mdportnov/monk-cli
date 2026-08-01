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

pub(crate) fn reinstall_service() -> std::result::Result<String, String> {
    crate::platform::elevate_install_service().map_err(|e| e.to_string())
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
            (clap_complete::Shell::Bash, dir.join("monk"), None)
        }
        Some("fish") => {
            let dir = home.join(".config/fish/completions");
            fs_err::create_dir_all(&dir).map_err(|e| e.to_string())?;
            (clap_complete::Shell::Fish, dir.join("monk.fish"), None)
        }
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

/// Make sure `.zshrc` contains an `fpath` entry covering `dir`, idempotently.
/// Returns a human-readable status line.
///
/// Idempotence uses a maintenance marker line (`MARKER`) — checking only the
/// directory string is unsafe: a comment like `# skip ~/.local/share/zsh/...`
/// would falsely look like an install. We also scan for an active `fpath=`
/// line that actually references the directory to cover the case where the
/// user added it by hand.
fn ensure_zshrc_fpath(home: &std::path::Path, dir: &std::path::Path) -> String {
    const MARKER: &str = "# added by monk doctor --fix";
    let zshrc = home.join(".zshrc");
    let dir_s = dir.display().to_string();
    let existing = fs_err::read_to_string(&zshrc).unwrap_or_default();
    if existing.contains(MARKER) {
        return "(zsh fpath block already installed by monk)".to_string();
    }
    if existing.lines().any(|l| zshrc_line_uses_fpath_dir(l, &dir_s)) {
        return format!("(zsh fpath already references {dir_s})");
    }
    let block = format!("\n{MARKER}\nfpath=({dir_s} $fpath)\nautoload -U compinit && compinit\n");
    match fs_err::OpenOptions::new().create(true).append(true).open(&zshrc) {
        Ok(mut f) => {
            use std::io::Write;
            if let Err(e) = f.write_all(block.as_bytes()) {
                return format!(
                    "could not append to ~/.zshrc ({e}); add manually:\n  fpath=({dir_s} $fpath)\n  autoload -U compinit && compinit"
                );
            }
            format!("appended fpath block to {} — restart your shell", zshrc.display())
        }
        Err(e) => format!(
            "could not open ~/.zshrc ({e}); add manually:\n  fpath=({dir_s} $fpath)\n  autoload -U compinit && compinit"
        ),
    }
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
    cfg.save().map_err(|e| e.to_string())?;
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
    fn fpath_line_detection_rejects_comments_and_unrelated_lines() {
        let d = "/home/u/.local/share/zsh/site-functions";
        assert!(!zshrc_line_uses_fpath_dir(&format!("# skip {d}"), d));
        assert!(!zshrc_line_uses_fpath_dir(&format!("export FOO={d}"), d));
        assert!(!zshrc_line_uses_fpath_dir("fpath=(/other/dir $fpath)", d));
        assert!(!zshrc_line_uses_fpath_dir("alias fpath='echo'", d));
    }
}
