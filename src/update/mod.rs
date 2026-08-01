//! Self-update against GitHub releases.
//!
//! Network access goes through the system `curl` binary (present on macOS,
//! Windows 10+, and virtually every Linux install) so the crate tree stays
//! free of a TLS stack. Release assets follow the naming produced by
//! `.github/workflows/release.yml`: `monk-v{version}-{target}.tar.gz` (unix)
//! or `.zip` (windows), plus a combined `SHA256SUMS.txt`.

use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{paths, Error, Result};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const REPO: &str = "mdportnov/monk-cli";
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;
const CURL_TIMEOUT_SECS: u32 = 15;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    /// Latest published version, without the leading `v`.
    pub latest: String,
    /// Release tag as published (e.g. `v0.2.0`) — used to build asset URLs.
    pub tag: String,
    /// True when `latest` is strictly newer than the running binary.
    pub newer: bool,
}

// ---------------------------------------------------------------------------
// Version comparison
// ---------------------------------------------------------------------------

/// Parse `1.2.3`, `v1.2.3` or `1.2.3-rc.1` into comparable parts. Returns
/// None for anything that doesn't look like semver.
fn parse_version(s: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let s = s.trim().trim_start_matches('v');
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p.to_string())),
        None => (s, None),
    };
    let mut it = core.split('.');
    let maj = it.next()?.parse().ok()?;
    let min = it.next()?.parse().ok()?;
    let pat = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((maj, min, pat, pre))
}

/// True when `candidate` is strictly newer than `current`. Unknown formats
/// compare as "not newer" — a malformed remote tag must never prompt an
/// update. A pre-release is older than its release (`1.2.0-rc.1 < 1.2.0`).
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some(c), Some(cur)) = (parse_version(candidate), parse_version(current)) else {
        return false;
    };
    let core = (c.0, c.1, c.2).cmp(&(cur.0, cur.1, cur.2));
    match core {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (&c.3, &cur.3) {
            // Same core: release beats pre-release; two pre-releases compare
            // lexically (good enough for rc.1 < rc.2).
            (None, Some(_)) => true,
            (Some(a), Some(b)) => a > b,
            _ => false,
        },
    }
}

// ---------------------------------------------------------------------------
// curl plumbing
// ---------------------------------------------------------------------------

fn curl(args: &[&str]) -> Result<Vec<u8>> {
    // `--connect-timeout` (not `--max-time`): the latter caps the WHOLE
    // transfer and would abort a multi-MB asset download on a slow link.
    // The speed floor aborts transfers that stall mid-flight instead.
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--connect-timeout",
            &CURL_TIMEOUT_SECS.to_string(),
            "--speed-time",
            "30",
            "--speed-limit",
            "1024",
            "-A",
            concat!("monk/", env!("CARGO_PKG_VERSION")),
        ])
        .args(args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Other("`curl` not found — install curl to use update checks".into())
            } else {
                Error::Io(e)
            }
        })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Other(format!(
            "download failed ({}): {}",
            out.status,
            err.trim().lines().last().unwrap_or("no error output")
        )));
    }
    Ok(out.stdout)
}

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    tag_name: String,
}

/// Ask the GitHub API for the latest published release tag.
pub fn fetch_latest() -> Result<UpdateStatus> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = curl(&["-H", "Accept: application/vnd.github+json", &url])?;
    let rel: ReleaseJson = serde_json::from_slice(&body)
        .map_err(|e| Error::Other(format!("unexpected GitHub API response: {e}")))?;
    let latest = rel.tag_name.trim_start_matches('v').to_string();
    if parse_version(&latest).is_none() {
        return Err(Error::Other(format!("unexpected release tag `{}`", rel.tag_name)));
    }
    Ok(UpdateStatus { newer: is_newer(&latest, CURRENT_VERSION), latest, tag: rel.tag_name })
}

// ---------------------------------------------------------------------------
// Cached check
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
struct CheckCache {
    checked_at: u64,
    latest: String,
    tag: String,
}

fn cache_path() -> Result<PathBuf> {
    Ok(paths::data_dir()?.join("update_check.json"))
}

fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn load_cache() -> Option<CheckCache> {
    let raw = fs_err::read_to_string(cache_path().ok()?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn store_cache(status: &UpdateStatus) {
    let Ok(path) = cache_path() else { return };
    let cache = CheckCache {
        checked_at: now_epoch(),
        latest: status.latest.clone(),
        tag: status.tag.clone(),
    };
    if let Ok(raw) = serde_json::to_string(&cache) {
        let _ = fs_err::write(path, raw);
    }
}

/// Check for a newer release. With `force = false` a result cached within
/// the last 24h is reused, so callers can invoke this on every startup
/// without hammering the GitHub API (60 req/h unauthenticated limit).
pub fn check(force: bool) -> Result<UpdateStatus> {
    if !force {
        if let Some(c) = load_cache() {
            if now_epoch().saturating_sub(c.checked_at) < CACHE_TTL_SECS {
                return Ok(UpdateStatus {
                    newer: is_newer(&c.latest, CURRENT_VERSION),
                    latest: c.latest,
                    tag: c.tag,
                });
            }
        }
    }
    let status = fetch_latest()?;
    store_cache(&status);
    Ok(status)
}

// ---------------------------------------------------------------------------
// Self-update
// ---------------------------------------------------------------------------

/// Build target triple of the running binary — resolved at compile time so
/// the downloaded asset always matches how this binary was built.
fn target_triple() -> Result<&'static str> {
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    return Ok("x86_64-apple-darwin");
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    return Ok("aarch64-apple-darwin");
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
    return Ok("x86_64-unknown-linux-gnu");
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "musl"))]
    return Ok("x86_64-unknown-linux-musl");
    #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
    return Ok("aarch64-unknown-linux-gnu");
    #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "musl"))]
    return Ok("aarch64-unknown-linux-musl");
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    return Ok("x86_64-pc-windows-msvc");
    #[cfg(all(target_arch = "aarch64", target_os = "windows"))]
    return Ok("aarch64-pc-windows-msvc");
    #[allow(unreachable_code)]
    Err(Error::Other("self-update is not supported on this platform".into()))
}

fn asset_name(version: &str, target: &str) -> String {
    let ext = if target.contains("windows") { "zip" } else { "tar.gz" };
    format!("monk-v{version}-{target}.{ext}")
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// Find the expected hash for `asset` inside a `sha256sum`-format manifest.
fn expected_hash<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == asset).then_some(hash)
    })
}

pub struct UpdateOutcome {
    pub version: String,
    pub exe: PathBuf,
}

