use miette::Diagnostic;
use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error("io error: {0}")]
    #[diagnostic(code(monk::io))]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    #[diagnostic(code(monk::config))]
    Config(String),

    #[error("invalid config file")]
    #[diagnostic(code(monk::config::parse))]
    ConfigParse(#[from] toml::de::Error),

    #[error("storage error: {0}")]
    #[diagnostic(code(monk::storage))]
    Storage(#[from] rusqlite::Error),

    #[error("daemon not running")]
    #[diagnostic(code(monk::daemon::not_running), help("start it with `monk daemon start`"))]
    DaemonNotRunning,

    #[error("daemon already running (pid {0})")]
    #[diagnostic(code(monk::daemon::already_running))]
    DaemonAlreadyRunning(u32),

    #[error("hard mode is active")]
    #[diagnostic(
        code(monk::hard_mode::active),
        help("use `monk panic` to request an escape with delay")
    )]
    HardModeActive,

    #[error("permission denied: {0}")]
    #[diagnostic(
        code(monk::permission),
        help("monk needs elevated privileges to modify the hosts file")
    )]
    Permission(String),

    #[error("ipc error: {0}")]
    #[diagnostic(code(monk::ipc))]
    Ipc(String),

    #[error("session not found: {0}")]
    #[diagnostic(code(monk::session::not_found))]
    SessionNotFound(String),

    #[error("invalid duration: {0}")]
    #[diagnostic(code(monk::duration))]
    Duration(#[from] humantime::DurationError),

    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Ipc(e.to_string())
    }
}
