//! GTK4 user interface: app lifetime, single instance, IPC messages, notifications.

mod actions;
mod add_dialog;
#[cfg(target_os = "linux")]
mod center;
#[cfg(not(target_os = "linux"))]
mod center {
    /// Elsewhere the desktop places new windows itself.
    pub fn on_screen(_window: &gtk::Window, _minimizable: bool) {}
}
mod clipboard;
mod finish;
mod item;
mod launch;
mod list;
mod power;
mod progress;
mod settings;
mod sidebar;
mod toolbar;
#[cfg(target_os = "linux")]
mod tray;
#[cfg(not(target_os = "linux"))]
mod tray {
    /// No tray icon here: closing the window while downloading minimizes it.
    pub fn start(_ctx: &std::rc::Rc<super::Ctx>) {}
}
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use turbodm::config::{self, Settings, APP_ID};
use turbodm::engine::{AddRequest, Manager, Snapshot};
use turbodm::ipc::{self, Message};

/// Shared by every part of the UI (GTK thread only).
pub struct Ctx {
    pub app: gtk::Application,
    pub manager: Arc<Manager>,
    pub rt: tokio::runtime::Handle,
    pub win: window::MainWindow,
    pub progress: RefCell<HashMap<String, progress::ProgressWindow>>,
    pub on_done: RefCell<HashMap<String, finish::OnDone>>, // downloads with a progress window
    pub tray: Cell<bool>, // a tray icon is showing (else closing minimizes)
    pub launching: Cell<usize>, // files being handed to the desktop: don't quit yet
}

impl Ctx {
    pub fn show(&self) {
        self.win.window.present();
        window::refresh(self); // the list isn't kept up to date while hidden
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
    // keep the browser extension pointed at this copy of TurboDM, wherever it's installed
    std::thread::spawn(|| {
        if let Err(err) = turbodm::register::register() {
            eprintln!("turbodm: connecting the browser extension: {err}");
        }
    });
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
            on_done: RefCell::new(HashMap::new()),
            tray: Cell::new(false),
            launching: Cell::new(0),
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
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let (Ok(mut term), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()))
                else { return };
            tokio::select! { _ = term.recv() => {}, _ = int.recv() => {} }
        }
        #[cfg(windows)]
        if tokio::signal::ctrl_c().await.is_err() {
            return;
        }
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
                Message::Download { url, filename, directory, referrer, cookies, user_agent, silent, .. } => {
                    let directory = directory.filter(|d| d.is_absolute());
                    let req = AddRequest { url, filename, directory, referrer, cookies, user_agent, ..Default::default() };
                    ctx.handle_download(req, silent);
                }
            }
        }
    });
}

/// Downloads finishing or failing: complete dialog, completion options, notifications.
fn listen_events(ctx: &Rc<Ctx>, rx: async_channel::Receiver<Snapshot>) {
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(snap) = rx.recv().await {
            finish::on_status(&ctx, snap);
        }
    });
}
