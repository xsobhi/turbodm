//! The Add dialog for a web page with a video, like IDM's "Download this video": the video's
//! title and the qualities it comes in (found by yt-dlp) instead of the file's size. Starting
//! one (and FFmpeg for the qualities that need it) is start.rs.

use super::super::wnd::set_text;
use super::super::{browsers, progress, App};
use super::start::{hint, with_ext};
use super::{link, start, to_end, Form};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use turbodm::engine::http::Headers;
use turbodm::engine::video::{self, tools, Choice, VideoInfo};
use turbodm::engine::AddRequest;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};

/// "Download video": the Add dialog for a web page's video (for the menus; the browser's
/// "Download video with TurboDM" comes through `App::handle_video`). An empty `url`: the link
/// on the clipboard, if there is one.
#[allow(dead_code)] // until the main window's menu has "Download video…" (src/win/main/menu.rs)
pub fn add_video(app: &Rc<App>, url: String) {
    super::open_form(app, AddRequest { url, ..Default::default() }, true);
}

impl App {
    /// A video on a web page, from the browser: its qualities in the Add dialog, or (no dialog)
    /// straight to the list in the quality picked last, once the page is looked at.
    pub fn handle_video(self: &Rc<Self>, req: AddRequest, silent: bool) {
        if req.user_agent.is_some() {
            browsers::mark_offered(self); // from the extension
        }
        if !silent && self.manager.settings().show_add_dialog {
            super::open_form(self, req, true);
        } else {
            let id = self.manager.add(AddRequest { start: true, video: Some(Default::default()), ..req });
            if self.manager.settings().show_progress_window {
                progress::open(self, &id);
            }
        }
    }
}

pub type Probed = Arc<Mutex<Option<(u64, Result<VideoInfo, String>)>>>;

/// The dialog's video, when its address is one.
pub struct State {
    pub wanted: bool, // opened for a video: always look for one
    not_found: RefCell<String>, // this address has no video: it's a file after all
    pub(super) probed: Probed,  // yt-dlp's answer, from the network thread
    pub found: RefCell<Option<VideoInfo>>,
    picked: Cell<usize>,
    pub quality: Cell<HWND>, // the qualities' drop-down (made for each video)
    pub title: Cell<HWND>,
    pub(super) ffmpeg: Arc<start::Ffmpeg>,
    pub(super) asking: Cell<Option<bool>>, // FFmpeg is asked for, before Start (true) or Download later
}

impl State {
    pub fn new(wanted: bool) -> Self {
        State { wanted, not_found: RefCell::default(), probed: Probed::default(), found: RefCell::default(),
                picked: Cell::new(0), quality: Cell::default(), title: Cell::default(),
                ffmpeg: Arc::default(), asking: Cell::new(None) }
    }

    /// yt-dlp found no video at this address.
    pub fn not_here(&self, url: &str) -> bool {
        *self.not_found.borrow() == url
    }

    pub(super) fn choice(&self) -> Option<Choice> {
        self.found.borrow().as_ref().and_then(|v| v.choices.get(self.picked.get()).cloned())
    }
}

/// Look for a video at `url` (yt-dlp, fetched first if needed).
pub fn look(app: &Rc<App>, form: &Rc<Form>, url: &str) {
    form.window.set_title("Download video");
    let getting = tools::yt_dlp_now().is_none();
    set_text(form.info.get(), if getting { "Getting yt-dlp, which finds videos on pages (once)…" } else { "Looking for the video…" });
    let req = &form.req;
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(), cookies: req.cookies.clone() };
    let (client, probed, number) = (app.manager.shared.client(), form.video.probed.clone(), form.probe_id.get());
    let (url, timeout) = (url.to_string(), app.manager.settings().timeout_secs);
    app.rt.spawn(async move {
        let answer = video::probe(&client, &url, &headers, timeout).await;
        *probed.lock().unwrap() = Some((number, answer));
    });
}

/// Back to a file (the address changed): the video's rows go.
pub fn leave(form: &Form) {
    start::stop_asking(form);
    if form.video.found.borrow_mut().take().is_some() {
        // SAFETY: our own control, made for that video
        unsafe { let _ = DestroyWindow(form.video.quality.replace(HWND::default())); }
        form.layout();
    }
    if !form.video.wanted {
        form.window.set_title("Download file info");
    }
}

