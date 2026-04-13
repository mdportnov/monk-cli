use std::{
    net::{Ipv4Addr, SocketAddr},
    time::{Duration, Instant},
};

use crate::{
    blocker::{self, dns_server, BlockerBackend, HostsBlocker, ProbeResult},
    config::Config,
    daemon::PidFile,
    ipc::{self, Request, Response},
    paths,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Info,
    Skipped,
}

impl Status {
    pub fn severity_rank(self) -> u8 {
        match self {
            Status::Fail => 0,
            Status::Warn => 1,
            Status::Ok => 2,
            Status::Info => 3,
            Status::Skipped => 4,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Status::Ok => "✓",
            Status::Warn => "!",
            Status::Fail => "✗",
            Status::Info => "·",
            Status::Skipped => "—",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Info => "info",
            Status::Skipped => "skip",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub id: &'static str,
    pub title: String,
    pub purpose: &'static str,
    pub status: Status,
    pub detail: String,
    pub hint: Option<String>,
    pub extras: Vec<String>,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    StartDaemon,
    StopDaemon,
    OpenConfig,
    OpenLog,
    OpenDataDir,
}

#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub key: char,
    pub label: &'static str,
    pub kind: ActionKind,
}

impl ActionKind {
    pub fn run(self) -> std::result::Result<String, String> {
        match self {
            ActionKind::StartDaemon => start_daemon(),
            ActionKind::StopDaemon => stop_daemon(),
            ActionKind::OpenConfig => open_path_action(paths::config_file()),
            ActionKind::OpenLog => open_path_action(paths::log_file()),
            ActionKind::OpenDataDir => open_path_action(paths::data_dir()),
        }
    }
}

fn start_daemon() -> std::result::Result<String, String> {
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

fn stop_daemon() -> std::result::Result<String, String> {
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

fn open_path_action(p: crate::Result<std::path::PathBuf>) -> std::result::Result<String, String> {
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

pub fn purpose_for(id: &str) -> &'static str {
    match id {
        "version" => "build identity — confirms which binary you're running",
        "platform" => "host os/arch — gates backend selection",
        "privileges" => "root access — required by hosts, resolver_dir, systemd_resolved, and :80",
        "path.config" => "where monk stores your profiles and general settings",
        "path.data" => "where monk stores audit logs and mode stats",
        "path.log" => "rolling log file for daemon output",
        "path.socket" => "unix socket for cli↔daemon ipc",
        "config" => "parses config.toml and reports profile count",
        "daemon" => "pidfile inspection — is monkd actually alive?",
        "ipc" => "round-trip ping to monkd over the socket",
        "hard_mode" => "is an unescapable session currently locking the machine?",
        "blocker.backends" => "which site blockers are available on this host",
        "dns_server" => "local dns responder on 127.0.0.1:53535 (answers blocked domains)",
        "block_page" => "http server on :80 that renders the blocked-site placeholder",
        "log" => "tail of the daemon log for quick triage",
        _ => "",
    }
}

impl Check {
    fn new(
        id: &'static str,
        title: impl Into<String>,
        status: Status,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            purpose: purpose_for(id),
            status,
            detail: detail.into(),
            hint: None,
            extras: Vec::new(),
            actions: Vec::new(),
        }
    }

    fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn with_extras(mut self, extras: Vec<String>) -> Self {
        self.extras = extras;
        self
    }

    fn with_actions(mut self, actions: Vec<Action>) -> Self {
        self.actions = actions;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub checks: Vec<Check>,
    pub duration: Duration,
}

impl Report {
    pub fn summary(&self) -> (usize, usize, usize) {
        let mut ok = 0;
        let mut warn = 0;
        let mut fail = 0;
        for c in &self.checks {
            match c.status {
                Status::Ok => ok += 1,
                Status::Warn => warn += 1,
                Status::Fail => fail += 1,
                _ => {}
            }
        }
        (ok, warn, fail)
    }

    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == Status::Fail)
    }

    /// Indices into `checks`, sorted by severity (Fail → Warn → Ok → Info → Skipped),
    /// preserving original order within the same severity.
    pub fn display_order(&self) -> Vec<usize> {
        let mut idx: Vec<usize> = (0..self.checks.len()).collect();
        idx.sort_by_key(|&i| (self.checks[i].status.severity_rank(), i));
        idx
    }
}

pub async fn run() -> Report {
    let start = Instant::now();
    let mut checks = Vec::new();

    checks.push(check_version());
    checks.push(check_platform());
    checks.push(check_privileges());
    checks.extend(check_paths());
    checks.push(check_config());
    checks.push(check_pidfile());
    checks.push(check_ipc().await);
    checks.push(check_hard_mode().await);
    checks.push(check_blocker_backend());
    checks.push(check_dns_server().await);
    checks.push(check_block_page().await);
    checks.push(check_log_tail());

    Report { checks, duration: start.elapsed() }
}

fn check_version() -> Check {
    Check::new("version", "monk version", Status::Info, format!("v{}", env!("CARGO_PKG_VERSION")))
}

fn check_platform() -> Check {
    Check::new(
        "platform",
        "platform",
        Status::Info,
        format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH),
    )
}

