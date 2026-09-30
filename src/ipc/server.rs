//! The app side: a user-only Unix socket that accepts one JSON message per line.

use super::{Message, Reply};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

const MAX_MESSAGE: u64 = 1 << 20;

/// Bind the socket. Fails with AddrInUse when another TurboDM is already listening.
pub fn bind(path: &PathBuf) -> std::io::Result<UnixListener> {
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
pub async fn serve(listener: UnixListener, to_ui: async_channel::Sender<Message>) {
    while let Ok((stream, _)) = listener.accept().await {
        let to_ui = to_ui.clone();
        tokio::spawn(async move {
            let _ = handle(stream, to_ui).await;
        });
    }
}

async fn handle(stream: UnixStream, to_ui: async_channel::Sender<Message>) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
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
    write.write_all(&out).await
}
