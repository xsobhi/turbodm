//! A tiny HTTP/1.1 test server: byte ranges on/off, throttling, unknown size; and the
//! tests' harness (harness.rs).
#![allow(dead_code)] // each test file uses part of it

pub mod harness;
pub mod video;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;

#[derive(Clone)]
pub struct Options {
    pub ranges: bool,
    pub no_length: bool,
    pub max_conns: usize, // answer 429 above this many parallel requests (0 = no limit)
}

pub struct TestServer {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Option<String>>>>, // Range header of every request
    pub delay_ms: Arc<AtomicU64>,                  // pause after each 64 KiB chunk
    pub content: Arc<Mutex<Arc<Vec<u8>>>>,         // what it serves (changed: a new file, same address)
    pub headers: Arc<Mutex<String>>,               // more header lines for every answer ("Name: value\r\n")
    pub busy: Arc<AtomicU64>,                      // answer this many requests next with 503 and Retry-After: 1
}

pub fn start(rt: &Runtime, data: Arc<Vec<u8>>, opts: Options, delay_ms: u64) -> TestServer {
    let listener = rt.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = TestServer {
        url: format!("http://127.0.0.1:{port}/files/test.bin"),
        requests: Arc::new(Mutex::new(vec![])),
        delay_ms: Arc::new(AtomicU64::new(delay_ms)),
        content: Arc::new(Mutex::new(data)),
        headers: Arc::new(Mutex::new(String::new())),
        busy: Arc::new(AtomicU64::new(0)),
    };
    let (requests, delay) = (server.requests.clone(), server.delay_ms.clone());
    let (content, extra, busy) = (server.content.clone(), server.headers.clone(), server.busy.clone());
    let live = Arc::new(AtomicU64::new(0));
    rt.spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (data, opts, requests, delay) = (content.lock().unwrap().clone(), opts.clone(), requests.clone(), delay.clone());
            let extra = extra.lock().unwrap().clone();
            // (one request at a time is accepted here: no other takes from `busy` meanwhile)
            let refuse = busy.load(Ordering::SeqCst) > 0;
            if refuse {
                busy.fetch_sub(1, Ordering::SeqCst);
            }
            let live = live.clone();
            tokio::spawn(async move {
                let n = live.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = if refuse {
                    refuse_with(stream, "503 Service Unavailable\r\nRetry-After: 1").await
                } else if opts.max_conns > 0 && n as usize > opts.max_conns {
                    too_many(stream).await
                } else {
                    serve(stream, data, opts, requests, delay, extra).await
                };
                live.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    server
}

async fn serve(stream: TcpStream, data: Arc<Vec<u8>>, opts: Options,
               requests: Arc<Mutex<Vec<Option<String>>>>, delay: Arc<AtomicU64>, extra: String) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("range:") {
            range = Some(value.trim().to_string());
        }
    }
    requests.lock().unwrap().push(range.clone());
    let len = data.len() as u64;
    let mut head;
    let (start, end) = match range.as_deref().and_then(|r| r.strip_prefix("bytes=")) {
        Some(spec) if opts.ranges => {
            let (a, b) = spec.split_once('-').unwrap();
            let start: u64 = a.parse().unwrap();
            let end = if b.is_empty() { len - 1 } else { b.parse::<u64>().unwrap().min(len - 1) };
            head = format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{len}\r\n");
            (start, end + 1)
        }
        _ => {
            head = "HTTP/1.1 200 OK\r\n".to_string();
            (0, len)
        }
    };
    if !opts.no_length {
        head += &format!("Content-Length: {}\r\n", end - start);
    }
    head += "Content-Type: application/octet-stream\r\n";
    head += &extra;
    head += "Content-Disposition: attachment; filename=\"test file.bin\"\r\nConnection: close\r\n\r\n";
    let mut stream = reader.into_inner();
    stream.write_all(head.as_bytes()).await?;
    for chunk in data[start as usize..end as usize].chunks(64 * 1024) {
        stream.write_all(chunk).await?;
        let ms = delay.load(Ordering::Relaxed);
        if ms > 0 {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
    }
    stream.shutdown().await
}

async fn too_many(stream: TcpStream) -> std::io::Result<()> {
    refuse_with(stream, "429 Too Many Requests").await
}

/// Read the request, answer `status` (and any header lines after it) with no body.
async fn refuse_with(stream: TcpStream, status: &str) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    while reader.read_line(&mut line).await? > 2 {
        line.clear();
    }
    let mut stream = reader.into_inner();
    let answer = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(answer.as_bytes()).await?;
    stream.shutdown().await
}

pub fn random_bytes(len: usize) -> Arc<Vec<u8>> {
    let mut state = 0x9E3779B97F4A7C15u64; // xorshift: fast, deterministic test data
    Arc::new((0..len).map(|_| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 24) as u8
    }).collect())
}
