use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::paths;

pub fn init() {
    let filter =
        EnvFilter::try_from_env("MONK_LOG").unwrap_or_else(|_| EnvFilter::new("warn,monk=info"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false).with_writer(std::io::stderr))
        .try_init();
}

pub fn init_daemon() -> Option<WorkerGuard> {
    let log_dir = paths::data_dir().ok()?;
    let appender = rolling::daily(&log_dir, "monkd.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter =
        EnvFilter::try_from_env("MONK_LOG").unwrap_or_else(|_| EnvFilter::new("info,monk=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false).with_writer(std::io::stderr))
        .with(fmt::layer().with_ansi(false).with_target(false).with_writer(writer))
        .init();
    Some(guard)
}
