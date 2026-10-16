//! The two free helpers videos need, yt-dlp (finds a page's video and where its streams are) and
//! FFmpeg (joins video and sound, makes MP3s): asked for once ("Get them" / "Not now"), then
//! downloaded by TurboDM like its update, each in a progress window and out of the list, checked
//! against their releases' checksums and put in place; then the video asked for goes on. After
//! that they update themselves quietly (yt-dlp every few days, FFmpeg monthly).

use super::wnd::{self, set_text, Rows, Window, LINE};
use super::{dialogs, post, progress, App};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use turbodm::engine::video::tools::{self, Helper};
use turbodm::engine::{AddRequest, Snapshot, Status};
use turbodm::util::human_size;
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

type Then = Box<dyn FnOnce(&Rc<App>)>;

thread_local! {
    static WAITING: RefCell<Vec<Then>> = RefCell::default(); // videos asked for meanwhile
    static ASKING: RefCell<Option<Rc<Window>>> = RefCell::default();
    static FETCHING: RefCell<Option<Fetching>> = RefCell::default();
}

/// The helpers being downloaded: how many are still to come, and what went wrong.
struct Fetching {
    left: usize,
    failed: Option<String>,
}

/// Both are here (always on Linux: the system's are used).
pub fn ready() -> bool {
    tools::missing().is_empty()
}

/// Do `then` once the helpers are here: now if they are, else after asking for them.
pub fn with_helpers(app: &Rc<App>, then: impl FnOnce(&Rc<App>) + 'static) {
    if ready() {
        return then(app);
    }
    WAITING.with(|w| w.borrow_mut().push(Box::new(then)));
    if FETCHING.with(|f| f.borrow().is_none()) {
        ask(app, true);
    }
}

/// At start: ask for them unless the user said "Not now" once (not when the browser started
/// TurboDM in the background, nor at the very first start, which shows the browser guide);
/// and keep them up to date.
pub fn start(app: &Rc<App>) {
    let settings = app.manager.settings();
    if !ready() && !settings.video_helpers_declined && !app.background && settings.browsers_offered {
        ask(app, false);
    }
    let client = app.manager.shared.client();
    app.rt.spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            if tools::missing().is_empty() {
                let _ = tools::update(&client, false).await; // (yt-dlp: when it's due, every few days)
                let _ = tools::update_ffmpeg(&client).await; // (monthly)
            }
            tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        }
    });
}

/// "To download videos, TurboDM needs two free helpers, yt-dlp and FFmpeg (about 103 MB)."
fn asking_text(missing: &[&str], size: Option<u64>) -> String {
    let size = size.map_or_else(|| "about 105 MB".to_string(), |s| format!("about {}", human_size(Some(s))));
    let what = match missing {
        [one] => format!("a free helper, {one} ({size})"),
        _ => format!("two free helpers, yt-dlp and FFmpeg ({size})"),
    };
    format!("To download videos, TurboDM needs {what}: yt-dlp finds a page's video, FFmpeg joins its video \
             and sound. They're downloaded once and keep themselves up to date. Get them now?")
}

