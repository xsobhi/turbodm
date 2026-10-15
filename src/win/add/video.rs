//! The Add dialog for a web page with a video, like IDM's "Download this video": the video's
//! title and the qualities it comes in (found by yt-dlp) instead of the file's size. The higher
//! qualities come as video and sound apart, joined by FFmpeg: the dialog asks before fetching it
//! (~85 MB, once), right where Start was.

use super::super::wnd::{enable, set_text};
use super::super::App;
use super::{done, link, remember_folder, to_end, Form};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use turbodm::engine::http::Headers;
use turbodm::engine::video::{self, quality_of, tools, Choice, VideoInfo, VideoJob};
use turbodm::engine::AddRequest;
use turbodm::util::human_size;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{DestroyWindow, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};

/// "Download video": the Add dialog for a web page's video (for the menus; the browser's
/// "Download video with TurboDM" comes through `App::handle_video`). An empty `url`: the link
/// on the clipboard, if there is one.
#[allow(dead_code)] // until the main window's menu has "Download video…" (src/win/main/menu.rs)
pub fn add_video(app: &Rc<App>, url: String) {
    open(app, AddRequest { url, ..Default::default() });
}

/// The dialog for a video, with the browser's request (its cookies, referrer, user agent).
pub fn open(app: &Rc<App>, req: AddRequest) {
    super::open_form(app, req, true);
}

pub type Probed = Arc<Mutex<Option<(u64, Result<VideoInfo, String>)>>>;

/// The dialog's video, when its address is one.
pub struct State {
    pub wanted: bool, // opened for a video: always look for one
    not_found: RefCell<String>, // this address has no video: it's a file after all
    probed: Probed,             // yt-dlp's answer, from the network thread
    pub found: RefCell<Option<VideoInfo>>,
    picked: Cell<usize>,
    pub quality: Cell<HWND>, // the qualities' drop-down (made for each video)
    pub title: Cell<HWND>,
    ffmpeg: Arc<Ffmpeg>,
    asking: Cell<Option<bool>>, // FFmpeg is asked for, before Start (true) or Download later
}

/// FFmpeg's size (to ask), and its download.
#[derive(Default)]
struct Ffmpeg {
    size: Mutex<Option<u64>>,
    sized: AtomicBool, // its size was asked for
    fetch: tools::Fetch,
    busy: AtomicBool,
    result: Mutex<Option<Result<(), String>>>,
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

    fn choice(&self) -> Option<Choice> {
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
    stop_asking(form);
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
        Some((_, Err(err))) if form.video.wanted => set_text(form.info.get(), &format!("Couldn't find a video: {err}")),
        Some((_, Err(_))) => {
            // a page without a video: downloaded as it is, like any file
            *form.video.not_found.borrow_mut() = form.url();
            link::check(app, form);
        }
        None => {}
    }
    let ffmpeg = &form.video.ffmpeg;
    let result = ffmpeg.result.lock().unwrap().take();
    match result {
        Some(Ok(())) => {
            ffmpeg.busy.store(false, Ordering::Relaxed);
            let asked = form.video.asking.get();
            stop_asking(form);
            if let Some(start_now) = asked {
                submit(app, form, start_now); // what was asked before FFmpeg was needed
            }
        }
        Some(Err(err)) => {
            ffmpeg.busy.store(false, Ordering::Relaxed);
            stop_asking(form);
            set_text(form.info.get(), &format!("Couldn't get FFmpeg: {err}"));
        }
        None if ffmpeg.busy.load(Ordering::Relaxed) => {
            let (done, total) = ffmpeg.fetch.get();
            let of = total.map(|t| format!(" of {}", human_size(Some(t)))).unwrap_or_default();
            set_text(form.info.get(), &format!("Getting FFmpeg… {}{of}", human_size(Some(done))));
        }
        None => {}
    }
    // FFmpeg's size arrived: say it
    if form.video.asking.get().is_some() && !ffmpeg.busy.load(Ordering::Relaxed) {
        set_text(form.info.get(), &ask_text(form));
    }
}

