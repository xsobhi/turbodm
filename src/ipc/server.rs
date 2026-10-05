//! The app side: a user-only Unix socket (a named pipe on Windows) that accepts one JSON
//! message per line.

use super::{Message, Reply};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

const MAX_MESSAGE: u64 = 1 << 20;

#[cfg(unix)]
pub use unix::{bind, serve, Listener};
#[cfg(windows)]
pub use windows::{bind, serve, Listener};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    pub type Listener = UnixListener;

    /// Bind the socket. Fails with AddrInUse when another TurboDM is already listening.
    pub fn bind(path: &Path) -> std::io::Result<Listener> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "already running"));
            }
            std::fs::remove_file(path)?; // stale socket from a crashed instance
        }
        let listener = UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(listener)
    }

    /// Accept connections forever, forwarding every non-ping message to the UI.
    pub async fn serve(listener: Listener, to_ui: async_channel::Sender<Message>) {
        while let Ok((stream, _)) = listener.accept().await {
            let to_ui = to_ui.clone();
            tokio::spawn(async move {
                let _ = handle(stream, to_ui).await;
            });
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::OsString;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    /// The pipe's name and its first instance, waiting for a client.
    pub type Listener = (OsString, NamedPipeServer);

    /// Create the pipe. Fails when another TurboDM already owns it.
    pub fn bind(path: &Path) -> std::io::Result<Listener> {
        let name = path.as_os_str().to_owned();
        let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
        Ok((name, server))
    }

    /// Accept connections forever, forwarding every non-ping message to the UI.
    pub async fn serve((name, mut server): Listener, to_ui: async_channel::Sender<Message>) {
        loop {
            if server.connect().await.is_err() {
                return;
            }
            let Ok(next) = ServerOptions::new().reject_remote_clients(true).create(&name) else { return };
            let connected = std::mem::replace(&mut server, next); // a new instance for the next client
            let to_ui = to_ui.clone();
            tokio::spawn(async move {
                let _ = handle(connected, to_ui).await;
            });
        }
    }
}

async fn handle(stream: impl AsyncRead + AsyncWrite, to_ui: async_channel::Sender<Message>) -> std::io::Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut line = String::new();
    BufReader::new(read.take(MAX_MESSAGE)).read_line(&mut line).await?;
    let reply = match serde_json::from_str::<Message>(line.trim()) {
        Ok(Message::Ping) => Reply::ok(),
        Ok(message) => match to_ui.send(message).await {
            Ok(()) => Reply::ok(),
            Err(_) => Reply::error("TurboDM is shutting down"),
        },
        Err(err) => Reply::error(format!("bad message: {err}")),
    };
    let mut out = serde_json::to_vec(&reply)?;
    out.push(b'\n');
    write.write_all(&out).await?;
    write.flush().await
}
