//! TurboDM entry point.
//!
//!   turbodm                     open the window (or show the running one)
//!   turbodm URL...              add downloads (to the running instance if any)
//!   turbodm --background        start hidden in the tray (used by the browser)
//!   turbodm --native-host       browser native-messaging host
//!   turbodm --register          (re)connect the browser extension; --unregister undoes it
//!   turbodm get URL [-c N] [-d DIR]   download in the terminal, no window

#![cfg_attr(windows, windows_subsystem = "windows")] // no console window behind the app

mod cli;
mod ui;

use turbodm::ipc::{self, Message};

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
            Some(flag @ ("--register" | "--unregister")) => {
                let result = if flag == "--register" { turbodm::register::register() } else { turbodm::register::unregister() };
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
                    .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
                    .map(|url| Message::Download {
                        url: url.clone(),
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
