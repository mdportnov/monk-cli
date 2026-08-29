use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use crate::{
    blocker::{self, dns_server, BlockerBackend, HostsBlocker, ProbeResult},
    config::Config,
    daemon::PidFile,
    ipc::{self, Request, Response},
    paths,
};

use super::{Action, ActionKind, Check, Status};

pub(crate) fn check_version() -> Check {
    Check::new("version", "monk version", Status::Info, format!("v{}", env!("CARGO_PKG_VERSION")))
}

pub(crate) fn check_platform() -> Check {
    Check::new(
        "platform",
        "platform",
        Status::Info,
        format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
    )
}

pub(crate) fn check_service_install() -> Check {
    #[cfg(target_os = "macos")]
    {
        let plist = std::path::Path::new("/Library/LaunchDaemons/dev.monk.monkd.plist");
        if plist.exists() {
            let cli = env!("CARGO_PKG_VERSION");
            let reinstall =
                Action { key: 'r', label: "reinstall service", kind: ActionKind::ReinstallService };
            // The service runs its own copy of the binary, so `monk update`
            // (which only replaces the CLI) can leave an old daemon behind.
            // Compare on disk, not over IPC: this must also fire when the
            // daemon is down.
            match installed_daemon_version() {
                Some(v) if v == cli => Check::new(
                    "service.install",
                    "system service",
                    Status::Ok,
                    format!("LaunchDaemon installed (v{v}); daemon runs as root"),
                )
                .with_extras(vec![plist.display().to_string()]),
                Some(v) => Check::new(
                    "service.install",
                    "system service",
                    Status::Warn,
                    format!("installed daemon is v{v}, cli is v{cli}"),
                )
                .with_hint(
                    "the service still runs the old binary — refresh it with `sudo monk service install`",
                )
                .with_extras(vec![plist.display().to_string()])
                .with_actions(vec![reinstall]),
                None => Check::new(
                    "service.install",
                    "system service",
                    Status::Warn,
                    "LaunchDaemon installed, but its binary is missing or unreadable",
                )
                .with_hint("reinstall it with `sudo monk service install`")
                .with_extras(vec![plist.display().to_string()])
                .with_actions(vec![reinstall]),
            }
        } else {
            Check::new(
                "service.install",
                "system service",
                Status::Warn,
                "system LaunchDaemon not installed",
            )
            .with_hint("install with `sudo monk service install` (or `monk doctor --fix`) so blocking works")
            .with_actions(vec![Action {
                key: 'r',
                label: "install service",
                kind: ActionKind::ReinstallService,
            }])
        }
    }
    #[cfg(target_os = "linux")]
    {
        check_service_install_linux()
    }

    #[cfg(windows)]
    {
        check_service_install_windows()
    }
}

/// systemd *user* unit: installed at all, pointing at a binary that still
/// exists, and actually running?
///
/// `monk service install` reports "manual start: …" and still returns success
/// when `systemctl --user enable --now` fails (no systemd, no user session,
/// a container) — without this check nothing else ever tells the user their
/// daemon is not running.
#[cfg(target_os = "linux")]
fn check_service_install_linux() -> Check {
    let reinstall =
        Action { key: 'r', label: "reinstall service", kind: ActionKind::ReinstallService };
    let user_unit = directories::BaseDirs::new()
        .map(|d| d.config_dir().join("systemd/user/monk.service"))
        .filter(|p| p.exists());
    let packaged_unit = ["/usr/lib/systemd/user/monk.service", "/lib/systemd/user/monk.service"]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.exists());

    let Some(unit) = user_unit.or(packaged_unit) else {
        return Check::new(
            "service.install",
            "system service",
            Status::Warn,
            "systemd user unit not installed",
        )
        .with_hint("install it with `monk service install` (no sudo needed on Linux)")
        .with_actions(vec![reinstall]);
    };

    let mut extras = vec![unit.display().to_string()];

    // A unit left over from an older install can point at a binary that has
    // since moved (`cargo install` → package install, a deleted checkout).
    if let Some(exec) = fs_err::read_to_string(&unit).ok().and_then(|raw| unit_exec_binary(&raw)) {
        extras.push(format!("ExecStart: {exec}"));
        if !std::path::Path::new(&exec).exists() {
            return Check::new(
                "service.install",
                "system service",
                Status::Fail,
                format!("unit points at a binary that no longer exists: {exec}"),
            )
            .with_hint("re-point it with `monk service install`")
            .with_extras(extras)
            .with_actions(vec![reinstall]);
        }
    }

    match systemctl_user(&["is-active", "monk"]) {
        Some(state) if state == "active" => {
            Check::new("service.install", "system service", Status::Ok, "systemd user unit active")
                .with_extras(extras)
        }
        Some(state) => Check::new(
            "service.install",
            "system service",
            Status::Warn,
            format!("systemd user unit is {state}"),
        )
        .with_hint("start it with `systemctl --user enable --now monk`")
        .with_extras(extras)
        .with_actions(vec![reinstall]),
        None => Check::new(
            "service.install",
            "system service",
            Status::Info,
            "unit installed; systemctl unavailable, cannot query state",
        )
        .with_extras(extras),
    }
}

