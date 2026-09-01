//! The `monk.app` wrapper the menu bar process runs inside.
//!
//! A bare executable has no bundle identity, and macOS hangs several things
//! off that identity: the icon and buttons on a notification, the entry in
//! System Settings → Notifications, and the login item's name. So `monk
//! menubar install` materializes a minimal application bundle around the
//! same binary and the LaunchAgent points at the copy inside it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::Result;

pub const BUNDLE_ID: &str = "dev.monk.menubar";
const INFO_PLIST: &str = include_str!("../../assets/macos/Info.plist");
const ICNS: &[u8] = include_bytes!("../../assets/macos/monk.icns");

pub fn app_path() -> Result<PathBuf> {
    Ok(crate::paths::user_home()?.join("Applications/monk.app"))
}

/// Path of the binary the bundle runs. This is what launchd must invoke:
/// starting the outer binary directly would leave the process bundle-less.
pub fn executable_path() -> Result<PathBuf> {
    Ok(app_path()?.join("Contents/MacOS/monk"))
}

/// True when *this* process is the one inside the bundle. Everything that
/// needs a bundle identity — notifications above all — must check first,
/// because those APIs abort rather than fail when the identity is missing.
pub fn is_bundled() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| fs_err::canonicalize(exe).ok())
        .zip(executable_path().ok().and_then(|p| fs_err::canonicalize(p).ok()))
        .is_some_and(|(exe, bundled)| exe == bundled)
}

/// Creates the bundle, or refreshes it in place when monk has been upgraded.
/// The binary is copied rather than symlinked: macOS resolves the executable
/// path before looking for the enclosing bundle, so a symlink out of the
/// bundle would hand the process the identity of nothing at all.
pub fn install() -> Result<PathBuf> {
    install_as(env!("CARGO_PKG_VERSION"))
}

/// Same, but stamping a version the caller knows better than this binary
/// does. `monk update` replaces the executable and then, still running the
/// *old* code, rebuilds the bundle around the new file: stamping
/// `CARGO_PKG_VERSION` there would label a new binary with the old version
/// and leave the bundle looking permanently up to date.
pub fn install_as(version: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let app = app_path()?;
    let macos = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    fs_err::create_dir_all(&macos)?;
    fs_err::create_dir_all(&resources)?;

    // Written only when they differ: an unchanged bundle must come out of a
    // refresh byte-identical, because rewriting a sealed file breaks the
    // signature and macOS then refuses to launch the app.
    let plist_changed = write_if_changed(
        &app.join("Contents/Info.plist"),
        INFO_PLIST.replace("__VERSION__", version).as_bytes(),
    )?;
    let icon_changed = write_if_changed(&resources.join("monk.icns"), ICNS)?;

    let target = macos.join("monk");
    // `monk menubar install` run *from inside the bundle* would otherwise
    // delete its own binary and then copy from the hole it just made.
    let same_file = fs_err::canonicalize(&exe)
        .ok()
        .zip(fs_err::canonicalize(&target).ok())
        .is_some_and(|(a, b)| a == b);
    if !same_file {
        // The old signature seals the old binary: leaving it next to a new
        // one makes macOS kill the app on launch. Drop it, copy, re-sign.
        let _ = fs_err::remove_dir_all(app.join("Contents/_CodeSignature"));
        // Copying onto a running binary fails with ETXTBSY; unlink first so
        // a refresh while the menu bar app is up still lands.
        if target.exists() {
            fs_err::remove_file(&target)?;
        }
        fs_err::copy(&exe, &target)?;
    }
    if !same_file || plist_changed || icon_changed {
        sign(&app);
    }

    // Let Launch Services notice the bundle so it shows up by name.
    register(&app);
    Ok(target)
}

/// Writes `contents` only when the file does not already hold exactly that,
/// and reports whether it wrote.
fn write_if_changed(path: &Path, contents: &[u8]) -> Result<bool> {
    if fs_err::read(path).is_ok_and(|current| current == contents) {
        return Ok(false);
    }
    fs_err::write(path, contents)?;
    Ok(true)
}

/// True when the installed bundle was built by a different version of monk
/// than the one running now — i.e. the CLI was upgraded underneath it.
pub fn is_stale() -> bool {
    let Ok(app) = app_path() else { return false };
    // A bundle missing its executable is as broken as an out-of-date one,
    // and so is an unreadable or truncated Info.plist — an interrupted
    // install, say. Without the plist macOS gives the app no identity, and
    // the only way out of either is to build the bundle again.
    if !app.join("Contents/MacOS/monk").exists() {
        return true;
    }
    let Ok(plist) = fs_err::read_to_string(app.join("Contents/Info.plist")) else {
        return true;
    };
    !plist.contains(&format!("<string>{}</string>", env!("CARGO_PKG_VERSION")))
}

/// Ad-hoc signature. An unsigned bundle still runs — the linker signs the
/// Mach-O itself — but its identity changes with every build, and macOS
/// then forgets the notification permission the user granted. Best effort:
/// a missing toolchain must not fail setup.
fn sign(app: &Path) {
    let signed = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(app)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match signed {
        Ok(status) if status.success() => {}
        Ok(_) | Err(_) => tracing::warn!("could not ad-hoc sign the app bundle"),
    }
}

pub fn uninstall() -> Result<()> {
    let app = app_path()?;
    if app.exists() {
        fs_err::remove_dir_all(&app)?;
    }
    Ok(())
}

fn register(app: &Path) {
    const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/\
                              LaunchServices.framework/Support/lsregister";
    let _ = Command::new(LSREGISTER)
        .arg("-f")
        .arg(app)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
