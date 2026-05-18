use std::time::Instant;

use crate::paths;

mod actions;
mod checks;
mod ui;

pub use ui::Report;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
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
}

#[derive(Debug, Clone, serde::Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    StartDaemon,
    StopDaemon,
    OpenConfig,
    OpenLog,
    OpenDataDir,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Action {
    pub key: char,
    pub label: &'static str,
    pub kind: ActionKind,
}

impl ActionKind {
    pub fn run(self) -> std::result::Result<String, String> {
        match self {
            ActionKind::StartDaemon => actions::start_daemon(),
            ActionKind::StopDaemon => actions::stop_daemon(),
            ActionKind::OpenConfig => actions::open_path_action(paths::config_file()),
            ActionKind::OpenLog => actions::open_path_action(paths::log_file()),
            ActionKind::OpenDataDir => actions::open_path_action(paths::data_dir()),
        }
    }
}

pub fn purpose_for(id: &str) -> &'static str {
    match id {
        "version" => "build identity — confirms which binary you're running",
        "platform" => "host os/arch — gates backend selection",
        "privileges" => "process privileges — daemon needs root, cli does not",
        "service.install" => "system LaunchDaemon — root daemon that owns /etc/hosts and :80",
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
    pub(crate) fn new(
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

    pub(crate) fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub(crate) fn with_extras(mut self, extras: Vec<String>) -> Self {
        self.extras = extras;
        self
    }

    pub(crate) fn with_actions(mut self, actions: Vec<Action>) -> Self {
        self.actions = actions;
        self
    }
}

pub async fn run() -> Report {
    let start = Instant::now();
    let mut checks = vec![
        checks::check_version(),
        checks::check_platform(),
        checks::check_privileges(),
        checks::check_service_install(),
    ];
    checks.extend(checks::check_paths());
    checks.push(checks::check_config());
    checks.push(checks::check_pidfile());
    checks.push(checks::check_ipc().await);
    checks.push(checks::check_hard_mode().await);
    checks.push(checks::check_blocker_backend());
    checks.push(checks::check_dns_server().await);
    checks.push(checks::check_block_page().await);
    checks.push(checks::check_log_tail());

    Report { checks, duration: start.elapsed() }
}