/// Download the latest release, verify its checksum, and atomically replace
/// the running binary. Returns an error (leaving the current install
/// untouched) at every step before the final rename.
pub fn perform_update() -> Result<UpdateOutcome> {
    let status = check(true)?;
    if !status.newer {
        return Err(Error::Other(format!(
            "already up to date (v{CURRENT_VERSION}, latest v{})",
            status.latest
        )));
    }

    let target = target_triple()?;
    let asset = asset_name(&status.latest, target);
    let base = format!("https://github.com/{REPO}/releases/download/{}", status.tag);

    let work = std::env::temp_dir().join(format!("monk-update-{}", std::process::id()));
    fs_err::create_dir_all(&work)?;
    let archive_path = work.join(&asset);

    let sums = String::from_utf8_lossy(&curl(&[&format!("{base}/SHA256SUMS.txt")])?).into_owned();
    let expected = expected_hash(&sums, &asset).ok_or_else(|| {
        Error::Other(format!("release {} has no asset `{asset}` for this platform", status.tag))
    })?;

    curl(&["-o", &archive_path.to_string_lossy(), &format!("{base}/{asset}")])?;
    let bytes = fs_err::read(&archive_path)?;
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = fs_err::remove_dir_all(&work);
        return Err(Error::Other(format!(
            "checksum mismatch for {asset}: expected {expected}, got {actual} — aborting"
        )));
    }

    // Both the unix tar.gz and the windows zip extract with bsdtar/GNU tar,
    // which ships on every supported OS (Windows 10+ includes bsdtar).
    let tar_ok = Command::new("tar")
        .args(["-xf", &archive_path.to_string_lossy(), "-C", &work.to_string_lossy()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !tar_ok {
        return Err(Error::Other("failed to extract release archive (is `tar` installed?)".into()));
    }

    let bin_name = if cfg!(windows) { "monk.exe" } else { "monk" };
    let inner = work.join(format!("monk-v{}-{target}", status.latest)).join(bin_name);
    if !inner.exists() {
        return Err(Error::Other(format!(
            "archive layout unexpected: {} missing",
            inner.display()
        )));
    }

    // Resolve symlinks so a `~/bin/monk -> /usr/local/bin/monk` style install
    // replaces the real binary instead of overwriting the symlink. On
    // windows canonicalize yields a `\\?\` path, which std fs ops accept.
    let current = std::env::current_exe().map_err(Error::Io)?;
    let current = fs_err::canonicalize(&current).unwrap_or(current);
    let dir =
        current.parent().ok_or_else(|| Error::Other("cannot resolve install directory".into()))?;

    // Stage next to the target so the final rename is atomic (same fs), then
    // move the running binary aside — renaming a running executable is legal
    // on unix and windows alike, deleting it is not (windows).
    let staged = dir.join(format!(".{bin_name}.new"));
    let old = dir.join(format!(".{bin_name}.old"));
    let elevate_hint = if cfg!(windows) {
        "rerun `monk update` from an elevated (Administrator) terminal"
    } else {
        "rerun with elevated privileges: `sudo monk update`"
    };
    fs_err::copy(&inner, &staged).map_err(|e| {
        Error::Other(format!("cannot write to {} ({e}) — {elevate_hint}", dir.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs_err::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    }
    let _ = fs_err::remove_file(&old);
    // Windows: a previous update's `.old` may still be locked by a daemon
    // that hasn't restarted — park the current binary under a unique name
    // then, instead of failing the rename into the occupied one.
    let old = if old.exists() {
        dir.join(format!(".{bin_name}.old.{}", std::process::id()))
    } else {
        old
    };
    fs_err::rename(&current, &old)?;
    if let Err(e) = fs_err::rename(&staged, &current) {
        // Roll back so the user is never left without a binary.
        let _ = fs_err::rename(&old, &current);
        return Err(Error::Io(e));
    }
    let _ = fs_err::remove_file(&old); // fails on windows while running — harmless leftover
    let _ = fs_err::remove_dir_all(&work);

    Ok(UpdateOutcome { version: status.latest, exe: current })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3, None)));
        assert_eq!(parse_version("v0.10.0"), Some((0, 10, 0, None)));
        assert_eq!(parse_version("1.2.3-rc.1"), Some((1, 2, 3, Some("rc.1".into()))));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("abc"), None);
    }

    #[test]
    fn newer_comparison() {
        assert!(is_newer("0.0.2", "0.0.1"));
        assert!(is_newer("0.1.0", "0.0.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.0.1", "0.0.1"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        // Release beats its own pre-release; malformed never wins.
        assert!(is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(is_newer("1.0.0-rc.2", "1.0.0-rc.1"));
        assert!(!is_newer("garbage", "0.0.1"));
        assert!(!is_newer("0.0.2", "garbage"));
    }

    #[test]
    fn asset_names_match_release_workflow() {
        assert_eq!(
            asset_name("0.2.0", "aarch64-apple-darwin"),
            "monk-v0.2.0-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            asset_name("0.2.0", "x86_64-pc-windows-msvc"),
            "monk-v0.2.0-x86_64-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn sha_manifest_lookup() {
        let sums = "abc123  monk-v0.2.0-aarch64-apple-darwin.tar.gz\n\
                    def456 *monk-v0.2.0-x86_64-pc-windows-msvc.zip\n";
        assert_eq!(expected_hash(sums, "monk-v0.2.0-aarch64-apple-darwin.tar.gz"), Some("abc123"));
        assert_eq!(expected_hash(sums, "monk-v0.2.0-x86_64-pc-windows-msvc.zip"), Some("def456"));
        assert_eq!(expected_hash(sums, "missing.tar.gz"), None);
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn target_triple_resolves_on_ci_platforms() {
        // All CI platforms are in the supported matrix; this guards against
        // a cfg typo silently breaking self-update for one of them.
        assert!(target_triple().is_ok());
    }
}
