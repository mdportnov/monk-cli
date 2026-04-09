use std::{sync::Arc, time::Duration};

use futures::{SinkExt, StreamExt};
use interprocess::local_socket::{tokio::prelude::*, ListenerOptions};
#[cfg(not(windows))]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use tokio::sync::Notify;
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

#[cfg(not(windows))]
use crate::paths;
use crate::{
    config::Config,
    daemon::{PidFile, Supervisor},
    ipc::{Envelope, Request, Response, PROTOCOL_VERSION},
    Error, Result,
};

pub async fn run() -> Result<()> {
    let pid = PidFile::new()?;
    pid.acquire()?;
    let _pid_guard = scopeguard::Guard::new(&pid);

    let config = Config::load()?;
    let supervisor = Arc::new(Supervisor::new(config)?);
    supervisor.restore()?;
    let shutdown = Arc::new(Notify::new());

    #[cfg(windows)]
    let name = r"\\.\pipe\monkd"
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| Error::Ipc(e.to_string()))?;
    #[cfg(not(windows))]
    let name = {
        let p = paths::ipc_socket()?;
        let _ = fs_err::remove_file(&p);
        p.to_fs_name::<GenericFilePath>().map_err(|e| Error::Ipc(e.to_string()))?
    };

    let listener =
        ListenerOptions::new().name(name).create_tokio().map_err(|e| Error::Ipc(e.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(sock_path) = paths::ipc_socket() {
            let _ = fs_err::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o666));
        }
    }

    let tick_sup = supervisor.clone();
    let tick_shutdown = shutdown.clone();
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = tick_sup.tick() {
                        tracing::warn!(?e, "tick failed");
                    }
                }
                _ = tick_shutdown.notified() => break,
            }
        }
    });

    tracing::info!("monkd listening");

    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| Error::Ipc(format!("sigterm handler: {e}")))?;
    #[cfg(unix)]
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        .map_err(|e| Error::Ipc(format!("sighup handler: {e}")))?;

    let sig_sup = supervisor.clone();
    let should_exit = |label: &str| -> bool {
        if sig_sup.hard_info().is_some() {
            tracing::warn!(signal = label, "signal ignored: hard mode active");
            false
        } else {
            tracing::info!(signal = label, "shutdown signal received");
            true
        }
    };

    'main: loop {
        #[cfg(unix)]
        let sig_ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        tokio::select! {
            _ = sig_ctrl_c => { if should_exit("ctrl_c") { break 'main; } }
            _ = sigterm.recv() => { if should_exit("sigterm") { break 'main; } }
            _ = sighup.recv() => { if should_exit("sighup") { break 'main; } }
            _ = shutdown.notified() => break 'main,
            accept = listener.accept() => {
                match accept {
                    Ok(stream) => {
                        let sup = supervisor.clone();
                        let shutdown = shutdown.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle(stream, sup, shutdown).await {
                                tracing::warn!(?e, "client error");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(?e, "accept failed"),
                }
            }
        }
        #[cfg(windows)]
        tokio::select! {
            _ = tokio::signal::ctrl_c() => { if should_exit("ctrl_c") { break 'main; } }
            _ = shutdown.notified() => break 'main,
            accept = listener.accept() => {
                match accept {
                    Ok(stream) => {
                        let sup = supervisor.clone();
                        let shutdown = shutdown.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle(stream, sup, shutdown).await {
                                tracing::warn!(?e, "client error");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(?e, "accept failed"),
                }
            }
        }
    }

    shutdown.notify_waiters();
    let _ = ticker.await;
    drop(listener);
    #[cfg(not(windows))]
    {
        if let Ok(p) = paths::ipc_socket() {
            let _ = fs_err::remove_file(p);
        }
    }
    Ok(())
}

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder().max_frame_length(4 * 1024 * 1024).new_codec()
}

async fn handle(
    stream: interprocess::local_socket::tokio::Stream,
    sup: Arc<Supervisor>,
    shutdown: Arc<Notify>,
) -> Result<()> {
    let (reader, writer) = stream.split();
    let mut source = FramedRead::new(reader, codec());
    let mut sink = FramedWrite::new(writer, codec());

    while let Some(frame) = source.next().await {
        let bytes = frame.map_err(|e| Error::Ipc(e.to_string()))?;
        let env: Envelope<Request> = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(e) => {
                let resp = Response::Error { message: format!("bad envelope: {e}") };
                let out = Envelope { v: PROTOCOL_VERSION, body: resp };
                let payload = serde_json::to_vec(&out)?;
                let _ = sink.send(payload.into()).await;
                return Ok(());
            }
        };
        if env.v != PROTOCOL_VERSION {
            let resp = Response::Error {
                message: format!(
                    "protocol version mismatch: client={}, daemon={}",
                    env.v, PROTOCOL_VERSION
                ),
            };
            let out = Envelope { v: PROTOCOL_VERSION, body: resp };
            let payload = serde_json::to_vec(&out)?;
            let _ = sink.send(payload.into()).await;
            return Ok(());
        }
        let resp = dispatch(env.body, &sup, &shutdown);
        let out = Envelope { v: PROTOCOL_VERSION, body: resp };
        let payload = serde_json::to_vec(&out)?;
        sink.send(payload.into()).await.map_err(|e| Error::Ipc(e.to_string()))?;
    }
    Ok(())
}

fn dispatch(req: Request, sup: &Arc<Supervisor>, shutdown: &Arc<Notify>) -> Response {
    match req {
        Request::Ping => Response::Pong { version: env!("CARGO_PKG_VERSION").into() },
        Request::Status => Response::Status {
            active: sup.active().map(Box::new),
            hard_mode: sup.hard_info().map(Box::new),
            pid: std::process::id(),
        },
        Request::Start { profile, duration, hard_mode, reason } => {
            let phrase = crate::session::lock::generate_phrase();
            match sup.start(profile, duration, hard_mode, reason, phrase) {
                Ok(s) => Response::Session(Box::new(s)),
                Err(e) => Response::Error { message: e.to_string() },
            }
        }
        Request::Stop { .. } => match sup.stop() {
            Ok(Some(s)) => Response::Session(Box::new(s)),
            Ok(None) => Response::Error { message: "no active session".into() },
            Err(Error::HardModeActive) => match sup.hard_info() {
                Some(info) => Response::HardModeActive(Box::new(info)),
                None => Response::Error { message: "hard mode active".into() },
            },
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::Panic { phrase, cancel } => match sup.panic(&phrase, cancel) {
            Ok(_) => match sup.hard_info() {
                Some(info) => Response::PanicScheduled(Box::new(info)),
                None => Response::Ok,
            },
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::List => Response::Sessions { sessions: sup.active().into_iter().collect() },
        Request::Shutdown => {
            if let Some(info) = sup.hard_info() {
                Response::HardModeActive(Box::new(info))
            } else {
                shutdown.notify_waiters();
                Response::Ok
            }
        }
        Request::Pause { .. } | Request::Resume { .. } => {
            Response::Error { message: "not implemented".into() }
        }
        Request::ListModes => Response::Modes { modes: sup.list_modes() },
        Request::ModeStats { name } => match sup.mode_stats(&name) {
            Ok(s) => Response::ModeStatsData(s),
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::SaveMode { name, profile } => match sup.save_mode(name, profile) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::DeleteMode { name } => match sup.delete_mode(&name) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::GetGeneral => Response::General(sup.get_general()),
        Request::UpdateGeneral { general } => match sup.update_general(general) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::ResetAll => match sup.reset_all() {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::GetConfig => Response::Config(Box::new(sup.get_config())),
        Request::SaveConfig { config } => match sup.save_config(*config) {
            Ok(()) => Response::Ok,
            Err(e) => Response::Error { message: e.to_string() },
        },
        Request::Unknown => Response::Error { message: "unknown request kind".into() },
    }
}

mod scopeguard {
    use super::PidFile;
    pub struct Guard<'a>(&'a PidFile);
    impl<'a> Guard<'a> {
        pub fn new(p: &'a PidFile) -> Self {
            Self(p)
        }
    }
    impl<'a> Drop for Guard<'a> {
        fn drop(&mut self) {
            self.0.release();
        }
    }
}
