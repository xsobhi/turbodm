//! Windows interface, made of Windows' own controls (Win32 and its common controls), which
//! Windows draws in the style of the version it runs on, like IDM. Linux has GTK (src/ui).

mod add;
mod batch;
mod browsers;
mod complete;
mod dialogs;
mod finish;
mod helpers;
mod main;
mod power;
mod progress;
mod queue;
mod refresh;
mod settings;
mod shell;
mod theme;
mod tray;
mod update;
mod wnd;

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use turbodm::config::{self, Settings};
use turbodm::engine::{AddRequest, Manager};
use turbodm::ipc::{self, Message};
use turbodm::power::OnDone;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Controls::{InitCommonControlsEx, ICC_WIN95_CLASSES, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Everything the interface shares (UI thread only).
pub struct App {
    pub manager: Arc<Manager>,
    pub rt: tokio::runtime::Handle,
    pub main: main::MainWindow,
    pub progress: RefCell<HashMap<String, progress::ProgressWindow>>,
    pub on_done: RefCell<HashMap<String, OnDone>>, // downloads with a progress window
    pub refreshing: RefCell<Option<refresh::Waiting>>, // waiting for a fresh address from the browser
    pub offered_refresh: RefCell<HashSet<String>>,     // expired links already offered a refresh
    from_browser: RefCell<Option<(String, std::time::Instant)>>, // the extension's last link, and when
    pub update: RefCell<Option<turbodm::update::Release>>, // found while hidden: shown on show()
    pub when_done: RefCell<HashMap<String, WhenDone>>, // unlisted downloads (an update, the video helpers)
    pub background: bool, // started by the browser: quit when idle and nothing is shown
}

thread_local! {
    static APP: OnceCell<Rc<App>> = const { OnceCell::new() };
    static INBOX: OnceCell<mpsc::Receiver<Job>> = const { OnceCell::new() };
}

/// What to do with a download kept out of the list once it's done or failed, instead of the
/// usual (the "Download complete" window, a notification).
pub type WhenDone = Box<dyn FnOnce(&Rc<App>, &turbodm::engine::Snapshot)>;

pub fn app() -> Rc<App> {
    APP.with(|app| app.get().expect("started").clone())
}

type Job = Box<dyn FnOnce(&Rc<App>) + Send>;
static OUTBOX: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);
pub const WM_WAKE: u32 = WM_APP + 1;

/// Run `job` on the UI thread, from any thread (network results, engine events…).
pub fn post(job: impl FnOnce(&Rc<App>) + Send + 'static) {
    if let Some(outbox) = OUTBOX.get() {
        let _ = outbox.lock().unwrap().send(Box::new(job));
        let hwnd = HWND(MAIN_WINDOW.load(Ordering::SeqCst) as *mut _);
        // SAFETY: posting to our window from any thread is what PostMessage is for
        unsafe { let _ = PostMessageW(Some(hwnd), WM_WAKE, WPARAM(0), LPARAM(0)); }
    }
}

/// The jobs posted meanwhile (the main window calls this on WM_WAKE).
fn run_posted() {
    let jobs: Vec<Job> = INBOX.with(|inbox| inbox.get().map(|rx| rx.try_iter().collect()).unwrap_or_default());
    let app = app();
    for job in jobs {
        job(&app);
    }
}

impl App {
    pub fn show(self: &Rc<Self>) {
        self.main.show();
        update::show_pending(self);
    }

    /// A download from the browser, the clipboard or the command line.
    pub fn handle_download(self: &Rc<Self>, req: AddRequest, silent: bool) {
        if !silent && self.manager.settings().show_add_dialog {
            add::open(self, req);
        } else {
            let id = self.manager.add(AddRequest { start: true, ..req });
            if self.manager.settings().show_progress_window {
                progress::open(self, &id);
            }
        }
    }

    pub fn quit(&self) {
        tray::remove(self);
        queue::remove(self);
        // SAFETY: ends our message loop
        unsafe { PostQuitMessage(0) };
    }

