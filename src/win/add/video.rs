//! The Add dialog for a web page with a video, like IDM's "Download this video": the video's
//! title, and instead of the file's size the ways it can be downloaded (found by yt-dlp): each
//! height as MP4, WebM or MKV (the Format beside the Quality), or the sound alone as MP3, M4A or
//! Opus. Starting it is start.rs; yt-dlp and FFmpeg are got first if they're missing (helpers.rs).

use super::super::wnd::{enable, set_text};
use super::super::{browsers, helpers, progress, App};
use super::start::{hint, with_ext};
use super::{link, to_end, Form};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use turbodm::engine::http::Headers;
use turbodm::engine::video::{self, tools, Container, Plan, VideoInfo, Want};
use turbodm::engine::AddRequest;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};

/// "Download video": the Add dialog for a web page's video (Tasks → Download video…; the
/// browser's comes through `App::handle_video`). An empty `url`: the link on the clipboard.
pub fn add_video(app: &Rc<App>, url: String) {
    helpers::with_helpers(app, move |app| super::open_form(app, AddRequest { url, ..Default::default() }, true));
}

impl App {
    /// A video on a web page, from the browser: its qualities in the Add dialog, or (no dialog)
    /// straight to the list in the quality picked last, once the page is looked at.
    pub fn handle_video(self: &Rc<Self>, req: AddRequest, silent: bool) {
        if req.user_agent.is_some() {
            browsers::mark_offered(self); // from the extension
        }
        helpers::with_helpers(self, move |app| {
            if !silent && app.manager.settings().show_add_dialog {
                super::open_form(app, req, true);
            } else {
                let id = app.manager.add(AddRequest { start: true, video: Some(Default::default()), ..req });
                if app.manager.settings().show_progress_window {
                    progress::open(app, &id);
                }
            }
        });
    }
}

pub type Probed = Arc<Mutex<Option<(u64, Result<VideoInfo, String>)>>>;

/// The dialog's video, when its address is one.
pub struct State {
    pub wanted: bool, // opened for a video: always look for one
    not_found: RefCell<String>, // this address has no video: it's a file after all
    pub(super) probed: Probed,  // yt-dlp's answer, from the network thread
    pub found: RefCell<Option<VideoInfo>>,
    plans: RefCell<Vec<Plan>>, // the Quality list's, for the format picked
    picked: Cell<usize>,
    containers: RefCell<Vec<Container>>, // the Format list's
    pub quality: Cell<HWND>, // the drop-downs (made for each video)
    pub format: Cell<HWND>,
    pub title: Cell<HWND>,
}

impl State {
    pub fn new(wanted: bool) -> Self {
        State { wanted, not_found: RefCell::default(), probed: Probed::default(), found: RefCell::default(),
                plans: RefCell::default(), picked: Cell::new(0), containers: RefCell::default(),
                quality: Cell::default(), format: Cell::default(), title: Cell::default() }
    }

    /// yt-dlp found no video at this address.
    pub fn not_here(&self, url: &str) -> bool {
        *self.not_found.borrow() == url
    }

    pub(super) fn plan(&self) -> Option<Plan> {
        self.plans.borrow().get(self.picked.get()).cloned()
    }
}

/// Look for a video at `url` (once yt-dlp and FFmpeg are here).
pub fn look(app: &Rc<App>, form: &Rc<Form>, url: &str) {
    form.window.set_title("Download video");
    if !helpers::ready() {
        set_text(form.info.get(), "Videos need two free helpers, yt-dlp and FFmpeg: get them first");
        let f = Rc::downgrade(form);
        helpers::with_helpers(app, move |app| if let Some(form) = f.upgrade() { link::check(app, &form) });
        return;
    }
    set_text(form.info.get(), "Looking for the video…");
    let req = &form.req;
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(), cookies: req.cookies.clone(), extra: vec![] };
    let (client, probed, number) = (app.manager.shared.client(), form.video.probed.clone(), form.probe_id.get());
    let (url, timeout) = (url.to_string(), app.manager.settings().timeout_secs);
    app.rt.spawn(async move {
        let answer = video::probe(&client, &url, &headers, timeout).await;
        *probed.lock().unwrap() = Some((number, answer));
    });
}

/// `hwnd` gone, `new` (or nothing) in its place.
fn replace(cell: &Cell<HWND>, new: HWND) {
    // SAFETY: our own control, made for the last video
    unsafe { let _ = DestroyWindow(cell.replace(new)); }
}

