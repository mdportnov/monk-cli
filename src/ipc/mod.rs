pub mod protocol;

pub use protocol::{Envelope, HardModeInfo, ModeSummary, Request, Response, PROTOCOL_VERSION};

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use interprocess::local_socket::tokio::{prelude::*, Stream};
#[cfg(not(windows))]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

#[cfg(not(windows))]
use crate::paths;
use crate::{Error, Result};

fn socket_name() -> Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        r"\\.\pipe\monkd".to_ns_name::<GenericNamespaced>().map_err(|e| Error::Ipc(e.to_string()))
    }
    #[cfg(not(windows))]
    {
        let p = paths::ipc_socket()?;
        p.to_fs_name::<GenericFilePath>().map_err(|e| Error::Ipc(e.to_string()))
    }
}

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder().max_frame_length(4 * 1024 * 1024).new_codec()
}

pub async fn send(req: &Request) -> Result<Response> {
    let name = socket_name()?;
    let stream = tokio::time::timeout(Duration::from_secs(2), Stream::connect(name))
        .await
        .map_err(|_| Error::DaemonNotRunning)?
        .map_err(|_| Error::DaemonNotRunning)?;
    let (reader, writer) = stream.split();
    let mut sink = FramedWrite::new(writer, codec());
    let mut source = FramedRead::new(reader, codec());

    let env = Envelope { v: PROTOCOL_VERSION, body: req };
    let payload = serde_json::to_vec(&env)?;
    sink.send(payload.into()).await.map_err(|e| Error::Ipc(e.to_string()))?;

    let frame = source
        .next()
        .await
        .ok_or_else(|| Error::Ipc("eof before response".into()))?
        .map_err(|e| Error::Ipc(e.to_string()))?;
    let env: Envelope<Response> = serde_json::from_slice(&frame)?;
    if env.v != PROTOCOL_VERSION {
        return Err(Error::Ipc(format!(
            "protocol version mismatch: daemon={}, client={}",
            env.v, PROTOCOL_VERSION
        )));
    }
    Ok(env.body)
}
