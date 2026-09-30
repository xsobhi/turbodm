//! TurboDM entry point.
//!
//!   turbodm                     open the window (or show the running one)
//!   turbodm URL...              add downloads (to the running instance if any)
//!   turbodm --background        start hidden in the tray (used by the browser)
//!   turbodm --native-host       browser native-messaging host
//!   turbodm get URL [-c N] [-d DIR]   download in the terminal, no window

mod cli;
mod ui;

use turbodm::ipc::{self, Message};

fn is_native_host_call(args: &[String]) -> bool {
    // Chrome passes the caller's origin; Firefox passes the manifest path and addon id.
    args.first().is_some_and(|a| {
        a == "--native-host" || a.starts_with("chrome-extension://") || a.ends_with(".json")
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = if is_native_host_call(&args) {
        ipc::native_host::run()
    } else {
        match args.first().map(String::as_str) {
            Some("get") => cli::get(&args[1..]),
            Some("--version" | "-V") => {
                println!("TurboDM {}", turbodm::config::VERSION);
                0
            }
            Some("--help" | "-h") => {
                println!("{}", include_str!("main.rs").lines().skip(2).take(6)
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