/// First `ExecStart=` binary in a unit file, unquoted. Systemd allows
/// `ExecStart=-/path`, `@/path` and friends — strip those prefixes.
///
/// Compiled on every platform so its tests run everywhere, not only on the
/// one OS that uses it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn unit_exec_binary(unit: &str) -> Option<String> {
    let line = unit.lines().find(|l| l.trim_start().starts_with("ExecStart="))?;
    let value = line.split_once('=')?.1.trim();
    let value = value.trim_start_matches(['-', '@', '+', '!', ':']);
    // A quoted path may contain spaces, so honour the quotes before falling
    // back to whitespace splitting.
    if let Some(rest) = value.strip_prefix('"') {
        return rest.split_once('"').map(|(path, _)| path.to_string());
    }
    Some(value.split_whitespace().next()?.to_string())
}

#[cfg(target_os = "linux")]
fn systemctl_user(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("systemctl").arg("--user").args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(text)
}

/// The logon scheduled task: present, and pointing at a binary that still
/// exists? `schtasks /Create` needs an elevated terminal, so a plain failure
/// here is the usual "setup looked fine but nothing runs" report.
#[cfg(windows)]
fn check_service_install_windows() -> Check {
    let reinstall =
        Action { key: 'r', label: "reinstall service", kind: ActionKind::ReinstallService };
    let out = std::process::Command::new("schtasks")
        .args(["/Query", "/TN", "monkd", "/V", "/FO", "LIST"])
        .output();
    let Ok(out) = out else {
        return Check::new(
            "service.install",
            "system service",
            Status::Warn,
            "schtasks not available — cannot check the logon task",
        );
    };
    if !out.status.success() {
        return Check::new(
            "service.install",
            "system service",
            Status::Warn,
            "logon scheduled task `monkd` not installed",
        )
        .with_hint(
            "install it from an Administrator terminal: `monk service install` (the task needs elevation)",
        )
        .with_actions(vec![reinstall]);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    match task_binary(&text) {
        Some(bin) if !std::path::Path::new(&bin).exists() => Check::new(
            "service.install",
            "system service",
            Status::Fail,
            format!("task `monkd` points at a binary that no longer exists: {bin}"),
        )
        .with_hint("re-point it from an Administrator terminal: `monk service install`")
        .with_actions(vec![reinstall]),
        Some(bin) => Check::new(
            "service.install",
            "system service",
            Status::Ok,
            "logon task `monkd` present",
        )
        .with_extras(vec![bin]),
        None => Check::new(
            "service.install",
            "system service",
            Status::Ok,
            "logon task `monkd` present",
        ),
    }
}

/// Pull the quoted `...monk.exe` path out of `schtasks /Query /V /FO LIST`
/// output. Header labels are localized, the path is not — so match on the
/// value, never on the label.
///
/// Compiled on every platform so its tests run everywhere, not only on the
/// one OS that uses it.
#[cfg_attr(not(windows), allow(dead_code))]
fn task_binary(query_output: &str) -> Option<String> {
    for line in query_output.lines() {
        let mut parts = line.split('"');
        // ["… label: ", "C:\path\monk.exe", " daemon run"]
        while let Some(candidate) = parts.nth(1) {
            if candidate.to_ascii_lowercase().ends_with("monk.exe") {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// Version of the binary the system service actually runs, read by executing
/// it with `--version` (clap prints `monk <semver>`). `None` when the file is
/// missing or does not answer.
#[cfg(target_os = "macos")]
fn installed_daemon_version() -> Option<String> {
    let bin = std::path::Path::new("/usr/local/libexec/monk/monkd");
    if !bin.exists() {
        return None;
    }
    let out = std::process::Command::new(bin).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.split_whitespace().last().map(|v| v.trim_start_matches('v').to_string())
}

pub(crate) fn check_privileges() -> Check {
    #[cfg(unix)]
    {
        let is_root = nix::unistd::geteuid().is_root();
        if is_root {
            Check::new("privileges", "privileges", Status::Info, "running as root")
        } else {
            Check::new("privileges", "privileges", Status::Info, "running as user (cli context)")
        }
    }
    #[cfg(windows)]
    {
        Check::new(
            "privileges",
            "privileges",
            Status::Info,
            "windows — elevation checked per-backend",
        )
    }
}

pub(crate) fn check_paths() -> Vec<Check> {
    fn describe(
        id: &'static str,
        title: &'static str,
        res: crate::Result<std::path::PathBuf>,
        action: Option<Action>,
    ) -> Check {
        let check = match res {
            Ok(p) => {
                let exists = p.exists();
                let status = if exists { Status::Ok } else { Status::Info };
                let detail = if exists {
                    p.display().to_string()
                } else {
                    format!("{} (not created yet)", p.display())
                };
                Check::new(id, title, status, detail)
            }
            Err(e) => Check::new(id, title, Status::Fail, format!("resolve failed: {e}")),
        };
        if let Some(a) = action {
            check.with_actions(vec![a])
        } else {
            check
        }
    }
    vec![
        describe(
            "path.config",
            "config file",
            paths::config_file(),
            Some(Action { key: 'o', label: "open config", kind: ActionKind::OpenConfig }),
        ),
        describe(
            "path.data",
            "data dir",
            paths::data_dir(),
            Some(Action { key: 'o', label: "open data dir", kind: ActionKind::OpenDataDir }),
        ),
        describe(
            "path.log",
            "log file",
            paths::log_file(),
            Some(Action { key: 'o', label: "open log", kind: ActionKind::OpenLog }),
        ),
        describe("path.socket", "ipc socket", paths::ipc_socket(), None),
    ]
}

pub(crate) fn check_config() -> Check {
    match Config::load() {
        Ok(cfg) => {
            let extras = vec![
                format!("profiles: {}", cfg.profiles.len()),
                format!("default profile: {}", cfg.general.default_profile),
                format!(
                    "default duration: {}",
                    humantime::format_duration(cfg.general.default_duration)
                ),
                format!(
                    "locale: {}",
                    cfg.general.locale.clone().unwrap_or_else(|| "system".into())
                ),
            ];
            Check::new("config", "config", Status::Ok, "loaded").with_extras(extras)
        }
        Err(e) => Check::new("config", "config", Status::Fail, format!("{e}"))
            .with_hint("press [R] to back it up and start fresh, or [o] to edit it by hand")
            .with_actions(vec![
                Action { key: 'R', label: "backup & reset config", kind: ActionKind::ResetConfig },
                Action { key: 'o', label: "open config", kind: ActionKind::OpenConfig },
            ]),
    }
}

pub(crate) fn check_profile_apps() -> Check {
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(_) => {
            return Check::new(
                "profiles.apps",
                "profile app refs",
                Status::Skipped,
                "config not loadable — see `config` check",
            )
        }
    };
    let cache = match crate::apps::AppCache::load() {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Check::new(
                "profiles.apps",
                "profile app refs",
                Status::Skipped,
                "no app cache yet — run `monk apps refresh`",
            )
        }
        Err(e) => {
            return Check::new(
                "profiles.apps",
                "profile app refs",
                Status::Skipped,
                format!("app cache load failed: {e}"),
            )
        }
    };
    // Only direct `profile.apps` entries count: brand presets bundle every
    // known app of a brand, so brand-derived ids pointing at apps the user
    // never installed are expected and not actionable.
    let mut extras = Vec::new();
    let mut stale_total = 0usize;
    for (name, profile) in &cfg.profiles {
        let resolution = crate::apps::resolve(&profile.apps, &cache);
        if !resolution.stale.is_empty() {
            stale_total += resolution.stale.len();
            extras.push(format!("{name}: {}", resolution.stale.join(", ")));
        }
    }
    if stale_total == 0 {
        Check::new(
            "profiles.apps",
            "profile app refs",
            Status::Ok,
            "all referenced apps are installed",
        )
    } else {
        Check::new(
            "profiles.apps",
            "profile app refs",
            Status::Warn,
            format!("{stale_total} reference(s) to uninstalled apps"),
        )
        .with_hint(
            "harmless — the daemon skips them; press [f] (or run `monk doctor --fix`) to clean up automatically",
        )
        .with_extras(extras)
        .with_actions(vec![
            Action { key: 'f', label: "remove stale refs", kind: ActionKind::PruneStaleAppRefs },
            Action { key: 'o', label: "open config", kind: ActionKind::OpenConfig },
        ])
    }
}

pub(crate) fn check_pidfile() -> Check {
    let start = Action { key: 's', label: "start daemon", kind: ActionKind::StartDaemon };
    let stop = Action { key: 'x', label: "stop daemon", kind: ActionKind::StopDaemon };
    let in_system_mode = paths::system_mode();
    match PidFile::new() {
        Ok(p) => match p.is_alive() {
            Ok(Some(pid)) => {
                let mut check =
                    Check::new("daemon", "daemon", Status::Ok, format!("running (pid {pid})"));
                if !in_system_mode {
                    check = check.with_actions(vec![stop]);
                }
                check
            }
            Ok(None) => {
                let mut c = Check::new("daemon", "daemon", Status::Warn, "not running");
                if in_system_mode {
                    c = c.with_hint("restart: `sudo launchctl kickstart -k system/dev.monk.monkd`");
                } else {
                    c = c
                        .with_hint("start it with `monk daemon` (or `sudo monk service install`)")
                        .with_actions(vec![start]);
                }
                c
            }
            Err(e) => Check::new("daemon", "daemon", Status::Fail, format!("pidfile read: {e}"))
                .with_actions(if in_system_mode { vec![] } else { vec![start] }),
        },
        Err(e) => Check::new("daemon", "daemon", Status::Fail, format!("pidfile init: {e}")),
    }
}

pub(crate) async fn check_ipc() -> Check {
    let start = Action { key: 's', label: "start daemon", kind: ActionKind::StartDaemon };
    match ipc::send(&Request::Ping).await {
        Ok(Response::Pong { version }) => {
            let cli = env!("CARGO_PKG_VERSION");
            if version == cli {
                Check::new("ipc", "ipc", Status::Ok, format!("daemon v{version} responds"))
            } else {
                Check::new(
                    "ipc",
                    "ipc",
                    Status::Warn,
                    format!("daemon v{version} responds, but cli is v{cli}"),
                )
                .with_hint(
                    "the installed daemon binary is outdated — update it with `sudo monk service install`",
                )
                .with_actions(vec![Action {
                    key: 'r',
                    label: "reinstall service",
                    kind: ActionKind::ReinstallService,
                }])
            }
        }
        Ok(other) => Check::new("ipc", "ipc", Status::Warn, format!("unexpected: {other:?}")),
        Err(e) => Check::new("ipc", "ipc", Status::Fail, format!("send failed: {e}"))
            .with_hint("daemon may be down or socket path mismatched")
            .with_actions(vec![start]),
    }
}

pub(crate) async fn check_hard_mode() -> Check {
    match ipc::send(&Request::Status).await {
        Ok(Response::Status { hard_mode: Some(h), .. }) => {
            let mut c = Check::new(
                "hard_mode",
                "hard mode",
                Status::Info,
                format!("active, {} remaining", humantime::format_duration(h.remaining)),
            );
            if let Some(at) = h.panic_releases_at {
                c.extras.push(format!("panic releases at {}", at.to_rfc3339()));
            }
            c
        }
        Ok(Response::Status { hard_mode: None, .. }) => {
            Check::new("hard_mode", "hard mode", Status::Info, "off")
        }
        Ok(_) => Check::new("hard_mode", "hard mode", Status::Skipped, "n/a"),
        Err(_) => Check::new("hard_mode", "hard mode", Status::Skipped, "daemon unreachable"),
    }
}

pub(crate) fn check_blocker_backend() -> Check {
    let mut lines = Vec::new();
    let hosts = blocker::hosts_path();
    let writable = fs_err::OpenOptions::new().append(true).open(&hosts).is_ok();
    lines.push(format!(
        "hosts: {} ({})",
        hosts.display(),
        if writable { "writable" } else { "read-only" }
    ));

    let hosts_available = matches!(HostsBlocker::probe(), ProbeResult::Available { .. });
    match HostsBlocker::probe() {
        ProbeResult::Available { priority, detail } => {
            lines.push(format!("hosts backend: available (priority {priority}, {detail})"));
        }
        ProbeResult::Unavailable { reason } => {
            lines.push(format!("hosts backend: unavailable — {reason}"));
        }
    }

    // Mutated only in the macOS/Linux probe blocks below; on other targets
    // (Windows) it stays false, so the `mut` would otherwise be flagged.
    #[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(unused_mut))]
    let mut alt_available = false;

    #[cfg(target_os = "macos")]
    {
        match crate::blocker::backends::resolver_dir::ResolverDirBlocker::probe() {
            ProbeResult::Available { priority, detail } => {
                alt_available = true;
                lines.push(format!("resolver_dir: available (priority {priority}, {detail})"));
            }
            ProbeResult::Unavailable { reason } => {
                lines.push(format!("resolver_dir: unavailable — {reason}"));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        match crate::blocker::backends::systemd_resolved::SystemdResolvedBlocker::probe() {
            ProbeResult::Available { priority, detail } => {
                alt_available = true;
                lines.push(format!("systemd_resolved: available (priority {priority}, {detail})"));
            }
            ProbeResult::Unavailable { reason } => {
                lines.push(format!("systemd_resolved: unavailable — {reason}"));
            }
        }
    }

    // In system_mode the daemon runs as root and owns /etc/hosts even when
    // THIS process (cli) can't write it. Don't fail the local-process probe.
    // service.install + ipc checks already tell us if blocking is wired up.
    let in_system_mode = paths::system_mode();
    let any_available = hosts_available || alt_available;
    let (status, detail) = if in_system_mode {
        (Status::Ok, "system daemon owns /etc/hosts (cli-local probe is informational)")
    } else if any_available {
        (Status::Ok, "site blocking available")
    } else {
        (Status::Fail, "no site blocker available — sessions will refuse to start")
    };
    let mut check =
        Check::new("blocker.backends", "blocker backends", status, detail).with_extras(lines);
    if !in_system_mode && !any_available {
        check = check.with_hint(
            "install the LaunchDaemon so monkd runs as root: `sudo monk service install`",
        );
    }
    check
}

pub(crate) async fn check_dns_server() -> Check {
    use tokio::{net::UdpSocket, time::timeout};

    let addr: SocketAddr = (Ipv4Addr::LOCALHOST, dns_server::PORT).into();
    let socket = match UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await {
        Ok(s) => s,
        Err(e) => {
            return Check::new(
                "dns_server",
                "dns server",
                Status::Fail,
                format!("probe socket bind failed: {e}"),
            );
        }
    };

    let mut query = Vec::with_capacity(32);
    query.extend_from_slice(&0x4d4eu16.to_be_bytes());
    query.extend_from_slice(&0x0100u16.to_be_bytes());
    query.extend_from_slice(&1u16.to_be_bytes());
    query.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    for label in ["monk-doctor", "test"] {
        query.push(label.len() as u8);
        query.extend_from_slice(label.as_bytes());
    }
    query.push(0);
    query.extend_from_slice(&1u16.to_be_bytes());
    query.extend_from_slice(&1u16.to_be_bytes());

    let start = Action { key: 's', label: "start daemon", kind: ActionKind::StartDaemon };
    if let Err(e) = socket.send_to(&query, addr).await {
        return Check::new(
            "dns_server",
            "dns server",
            Status::Fail,
            format!("send to 127.0.0.1:{} failed: {e}", dns_server::PORT),
        )
        .with_hint("daemon not running? the dns server starts with `monk daemon`")
        .with_actions(vec![start]);
    }

    let mut buf = [0u8; 512];
    match timeout(Duration::from_millis(500), socket.recv_from(&mut buf)).await {
        Ok(Ok((len, _))) => match parse_dns_a_answer(&buf[..len]) {
            Ok(ip) => Check::new(
                "dns_server",
                "dns server",
                Status::Ok,
                format!(
                    "127.0.0.1:{} answered A → {}.{}.{}.{}",
                    dns_server::PORT,
                    ip[0],
                    ip[1],
                    ip[2],
                    ip[3]
                ),
            ),
            Err(e) => Check::new("dns_server", "dns server", Status::Warn, e.to_string()),
        },
        Ok(Err(e)) => {
            Check::new("dns_server", "dns server", Status::Fail, format!("recv failed: {e}"))
        }
        Err(_) => Check::new("dns_server", "dns server", Status::Fail, "timeout waiting for reply")
            .with_hint("daemon may be down or another process owns the port")
            .with_actions(vec![start]),
    }
}

fn parse_dns_a_answer(buf: &[u8]) -> std::result::Result<[u8; 4], &'static str> {
    if buf.len() < 12 {
        return Err("truncated header");
    }
    let ancount = u16::from_be_bytes([buf[6], buf[7]]);
    if ancount == 0 {
        return Err("no answer section");
    }
    let mut i = 12usize;
    // skip question: name + qtype(2) + qclass(2)
    i = skip_name(buf, i)?;
    if i + 4 > buf.len() {
        return Err("truncated question");
    }
    i += 4;
    // walk answer RRs until we find an A record
    for _ in 0..ancount {
        i = skip_name(buf, i)?;
        if i + 10 > buf.len() {
            return Err("truncated rr header");
        }
        let rtype = u16::from_be_bytes([buf[i], buf[i + 1]]);
        let rdlength = u16::from_be_bytes([buf[i + 8], buf[i + 9]]) as usize;
        i += 10;
        if i + rdlength > buf.len() {
            return Err("truncated rdata");
        }
        if rtype == 1 && rdlength == 4 {
            return Ok([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
        }
        i += rdlength;
    }
    Err("no A record in answer")
}

fn skip_name(buf: &[u8], mut i: usize) -> std::result::Result<usize, &'static str> {
    loop {
        if i >= buf.len() {
            return Err("truncated name");
        }
        let b = buf[i];
        if b == 0 {
            return Ok(i + 1);
        }
        if b & 0xC0 == 0xC0 {
            if i + 2 > buf.len() {
                return Err("truncated pointer");
            }
            return Ok(i + 2);
        }
        if b & 0xC0 != 0 {
            return Err("invalid label");
        }
        i += 1 + b as usize;
    }
}

pub(crate) async fn check_block_page() -> Check {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpStream,
        time::timeout,
    };

    let start = Action { key: 's', label: "start daemon", kind: ActionKind::StartDaemon };
    let fut = async {
        let mut stream = TcpStream::connect(("127.0.0.1", 80)).await?;
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: monk-doctor\r\nConnection: close\r\n\r\n")
            .await?;
        let mut buf = [0u8; 256];
        let n = stream.read(&mut buf).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&buf[..n]).to_string())
    };
    match timeout(Duration::from_millis(500), fut).await {
        Ok(Ok(head)) => {
            let first = head.lines().next().unwrap_or("").to_string();
            if first.starts_with("HTTP/1.1") {
                Check::new("block_page", "block page", Status::Ok, first)
            } else {
                Check::new("block_page", "block page", Status::Warn, "non-http response")
            }
        }
        Ok(Err(e)) => {
            use std::io::ErrorKind;
            let (detail, hint) = match e.kind() {
                ErrorKind::ConnectionRefused => (
                    "connection refused on 127.0.0.1:80",
                    "daemon not running or block page server failed to bind :80",
                ),
                ErrorKind::PermissionDenied => (
                    "permission denied",
                    "binding :80 requires root; run daemon with elevated privileges",
                ),
                ErrorKind::AddrInUse => ("address in use", "another process owns 127.0.0.1:80"),
                _ => ("not reachable", "daemon not running or port 80 unavailable"),
            };
            Check::new("block_page", "block page", Status::Warn, format!("{detail}: {e}"))
                .with_hint(hint)
                .with_actions(vec![start])
        }
        Err(_) => Check::new("block_page", "block page", Status::Warn, "timeout")
            .with_hint("port 80 may be owned by a non-responsive process")
            .with_actions(vec![start]),
    }
}

pub(crate) fn check_env_path() -> Check {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return Check::new("env.path", "PATH", Status::Skipped, format!("current_exe: {e}"));
        }
    };
    let Some(bin_dir) = exe.parent().map(|p| p.to_path_buf()) else {
        return Check::new("env.path", "PATH", Status::Skipped, "no parent dir");
    };
    let path = std::env::var("PATH").unwrap_or_default();
    let bin_dir_s = bin_dir.to_string_lossy();
    let on_path = std::env::split_paths(&path).any(|p| p == bin_dir);
    if on_path {
        Check::new("env.path", "PATH", Status::Ok, format!("monk dir on PATH ({bin_dir_s})"))
    } else {
        Check::new("env.path", "PATH", Status::Warn, format!("monk dir {bin_dir_s} not on PATH"))
            .with_hint("new shells won't find `monk` — fix with `monk doctor --fix`")
            .with_actions(vec![Action {
                key: 'p',
                label: "show PATH hint",
                kind: ActionKind::PrintPathHint,
            }])
    }
}