/// Back to a file (the address changed): the video's rows go.
pub fn leave(form: &Form) {
    if form.video.found.borrow_mut().take().is_some() {
        replace(&form.video.quality, HWND::default());
        replace(&form.video.format, HWND::default());
        form.video.plans.borrow_mut().clear();
        form.layout();
    }
    if !form.video.wanted {
        form.window.set_title("Download file info");
    }
}

/// What yt-dlp found (every 150 ms, from the dialog's timer).
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

/// The video found: its title, formats and qualities (what was picked last time, or the usual).
fn show(app: &Rc<App>, form: &Rc<Form>, found: VideoInfo) {
    let settings = app.manager.settings();
    let first = found.first(&settings.video_quality, &settings.video_format, tools::ffmpeg().is_some());
    let containers = found.containers();
    let container = match first.want {
        Want::Video(_, container) => container,
        Want::Sound(_) => containers.iter().copied().find(|c| c.name() == settings.video_format).unwrap_or(Container::Mp4),
    };
    set_text(form.video.title.get(), &title_line(&found));
    *form.video.found.borrow_mut() = Some(found);
    *form.video.containers.borrow_mut() = containers.clone();
    let names: Vec<&str> = containers.iter().map(|c| c.name()).collect();
    let selected = containers.iter().position(|c| *c == container).unwrap_or(0);
    let (f, a) = (Rc::downgrade(form), Rc::downgrade(app));
    let format = form.window.choice(&names, selected, move |i| if let (Some(f), Some(a)) = (f.upgrade(), a.upgrade()) {
        let container = f.video.containers.borrow()[i];
        fill(&a, &f, container, f.video.plan().map(|p| p.want));
    });
    replace(&form.video.format, format);
    fill(app, form, container, Some(first.want));
    form.layout();
}

/// The Quality list for `container`, with the closest to `want` picked.
fn fill(app: &Rc<App>, form: &Rc<Form>, container: Container, want: Option<Want>) {
    let plans = form.video.found.borrow().as_ref().map(|found| found.plans(container)).unwrap_or_default();
    let index = match want {
        Some(Want::Sound(sound)) => plans.iter().position(|p| p.want == Want::Sound(sound)),
        // that height, else the best below it
        Some(Want::Video(height, _)) => plans.iter().position(|p| matches!(p.want, Want::Video(h, _)
                                                                if h.unwrap_or(0) <= height.unwrap_or(u32::MAX))),
        None => None,
    }.unwrap_or(0);
    let ffmpeg = tools::ffmpeg().is_some();
    let items: Vec<String> = plans.iter().map(|p| {
        let needs = if p.needs_ffmpeg() && !ffmpeg { "  (needs FFmpeg)" } else { "" };
        format!("{}{needs}", p.summary())
    }).collect();
    let items: Vec<&str> = items.iter().map(String::as_str).collect();
    let (f, a) = (Rc::downgrade(form), Rc::downgrade(app));
    let quality = form.window.choice(&items, index, move |i| if let (Some(f), Some(a)) = (f.upgrade(), a.upgrade()) {
        pick(&a, &f, i);
    });
    // SAFETY: our own controls: tabbing goes from the path's field to the quality, then the format
    unsafe {
        let _ = SetWindowPos(quality, Some(form.browse.get()), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let _ = SetWindowPos(form.video.format.get(), Some(quality), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    replace(&form.video.quality, quality);
    *form.video.plans.borrow_mut() = plans;
    form.layout();
    pick(app, form, index);
}

/// One was picked: its file name (extension) and what it takes.
fn pick(app: &Rc<App>, form: &Rc<Form>, index: usize) {
    form.video.picked.set(index);
    let Some(plan) = form.video.plan() else { return };
    // (the format is a video's: the sound alone says its own)
    let video = matches!(plan.want, Want::Video(..));
    enable(form.video.format.get(), video && form.video.containers.borrow().len() > 1);
    let title = form.video.found.borrow().as_ref().map(|v| v.title.clone()).unwrap_or_default();
    let (folder, name) = form.target(app);
    let (folder, name) = match (form.chosen.get(), name) {
        (true, Some(name)) => (folder, with_ext(&name, &plan.ext)), // the user's name and folder
        _ => {
            let name = video::filename(&title, &plan.ext);
            (app.manager.settings().folder_for(&name), name)
        }
    };
    set_text(form.path.get(), &folder.join(name).display().to_string());
    to_end(form.path.get());
    form.update_remember();
    set_text(form.info.get(), &hint(&plan));
}