fn check_privileges() -> Check {
    #[cfg(unix)]
    {
        let is_root = nix::unistd::geteuid().is_root();
        if is_root {
            Check::new("privileges", "privileges", Status::Ok, "running as root")
        } else {
            Check::new("privileges", "privileges", Status::Warn, "not running as root")
                .with_hint("some blocker backends require root (run `sudo monk ...`)")
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

fn check_paths() -> Vec<Check> {
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

fn check_config() -> Check {
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
            .with_hint("fix or delete the file at `path.config`"),
    }
}

fn check_pidfile() -> Check {
    let start = Action { key: 's', label: "start daemon", kind: ActionKind::StartDaemon };
    let stop = Action { key: 'x', label: "stop daemon", kind: ActionKind::StopDaemon };
    match PidFile::new() {
        Ok(p) => match p.is_alive() {
            Ok(Some(pid)) => {
                Check::new("daemon", "daemon", Status::Ok, format!("running (pid {pid})"))
                    .with_actions(vec![stop])
            }
            Ok(None) => Check::new("daemon", "daemon", Status::Warn, "not running")
                .with_hint("start it with `monk daemon` (or `sudo monk daemon`)")
                .with_actions(vec![start]),
            Err(e) => Check::new("daemon", "daemon", Status::Fail, format!("pidfile read: {e}"))
                .with_actions(vec![start]),
        },
        Err(e) => Check::new("daemon", "daemon", Status::Fail, format!("pidfile init: {e}")),
    }
}

async fn check_ipc() -> Check {
    let start = Action { key: 's', label: "start daemon", kind: ActionKind::StartDaemon };
    match ipc::send(&Request::Ping).await {
        Ok(Response::Pong { version }) => {
            Check::new("ipc", "ipc", Status::Ok, format!("daemon v{version} responds"))
        }
        Ok(other) => Check::new("ipc", "ipc", Status::Warn, format!("unexpected: {other:?}")),
        Err(e) => Check::new("ipc", "ipc", Status::Fail, format!("send failed: {e}"))
            .with_hint("daemon may be down or socket path mismatched")
            .with_actions(vec![start]),
    }
}

async fn check_hard_mode() -> Check {
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

fn check_blocker_backend() -> Check {
    let mut lines = Vec::new();
    let hosts = blocker::hosts_path();
    let writable = fs_err::OpenOptions::new().append(true).open(&hosts).is_ok();
    lines.push(format!(
        "hosts: {} ({})",
        hosts.display(),
        if writable { "writable" } else { "read-only" }
    ));

    match HostsBlocker::probe() {
        ProbeResult::Available { priority, detail } => {
            lines.push(format!("hosts backend: available (priority {priority}, {detail})"));
        }
        ProbeResult::Unavailable { reason } => {
            lines.push(format!("hosts backend: unavailable — {reason}"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        match crate::blocker::backends::resolver_dir::ResolverDirBlocker::probe() {
            ProbeResult::Available { priority, detail } => {
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
                lines.push(format!("systemd_resolved: available (priority {priority}, {detail})"));
            }
            ProbeResult::Unavailable { reason } => {
                lines.push(format!("systemd_resolved: unavailable — {reason}"));
            }
        }
    }

    let status = if writable { Status::Ok } else { Status::Warn };
    let detail = if writable {
        "hosts writable"
    } else {
        "hosts not writable — elevated privileges required"
    };
    Check::new("blocker.backends", "blocker backends", status, detail).with_extras(lines)
}

async fn check_dns_server() -> Check {
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

async fn check_block_page() -> Check {
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

fn check_log_tail() -> Check {
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

    fn c(id: &'static str, s: Status) -> Check {
        Check::new(id, id, s, "")
    }

    #[test]
    fn summary_counts_ok_warn_fail() {
        let r = Report {
            checks: vec![
                c("a", Status::Ok),
                c("b", Status::Ok),
                c("c", Status::Warn),
                c("d", Status::Fail),
                c("e", Status::Info),
                c("f", Status::Skipped),
            ],
            duration: Duration::ZERO,
        };
        assert_eq!(r.summary(), (2, 1, 1));
        assert!(r.has_failures());
    }

    #[test]
    fn display_order_sorts_by_severity_stable() {
        let r = Report {
            checks: vec![
                c("a", Status::Ok),
                c("b", Status::Fail),
                c("c", Status::Ok),
                c("d", Status::Warn),
                c("e", Status::Fail),
                c("f", Status::Info),
            ],
            duration: Duration::ZERO,
        };
        let order = r.display_order();
        let ids: Vec<&str> = order.iter().map(|&i| r.checks[i].id).collect();
        assert_eq!(ids, vec!["b", "e", "d", "a", "c", "f"]);
    }

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
}