pub(crate) fn check_completions() -> Check {
    let shell = std::env::var("SHELL").ok().and_then(|s| {
        std::path::Path::new(&s).file_name().map(|n| n.to_string_lossy().to_lowercase())
    });
    let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) else {
        return Check::new("env.completions", "completions", Status::Skipped, "no home dir");
    };
    // First candidate is where `[c]`/`--fix` installs; the rest are common
    // spots (homebrew, distro packages, oh-my-zsh, hand installs) so we don't
    // warn when completions are already set up some other way.
    let (label, candidates): (&str, Vec<std::path::PathBuf>) = match shell.as_deref() {
        Some("zsh") => {
            let mut c = vec![
                home.join(".local/share/zsh/site-functions/_monk"),
                home.join(".zfunc/_monk"),
                home.join(".oh-my-zsh/completions/_monk"),
                std::path::PathBuf::from("/opt/homebrew/share/zsh/site-functions/_monk"),
                std::path::PathBuf::from("/usr/local/share/zsh/site-functions/_monk"),
                std::path::PathBuf::from("/usr/share/zsh/site-functions/_monk"),
            ];
            if let Ok(fpath) = std::env::var("FPATH") {
                c.extend(std::env::split_paths(&fpath).map(|d| d.join("_monk")));
            }
            ("zsh", c)
        }
        Some("bash") => (
            "bash",
            vec![
                home.join(".local/share/bash-completion/completions/monk"),
                std::path::PathBuf::from("/opt/homebrew/etc/bash_completion.d/monk"),
                std::path::PathBuf::from("/usr/local/etc/bash_completion.d/monk"),
                std::path::PathBuf::from("/etc/bash_completion.d/monk"),
                std::path::PathBuf::from("/usr/share/bash-completion/completions/monk"),
            ],
        ),
        Some("fish") => (
            "fish",
            vec![
                home.join(".config/fish/completions/monk.fish"),
                std::path::PathBuf::from("/opt/homebrew/share/fish/vendor_completions.d/monk.fish"),
                std::path::PathBuf::from("/usr/local/share/fish/vendor_completions.d/monk.fish"),
                std::path::PathBuf::from("/usr/share/fish/vendor_completions.d/monk.fish"),
            ],
        ),
        // Windows without a SHELL variable: PowerShell, whose completions are
        // a script dot-sourced from the profile.
        #[cfg(windows)]
        _ => {
            let Some(profile) = super::actions::powershell_profile() else {
                return Check::new(
                    "env.completions",
                    "completions",
                    Status::Info,
                    "powershell profile not found — run `monk completions powershell` manually",
                );
            };
            let dir = profile.parent().map(std::path::Path::to_path_buf).unwrap_or(home);
            ("powershell", vec![dir.join("monk.completion.ps1")])
        }
        #[cfg(not(windows))]
        other => {
            return Check::new(
                "env.completions",
                "completions",
                Status::Info,
                format!("unsupported shell: {}", other.unwrap_or("?")),
            );
        }
    };
    if let Some(found) = candidates.iter().find(|p| p.exists()) {
        Check::new(
            "env.completions",
            "completions",
            Status::Ok,
            format!("{label}: {}", found.display()),
        )
    } else {
        Check::new(
            "env.completions",
            "completions",
            Status::Warn,
            format!("{label} completions not installed (tab-completion for monk commands)"),
        )
        .with_hint("press [c] to install now — no manual steps needed")
        .with_extras(vec![format!("will install to {}", candidates[0].display())])
        .with_actions(vec![Action {
            key: 'c',
            label: "install completions",
            kind: ActionKind::InstallCompletions,
        }])
    }
}

