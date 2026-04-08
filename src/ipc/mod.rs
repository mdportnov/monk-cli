pub mod protocol;

pub use protocol::{HardModeInfo, Request, Response};

use std::time::Duration;

use interprocess::local_socket::tokio::{prelude::*, Stream};
#[cfg(not(windows))]
use interprocess::local_socket::{GenericFilePath, ToFsName};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{paths, Error, Result};

fn socket_name() -> Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        Ok(r"\\.\pipe\monkd"
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| Error::Ipc(e.to_string()))?)
    }
    #[cfg(not(windows))]
    {
        let p = paths::ipc_socket()?;
        p.to_fs_name::<GenericFilePath>().map_err(|e| Error::Ipc(e.to_string()))
    }
}

pub async fn send(req: &Request) -> Result<Response> {
    let name = socket_name()?;
    let stream = tokio::time::timeout(Duration::from_secs(2), Stream::connect(name))
        .await
        .map_err(|_| Error::DaemonNotRunning)?
        .map_err(|_| Error::DaemonNotRunning)?;
    let (reader, mut writer) = stream.split();
    let payload = serde_json::to_vec(req)?;
    writer.write_all(&payload).await.map_err(|e| Error::Ipc(e.to_string()))?;
    writer.write_all(b"\n").await.map_err(|e| Error::Ipc(e.to_string()))?;
    writer.flush().await.map_err(|e| Error::Ipc(e.to_string()))?;

    let mut buf = String::new();
    BufReader::new(reader).read_line(&mut buf).await.map_err(|e| Error::Ipc(e.to_string()))?;
    let resp: Response = serde_json::from_str(buf.trim())?;
    Ok(resp)
}
