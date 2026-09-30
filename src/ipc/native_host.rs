//! Browser native-messaging host (`turbodm --native-host`).
//!
//! Browsers start this with stdin/stdout pipes and exchange messages framed as a
//! 32-bit native-endian length followed by UTF-8 JSON. Each message is relayed to
//! the running app (started in the background when needed).

use super::client::{send, send_or_launch};
use super::{Message, Reply};
use std::io::{Read, Write};

const MAX_MESSAGE: usize = 1 << 20;

fn read_message(input: &mut impl Read) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return None;
    }
    let mut buf = vec![0; len];
    input.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn write_message(output: &mut impl Write, reply: &Reply) -> std::io::Result<()> {
    let json = serde_json::to_vec(reply)?;
    output.write_all(&(json.len() as u32).to_ne_bytes())?;
    output.write_all(&json)?;
    output.flush()
}

pub fn run() -> i32 {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    while let Some(raw) = read_message(&mut input) {
        let reply = match serde_json::from_slice::<Message>(&raw) {
            // a status check from the popup must not launch the app
            Ok(Message::Ping) => send(&Message::Ping).unwrap_or(Reply { ok: true, ..Default::default() }),
            Ok(message) => send_or_launch(&message).unwrap_or_else(|e| Reply::error(e.to_string())),
            Err(err) => Reply::error(format!("bad message: {err}")),
        };
        if write_message(&mut output, &reply).is_err() {
            break;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_roundtrip() {
        let mut buf = vec![];
        write_message(&mut buf, &Reply::ok()).unwrap();
        let body = read_message(&mut buf.as_slice()).unwrap();
        let reply: Reply = serde_json::from_slice(&body).unwrap();
        assert!(reply.ok && reply.running);
    }
}
