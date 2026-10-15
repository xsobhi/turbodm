//! TurboDM entry point.
//!
//!   turbodm                     open the window (or show the running one)
//!   turbodm URL...              add downloads, magnet links, .torrent files (to the running TurboDM)
//!   turbodm --background        start hidden in the tray (used by the browser)
//!   turbodm --native-host       browser native-messaging host
//!   turbodm --register          (re)connect the browser extension; --unregister undoes it
//!   turbodm get URL [-c N] [-d DIR]   download in the terminal, no window

#![cfg_attr(windows, windows_subsystem = "windows")] // no console window behind the app

mod cli;
#[cfg(not(windows))]
mod ui; // GTK
#[cfg(windows)]
mod win; // Windows' own controls
#[cfg(windows)]
use win as ui;

use turbodm::ipc::{self, Message};

/// A link, a magnet link or a .torrent file (made absolute: the running TurboDM that gets it
/// may be in another folder).
fn download_arg(arg: &str) -> Option<String> {
    use turbodm::engine::torrent::is_torrent;
    if arg.starts_with("http://") || arg.starts_with("https://") || arg.starts_with("magnet:") {
        return Some(arg.to_string());
    }
    let local = is_torrent(arg, None) && std::path::Path::new(arg).is_file();
    local.then(|| std::path::absolute(arg).map_or(arg.to_string(), |p| p.display().to_string()))
}

fn is_native_host_call(args: &[String]) -> bool {
    // Chrome passes the caller's origin; Firefox passes the manifest path and addon id.
    args.first().is_some_and(|a| {
        a == "--native-host" || a.starts_with("chrome-extension://") || a.ends_with(".json")
    })
}

/// Windows: print to the terminal we were started from (the app itself has no console).
#[cfg(windows)]
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(process: u32) -> i32;
    }
    // SAFETY: plain Win32 call; failing (no parent console) is fine
    unsafe { AttachConsole(u32::MAX) };
}

#[cfg(not(windows))]
fn attach_console() {}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), Some("get" | "--version" | "-V" | "--help" | "-h")) {
        attach_console();
    }
    let code = if is_native_host_call(&args) {
        ipc::native_host::run()
    } else {
        match args.first().map(String::as_str) {
            Some("get") => cli::get(&args[1..]),
            Some(flag @ ("--register" | "--unregister" | "--register-system" | "--unregister-system")) => {
                use turbodm::register::*;
                let result = match flag {
                    "--register" => register(),
                    "--unregister" => unregister(),
                    "--register-system" => register_system(), // the installer, for all users
                    _ => unregister_system(),
                };
                match result {
                    Ok(()) => 0,
                    Err(err) => {
                        eprintln!("turbodm {flag}: {err}");
                        1
                    }
                }
            }
            Some("--version" | "-V") => {
                println!("TurboDM {}", turbodm::config::VERSION);
                0
            }
            Some("--help" | "-h") => {
                println!("{}", include_str!("main.rs").lines().skip(2).take(7)
                    .map(|l| l.trim_start_matches("//!")).collect::<Vec<_>>().join("\n"));
                0
            }
            _ => {
                let background = args.iter().any(|a| a == "--background");
                let messages = args
                    .iter()
                    .filter_map(|a| download_arg(a))
                    .map(|url| Message::Download {
                        url,
                        filename: None,
                        directory: None,
                        referrer: None,
                        cookies: None,
                        user_agent: None,
                        file_size: None,
                        silent: false,
                    })
                    .collect();
                ui::run(messages, background)
            }
        }
    };
    std::process::exit(code);
}