/// "Caminandes 2  ·  2:26  ·  Youtube"
fn title_line(found: &VideoInfo) -> String {
    let mut parts = vec![found.title.clone()];
    if let Some(d) = found.duration.map(|d| d.round() as u64) {
        parts.push(if d >= 3600 { format!("{}:{:02}:{:02}", d / 3600, d / 60 % 60, d % 60) }
                   else { format!("{}:{:02}", d / 60, d % 60) });
    }
    if !found.site.is_empty() && found.site != "Generic" {
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
    let sizing = &form.video.ffmpeg;
    if needs_ffmpeg && tools::CAN_FETCH_FFMPEG && !sizing.sized.swap(true, Ordering::Relaxed) {
        let (client, ffmpeg) = (app.manager.shared.client(), sizing.clone());
        app.rt.spawn(async move {
            *ffmpeg.size.lock().unwrap() = tools::ffmpeg_size(&client).await;
        });
    }
}

/// `name` with `ext` instead of its media extension ("a.mp4" → "a.m4a"; "a.b" → "a.b.m4a").
fn with_ext(name: &str, ext: &str) -> String {
    const MEDIA: [&str; 11] = ["mp4", "m4a", "webm", "mkv", "mp3", "opus", "ogg", "flv", "3gp", "mov", "aac"];
    let stem = match name.rsplit_once('.') {
        Some((stem, e)) if MEDIA.contains(&e.to_ascii_lowercase().as_str()) => stem,
        _ => name,
    };
    format!("{stem}.{ext}")
}

/// A quality was picked: its file name (extension) and what it takes.
fn pick(app: &Rc<App>, form: &Rc<Form>, index: usize) {
    form.video.picked.set(index);
    stop_asking(form);
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

/// Under the title: what the quality takes.
fn hint(form: &Form, choice: &Choice) -> String {
    let ffmpeg = tools::ffmpeg().is_some();
    let others_need_it = !ffmpeg && form.video.found.borrow().as_ref().is_some_and(|v| v.choices.iter().any(|c| c.merge));
    let size = choice.size.map(|s| format!("about {}", human_size(Some(s)))).unwrap_or_else(|| "not known yet".into());
    if choice.merge && !ffmpeg {
        if tools::CAN_FETCH_FFMPEG { format!("Size: {size}  ·  needs FFmpeg to join its video and sound") }
        else { tools::NO_FFMPEG.into() }
    } else if others_need_it && !choice.audio_only() {
        format!("Size: {size}  ·  no FFmpeg needed (the higher qualities need it)")
    } else {
        format!("Size: {size}")
    }
}

/// "FFmpeg is needed for this quality (84.3 MB): download it?"
fn ask_text(form: &Form) -> String {
    let size = form.video.ffmpeg.size.lock().unwrap().map(|s| human_size(Some(s))).unwrap_or_else(|| "about 85 MB".into());
    format!("FFmpeg is needed for this quality ({size}): download it?")
}

/// The question about FFmpeg goes (another quality, or it's answered).
fn stop_asking(form: &Form) {
    if form.video.asking.take().is_some() {
        let [_, later, start] = form.buttons.get();
        set_text(start, "Start download");
        enable(later, true);
        enable(start, true);
        form.layout(); // the button's width, for its words
    }
}

/// Start (or Download later) for a video: FFmpeg first if the quality needs it (asked here,
/// in the dialog: the info line asks, Start turns into "Download FFmpeg").
pub fn submit(app: &Rc<App>, form: &Rc<Form>, start_now: bool) {
    let ffmpeg = &form.video.ffmpeg;
    if ffmpeg.busy.load(Ordering::Relaxed) {
        return;
    }
    let found = form.video.found.borrow().clone();
    let (Some(found), Some(choice)) = (found, form.video.choice()) else {
        let looking = form.video.probed.lock().unwrap().is_none();
        set_text(form.info.get(), if looking { "Still looking for the video…" } else { "There's no video to download here" });
        return;
    };
    if choice.merge && tools::ffmpeg().is_none() {
        if !tools::CAN_FETCH_FFMPEG {
            set_text(form.info.get(), tools::NO_FFMPEG);
            return;
        }
        let [_, later, start] = form.buttons.get();
        match form.video.asking.get() {
            None => {
                // first: ask, right here
                form.video.asking.set(Some(start_now));
                set_text(start, "Download FFmpeg");
                enable(later, false);
                form.layout();
                set_text(form.info.get(), &ask_text(form));
            }
            Some(_) => {
                // yes: fetch it, then do what was asked
                ffmpeg.busy.store(true, Ordering::Relaxed);
                enable(start, false);
                set_text(form.info.get(), "Getting FFmpeg…");
                let (client, ffmpeg) = (app.manager.shared.client(), ffmpeg.clone());
                app.rt.spawn(async move {
                    let result = tools::fetch_ffmpeg(&client, &ffmpeg.fetch).await;
                    *ffmpeg.result.lock().unwrap() = Some(result);
                });
            }
        }
        return;
    }
    let (directory, name) = form.target(app);
    let name = name.map(|n| with_ext(&n, &choice.ext)).unwrap_or_else(|| video::filename(&found.title, &choice.ext));
    remember_folder(app, form, &name, &directory);
    let quality = quality_of(&choice);
    let mut settings = app.manager.settings();
    if settings.video_quality != quality {
        settings.video_quality = quality; // offered first next time
        app.manager.update_settings(settings);
    }
    let id = app.manager.add(AddRequest { url: found.url.clone(), filename: Some(name), directory: Some(directory),
                                          start: start_now, video: Some(VideoJob::new(&found, &choice)),
                                          ..form.req.clone() });
    done(app, form, &id, start_now);
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_the_name_changes_the_extension() {
        assert_eq!(super::with_ext("My clip.mp4", "m4a"), "My clip.m4a");
        assert_eq!(super::with_ext("My clip", "mp4"), "My clip.mp4");
        assert_eq!(super::with_ext("v1.2 final", "mp4"), "v1.2 final.mp4");
    }
}