/// What yt-dlp found, and FFmpeg's download (every 150 ms, from the dialog's timer).
pub fn watch(app: &Rc<App>, form: &Rc<Form>) {
    let answer = form.video.probed.lock().unwrap().take();
    match answer {
        Some((number, _)) if number != form.probe_id.get() => {} // the address changed meanwhile
        Some((_, Ok(found))) => show(app, form, found),
        // (a video site's page, or one opened for its video: why not, rather than the page itself)
        Some((_, Err(err))) if form.video.wanted || video::is_video_site(&form.url()) => {
            set_text(form.info.get(), &format!("Couldn't get the video: {err}"));
        }
        Some((_, Err(_))) => {
            // another page without a video: downloaded as it is, like any file
            *form.video.not_found.borrow_mut() = form.url();
            link::check(app, form);
        }
        None => {}
    }
    start::watch(app, form);
}

/// "Caminandes 2  ·  2:26  ·  YouTube"
fn title_line(found: &VideoInfo) -> String {
    let mut parts = vec![found.title.clone()];
    if let Some(d) = found.duration.map(|d| d.round() as u64) {
        parts.push(if d >= 3600 { format!("{}:{:02}:{:02}", d / 3600, d / 60 % 60, d % 60) }
                   else { format!("{}:{:02}", d / 60, d % 60) });
    }
    if !found.site.is_empty() {
        parts.push(found.site.clone());
    }
    parts.join("  ·  ")
}

/// The video found: its title and qualities (the one picked last time, or the usual one).
fn show(app: &Rc<App>, form: &Rc<Form>, found: VideoInfo) {
    let ffmpeg = tools::ffmpeg().is_some();
    let picked = found.choice_like(&app.manager.settings().video_quality).unwrap_or_else(|| found.default_choice(ffmpeg));
    let items: Vec<String> = found.choices.iter().map(|c| {
        let needs = if c.merge && !ffmpeg { "  (needs FFmpeg)" } else { "" };
        format!("{}{needs}", c.summary())
    }).collect();
    let items: Vec<&str> = items.iter().map(String::as_str).collect();
    let (f, a) = (Rc::downgrade(form), Rc::downgrade(app));
    let quality = form.window.choice(&items, picked, move |i| if let (Some(f), Some(a)) = (f.upgrade(), a.upgrade()) {
        pick(&a, &f, i);
    });
    // SAFETY: our own controls: the new drop-down comes after the path's field when tabbing
    unsafe { let _ = SetWindowPos(quality, Some(form.browse.get()), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); }
    // SAFETY: the last video's drop-down, ours
    unsafe { let _ = DestroyWindow(form.video.quality.replace(quality)); }
    set_text(form.video.title.get(), &title_line(&found));
    let needs_ffmpeg = !ffmpeg && found.choices.iter().any(|c| c.merge);
    *form.video.found.borrow_mut() = Some(found);
    form.layout();
    pick(app, form, picked);
    if needs_ffmpeg {
        start::size_ffmpeg(app, form); // to say how big it is when asking
    }
}

/// A quality was picked: its file name (extension) and what it takes.
fn pick(app: &Rc<App>, form: &Rc<Form>, index: usize) {
    form.video.picked.set(index);
    start::stop_asking(form);
    let Some(choice) = form.video.choice() else { return };
    let title = form.video.found.borrow().as_ref().map(|v| v.title.clone()).unwrap_or_default();
    let (folder, name) = form.target(app);
    let (folder, name) = match (form.chosen.get(), name) {
        (true, Some(name)) => (folder, with_ext(&name, &choice.ext)), // the user's name and folder
        _ => {
            let name = video::filename(&title, &choice.ext);
            (app.manager.settings().folder_for(&name), name)
        }
    };
    set_text(form.path.get(), &folder.join(name).display().to_string());
    to_end(form.path.get());
    form.update_remember();
    set_text(form.info.get(), &hint(form, &choice));
}