/// The window asking for them. `for_video`: the user just asked for a video ("Not now" is then
/// for this once; at start it's remembered).
fn ask(app: &Rc<App>, for_video: bool) {
    if let Some(open) = ASKING.with(|a| a.borrow().clone()) {
        return open.show();
    }
    let missing = tools::missing();
    let window = Window::dialog("Video downloads", None, (460, 160), false);
    let heading = window.heading("Get the helpers for videos?");
    let text = window.note(&asking_text(&missing, None));
    let (a, w) = (app.clone(), window.clone());
    let later = window.button("Not now", move || {
        if !for_video {
            let mut s = a.manager.settings();
            s.video_helpers_declined = true;
            a.manager.update_settings(s);
        }
        WAITING.with(|w| w.borrow_mut().clear());
        ASKING.with(|a| a.borrow_mut().take());
        w.destroy();
    });
    let (a, w) = (app.clone(), window.clone());
    let get = window.default_button("Get them", move || {
        get_them(&a);
        w.destroy();
    });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.close());
    window.on_close(|| {
        ASKING.with(|a| a.borrow_mut().take());
        true
    });
    let mut rows = Rows::new(16, 14, 0, 428);
    rows.full(heading, 24);
    rows.full(text, 4 * LINE);
    let bottom = rows.buttons(&[later, get]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
    ASKING.with(|a| *a.borrow_mut() = Some(window.clone()));
    // their real size, once the releases say
    let (client, text) = (app.manager.shared.client(), text.0 as isize); // (a window handle, sent as a number)
    app.rt.spawn(async move {
        if let Ok(helpers) = tools::helpers(&client).await {
            let size = helpers.iter().map(|h| h.size).sum::<Option<u64>>();
            post(move |_| if ASKING.with(|a| a.borrow().is_some()) {
                set_text(windows::Win32::Foundation::HWND(text as *mut _), &asking_text(&missing, size));
            });
        }
    });
}

/// "Get them": where they are now, then each downloaded in a progress window.
fn get_them(app: &Rc<App>) {
    ASKING.with(|a| a.borrow_mut().take());
    FETCHING.with(|f| *f.borrow_mut() = Some(Fetching { left: 0, failed: None }));
    let client = app.manager.shared.client();
    app.rt.spawn(async move {
        let found = tools::helpers(&client).await;
        post(move |app| match found {
            Ok(helpers) => download(app, helpers),
            Err(err) => finished(app, Some(format!("Couldn't find them: {err}"))),
        });
    });
}

fn download(app: &Rc<App>, helpers: Vec<Helper>) {
    FETCHING.with(|f| if let Some(fetching) = f.borrow_mut().as_mut() { fetching.left = helpers.len() });
    if helpers.is_empty() {
        return finished(app, None);
    }
    for helper in helpers {
        let id = app.manager.add_unlisted(AddRequest { url: helper.url.clone(), filename: Some(helper.file.clone()),
                                                       directory: Some(tools::incoming()), ..Default::default() });
        app.when_done.borrow_mut().insert(id.clone(), Box::new(move |app, snap| arrived(app, snap, helper)));
        progress::open(app, &id);
    }
}

/// One is downloaded (or failed): checked and put in place, off the UI thread.
fn arrived(app: &Rc<App>, snap: &Snapshot, helper: Helper) {
    progress::close(app, &snap.id);
    app.on_done.borrow_mut().remove(&snap.id);
    app.manager.remove(&snap.id, snap.status != Status::Completed); // (out of sight anyway)
    if snap.status != Status::Completed {
        let why = snap.error.clone().unwrap_or_default();
        return installed(app, Err(format!("Couldn't download {}: {why}", helper.name)));
    }
    let path = snap.path.clone();
    app.rt.spawn_blocking(move || {
        let result = tools::install(&helper, &path);
        post(move |app| installed(app, result));
    });
}

fn installed(app: &Rc<App>, result: Result<(), String>) {
    let all_in = FETCHING.with(|f| {
        let mut fetching = f.borrow_mut();
        let state = fetching.as_mut()?;
        state.left = state.left.saturating_sub(1);
        if let Err(err) = result {
            state.failed.get_or_insert(err);
        }
        (state.left == 0).then(|| state.failed.take())
    });
    if let Some(failed) = all_in {
        finished(app, failed);
    }
}

/// All of them are in (or one failed): what was asked for goes on.
fn finished(app: &Rc<App>, failed: Option<String>) {
    FETCHING.with(|f| f.borrow_mut().take());
    let waiting: Vec<Then> = WAITING.with(|w| w.borrow_mut().drain(..).collect());
    match failed {
        None if ready() => waiting.into_iter().for_each(|then| then(app)),
        failed => dialogs::message(app.main.window.hwnd(), "Couldn't get the video helpers",
                                   &failed.unwrap_or_else(|| "They didn't arrive: try again".into()), true),
    }
}