    fn handle_message(self: &Rc<Self>, message: Message) {
        match message {
            Message::Ping => {}
            Message::Show => self.show(),
            Message::Quit => self.quit(),
            Message::Download { url, filename, directory, referrer, cookies, user_agent, silent, .. } => {
                if user_agent.is_some() {
                    browsers::mark_offered(self); // from the extension
                    // (the same link again at once: a second copy of the extension in the browser,
                    // the one loaded by hand and the store's)
                    let now = std::time::Instant::now();
                    let last = self.from_browser.replace(Some((url.clone(), now)));
                    if last.is_some_and(|(last, at)| last == url && now - at < std::time::Duration::from_secs(3)) {
                        return;
                    }
                }
                let directory = directory.filter(|d| d.is_absolute());
                let req = AddRequest { url, filename, directory, referrer, cookies, user_agent, ..Default::default() };
                // (a download waiting for a fresh address takes it: IDM's "Refresh download address")
                if !refresh::take(self, &req) {
                    self.handle_download(req, silent);
                }
            }
            Message::Video { url, referrer, cookies, user_agent, silent, choice } => { // (add/video.rs)
                // (a quality picked in the browser's menu: its dialog opens with it)
                let video = Some(turbodm::engine::video::VideoJob { choice: choice.unwrap_or_default(), ..Default::default() });
                self.handle_video(AddRequest { url, referrer, cookies, user_agent, video, ..Default::default() }, silent);
            }
            // the browser's menu needs the helpers (it was told so): the window that gets them
            Message::VideoQualities { .. } => helpers::with_helpers(self, |_| {}),
            Message::VideoPeek { url, referrer, cookies, user_agent } => {
                let headers = turbodm::engine::http::Headers { user_agent, referrer, cookies, extra: vec![] };
                turbodm::engine::video::look_ahead(&self.manager.shared, &self.rt, url, headers);
            }
        }
    }
}

/// Start the interface. `messages` go to the already-running TurboDM if there is one.
pub fn run(messages: Vec<Message>, background: bool) -> i32 {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all()
        .build().expect("runtime");
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
    std::thread::spawn(|| {
        if let Err(err) = turbodm::register::register() {
            eprintln!("turbodm: connecting the browser extension: {err}");
        }
    });
    // SAFETY: once, on the UI thread, before any window
    unsafe {
        // (OLE: COM as the folder picker and the shell want it, and dragging files out)
        let _ = windows::Win32::System::Ole::OleInitialize(None);
        let classes = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                                             dwICC: ICC_WIN95_CLASSES | ICC_STANDARD_CLASSES };
        let _ = InitCommonControlsEx(&classes);
    }
    let (manager, events) = Manager::new(Settings::load(), config::downloads_file(), runtime.handle().clone());
    let (outbox, inbox) = mpsc::channel();
    let _ = OUTBOX.set(Mutex::new(outbox));
    INBOX.with(|i| { let _ = i.set(inbox); });
    let app = Rc::new(App {
        manager: manager.clone(),
        rt: runtime.handle().clone(),
        main: main::MainWindow::new(),
        progress: RefCell::default(),
        on_done: RefCell::default(),
        refreshing: RefCell::default(),
        offered_refresh: RefCell::default(),
        from_browser: RefCell::default(),
        update: RefCell::default(),
        when_done: RefCell::default(),
        background,
    });
    MAIN_WINDOW.store(app.main.window.hwnd().0 as isize, Ordering::SeqCst);
    APP.with(|a| { let _ = a.set(app.clone()); });

    let (to_ui, from_ipc) = async_channel::unbounded::<Message>();
    runtime.spawn(ipc::server::serve(listener, to_ui, app.manager.shared.clone()));
    runtime.spawn(async move {
        while let Ok(message) = from_ipc.recv().await {
            post(move |app| app.handle_message(message));
        }
    });
    runtime.spawn(async move {
        while let Ok(snap) = events.recv().await {
            post(move |app| finish::on_status(app, snap));
        }
    });
    runtime.spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            post(|app| app.quit());
        }
    });
    for message in messages {
        app.handle_message(message);
    }
    main::setup(&app);
    tray::start(&app);
    queue::start(&app);
    update::start(&app);
    helpers::start(&app);
    if background {
        browsers::mark_offered(&app); // the browser started us: its extension works
    } else {
        app.show();
        browsers::offer(&app);
    }
    main::message_loop(&app);
    theme::stop();
    manager.shutdown();
    drop(app);
    runtime.shutdown_timeout(std::time::Duration::from_secs(3));
    0
}
