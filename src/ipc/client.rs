//! The sending side: used by the native-messaging host and by `turbodm URL`.

use super::{Message, Reply};
use crate::config::socket_path;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
fn connect() -> std::io::Result<std::os::unix::net::UnixStream> {
    let stream = std::os::unix::net::UnixStream::connect(socket_path())?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(stream)
}

/// The app's named pipe, opened like a file (retried while it's busy with another client).
#[cfg(windows)]
fn connect() -> std::io::Result<std::fs::File> {
    const PIPE_BUSY: i32 = 231;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::fs::OpenOptions::new().read(true).write(true).open(socket_path()) {
            Err(e) if e.raw_os_error() == Some(PIPE_BUSY) && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            result => return result,
        }
    }
}

/// Send one message to the running app.
pub fn send(message: &Message) -> std::io::Result<Reply> {
    let mut stream = connect()?;
    let mut out = serde_json::to_vec(message)?;
    out.push(b'\n');
    stream.write_all(&out)?;
    stream.flush()?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    Ok(serde_json::from_str(line.trim())?)
}

pub fn is_running() -> bool {
    send(&Message::Ping).is_ok()
}

/// Start the app detached from the caller (browser or terminal), hidden in the tray.
pub fn launch_background() -> std::io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--background").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    #[cfg(windows)]
    {
        const DETACHED_PROCESS: u32 = 0x8;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
        std::os::windows::process::CommandExt::creation_flags(&mut command, DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(|_| ())
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