pub(crate) fn check_log_tail() -> Check {
    let Ok(path) = paths::log_file() else {
        return Check::new("log", "recent log", Status::Skipped, "log path unavailable");
    };
    if !path.exists() {
        return Check::new("log", "recent log", Status::Info, "no log file yet");
    }
    let open = Action { key: 'o', label: "open log", kind: ActionKind::OpenLog };
    match fs_err::read_to_string(&path) {
        Ok(s) => {
            let lines: Vec<String> = s.lines().rev().take(8).map(|l| l.to_string()).collect();
            let lines: Vec<String> = lines.into_iter().rev().collect();
            Check::new("log", "recent log", Status::Info, path.display().to_string())
                .with_extras(lines)
                .with_actions(vec![open])
        }
        Err(e) => Check::new("log", "recent log", Status::Warn, format!("read failed: {e}"))
            .with_actions(vec![open]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_a_response(ip: [u8; 4]) -> Vec<u8> {
        let mut r = Vec::new();
        r.extend_from_slice(&0x1234u16.to_be_bytes());
        r.extend_from_slice(&0x8180u16.to_be_bytes());
        r.extend_from_slice(&1u16.to_be_bytes()); // qd
        r.extend_from_slice(&1u16.to_be_bytes()); // an
        r.extend_from_slice(&0u16.to_be_bytes());
        r.extend_from_slice(&0u16.to_be_bytes());
        // question: example.com A IN
        for label in ["example", "com"] {
            r.push(label.len() as u8);
            r.extend_from_slice(label.as_bytes());
        }
        r.push(0);
        r.extend_from_slice(&1u16.to_be_bytes());
        r.extend_from_slice(&1u16.to_be_bytes());
        // answer: ptr 0xC00C, A, IN, ttl=60, rdlen=4, ip
        r.extend_from_slice(&[0xC0, 0x0C]);
        r.extend_from_slice(&1u16.to_be_bytes());
        r.extend_from_slice(&1u16.to_be_bytes());
        r.extend_from_slice(&60u32.to_be_bytes());
        r.extend_from_slice(&4u16.to_be_bytes());
        r.extend_from_slice(&ip);
        r
    }

    #[test]
    fn parse_dns_a_answer_extracts_ip() {
        let buf = build_a_response([127, 0, 0, 1]);
        assert_eq!(parse_dns_a_answer(&buf).unwrap(), [127, 0, 0, 1]);
    }

    #[test]
    fn parse_dns_a_answer_rejects_empty() {
        assert!(parse_dns_a_answer(&[0u8; 11]).is_err());
    }

    #[test]
    fn unit_exec_binary_handles_systemd_prefixes() {
        let unit = "[Service]\nType=simple\nExecStart=/usr/bin/monk daemon run\n";
        assert_eq!(unit_exec_binary(unit).as_deref(), Some("/usr/bin/monk"));
        assert_eq!(
            unit_exec_binary("ExecStart=-/home/u/.cargo/bin/monk daemon run").as_deref(),
            Some("/home/u/.cargo/bin/monk")
        );
        assert_eq!(
            unit_exec_binary("ExecStart=\"/opt/my monk/monk\" daemon run").as_deref(),
            Some("/opt/my monk/monk")
        );
        assert_eq!(unit_exec_binary("[Unit]\nDescription=x\n"), None);
    }

    #[test]
    fn unit_exec_binary_flags_the_unrendered_template() {
        // The bug this guards: shipping the template verbatim in a package.
        assert_eq!(unit_exec_binary("ExecStart=__BIN__ daemon run").as_deref(), Some("__BIN__"));
    }

    #[test]
    fn task_binary_reads_localized_schtasks_output() {
        let en = "Task To Run:  \"C:\\Users\\u\\monk.exe\" daemon run\n";
        assert_eq!(task_binary(en).as_deref(), Some("C:\\Users\\u\\monk.exe"));
        // ru-RU labels differ, the value does not
        let ru = "Задача для выполнения: \"C:\\monk\\monk.exe\" daemon run\n";
        assert_eq!(task_binary(ru).as_deref(), Some("C:\\monk\\monk.exe"));
        assert_eq!(task_binary("Status: Ready\n"), None);
    }
}
