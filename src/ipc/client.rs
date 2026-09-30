//! The sending side: used by the native-messaging host and by `turbodm URL`.

use super::{Message, Reply};
use crate::config::socket_path;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Send one message to the running app.
pub fn send(message: &Message) -> std::io::Result<Reply> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut out = serde_json::to_vec(message)?;
    out.push(b'\n');
    stream.write_all(&out)?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(line.trim())?)
}

pub fn is_running() -> bool {
    send(&Message::Ping).is_ok()
}

/// Start the app detached from the caller (browser or terminal), hidden in the tray.
pub fn launch_background() -> std::io::Result<()> {
    Command::new(std::env::current_exe()?)
        .arg("--background")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
}

/// Send, starting the app first if it isn't running (waits up to 10 s for it).
pub fn send_or_launch(message: &Message) -> std::io::Result<Reply> {
    if let Ok(reply) = send(message) {
        return Ok(reply);
    }
    launch_background()?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        std::thread::sleep(Duration::from_millis(150));
        match send(message) {
            Ok(reply) => return Ok(reply),
            Err(err) if Instant::now() > deadline => return Err(err),
            Err(_) => continue,
        }
    }
}
