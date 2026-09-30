//! GTK4 user interface: app lifetime, single instance, IPC messages, notifications.

mod actions;
mod add_dialog;
mod center;
mod clipboard;
mod item;
mod list;
mod progress;
mod segment_bar;
mod settings;
mod tray;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use turbodm::config::{self, Settings, APP_ID};
use turbodm::engine::{AddRequest, Manager, Snapshot, Status};
use turbodm::ipc::{self, Message};

/// Shared by every part of the UI (GTK thread only).
pub struct Ctx {
    pub app: gtk::Application,
    pub manager: Arc<Manager>,
    pub rt: tokio::runtime::Handle,
    pub win: window::MainWindow,
    pub progress: RefCell<HashMap<String, progress::ProgressWindow>>,
    pub tray: Cell<bool>, // a tray icon is showing (else closing minimizes)
}

impl Ctx {
    pub fn show(&self) {
        self.win.window.present();
    }

    /// A download request from the browser, the clipboard, or the command line.
    pub fn handle_download(self: &Rc<Self>, req: AddRequest, silent: bool) {
        if !silent && self.manager.settings().show_add_dialog {
            add_dialog::open(self, req);
        } else {
            let id = self.manager.add(AddRequest { start: true, ..req });
            if self.manager.settings().show_progress_window {
                progress::open(self, &id);
            }
        }
    }
}

/// Start the GUI. `messages` are handed to the already-running instance if there is one.
pub fn run(messages: Vec<Message>, background: bool) -> i32 {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all()
        .build().expect("runtime"); // network-bound: a few threads are plenty
    let _enter = runtime.enter();
    let listener = match ipc::server::bind(&config::socket_path()) {
        Ok(listener) => listener,
        Err(_) => {
            let messages = if messages.is_empty() && !background { vec![Message::Show] } else { messages };
            for message in &messages {
                let _ = ipc::client::send(message);
            }
            return 0; // another TurboDM is running and got our request
        }
    };
    let (manager, events) = Manager::new(Settings::load(), config::downloads_file(), runtime.handle().clone());
    let (to_ui, from_ipc) = async_channel::unbounded::<Message>();
    runtime.spawn(ipc::server::serve(listener, to_ui.clone()));
    for message in messages {
        let _ = to_ui.try_send(message);
    }
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE) // single instance is handled by the socket
        .build();
    let hold = RefCell::new(None);
    let rt = runtime.handle().clone();
    let manager_for_app = manager.clone();
    app.connect_activate(move |app| {
        if hold.borrow().is_some() {
            return;
        }
        *hold.borrow_mut() = Some(app.hold()); // keep running while the window is hidden
        let ctx = Rc::new(Ctx {
            app: app.clone(),
            manager: manager_for_app.clone(),
            rt: rt.clone(),
            win: window::MainWindow::new(app),
            progress: RefCell::new(HashMap::new()),
            tray: Cell::new(false),
        });
        window::setup(&ctx);
        actions::install(&ctx);
        tray::start(&ctx);
        clipboard::start(&ctx);
        listen_ipc(&ctx, from_ipc.clone());
        listen_events(&ctx, events.clone());
        if !background {
            ctx.show();
        }
    });
    // logout/shutdown/kill send SIGTERM: quit properly so downloads are paused and saved
    let quit = to_ui.clone();
    runtime.spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()))
            else { return };
        tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
        let _ = quit.send(Message::Quit).await;
    });
    let code = app.run_with_args(&["turbodm"]);
    manager.shutdown();
    runtime.shutdown_timeout(std::time::Duration::from_secs(3));
    code.into()
}

fn listen_ipc(ctx: &Rc<Ctx>, rx: async_channel::Receiver<Message>) {
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(message) = rx.recv().await {
            match message {
                Message::Ping => {}
                Message::Show => ctx.show(),
                Message::Quit => ctx.app.quit(),
                Message::Download { url, filename, referrer, cookies, user_agent, silent, .. } => {
                    let req = AddRequest { url, filename, referrer, cookies, user_agent, ..Default::default() };
                    ctx.handle_download(req, silent);
                }
            }
        }
    });
}

/// Desktop notifications when downloads finish or fail.
fn listen_events(ctx: &Rc<Ctx>, rx: async_channel::Receiver<Snapshot>) {
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(snap) = rx.recv().await {
            if !ctx.manager.settings().notify_complete {
                continue;
            }
            let notification = match snap.status {
                Status::Completed => {
                    let n = gio::Notification::new("Download complete");
                    n.set_body(Some(&snap.filename));
                    n.add_button_with_target_value("Open", "app.open-file", Some(&snap.id.to_variant()));
                    n.add_button_with_target_value("Show in folder", "app.open-folder", Some(&snap.id.to_variant()));
                    n
                }
                Status::Error => {
                    let n = gio::Notification::new("Download failed");
                    n.set_body(Some(&format!("{}\n{}", snap.filename, snap.error.unwrap_or_default())));
                    n
                }
                _ => continue,
            };
            notification.set_icon(&gio::ThemedIcon::new("folder-download"));
            ctx.app.send_notification(Some(&format!("tdm-{}", snap.id)), &notification);
        }
    });
}
