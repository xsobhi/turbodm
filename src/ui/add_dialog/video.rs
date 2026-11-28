//! The Add dialog for a web page with a video, as on Windows (src/win/add/video.rs) and like
//! IDM's "Download this video": the video's title, and instead of the file's size the ways it
//! can be downloaded (found by yt-dlp): each height as MP4, WebM or MKV (the Format beside the
//! Quality), or the sound alone as MP3, M4A or Opus; the one picked in the browser's menu, if it
//! came from there. The quality picked starts downloading behind the dialog (like a file's), and
//! Start moves it where it was asked to go.

use super::{done, link, remember_folder, Form};
use crate::ui::{frame, Ctx};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use turbodm::engine::http::Headers;
use turbodm::engine::manager::Confirm;
use turbodm::engine::video::{self, remembered, tools, Container, Plan, Source, VideoInfo, VideoJob, Want};
use turbodm::engine::AddRequest;
use turbodm::text::{video_hint, video_title_line, with_ext};

pub type Probed = Arc<Mutex<Option<(u64, Result<VideoInfo, String>)>>>;

/// The dialog's video, when its address is one.
pub struct State {
    pub wanted: bool, // opened for a video: always look for one
    not_found: RefCell<String>, // this address has no video: it's a file after all
    probed: Probed,             // yt-dlp's answer
    pub found: RefCell<Option<VideoInfo>>,
    plans: RefCell<Vec<Plan>>, // the Quality list's, for the format picked
    containers: RefCell<Vec<Container>>, // the Format list's
    quality: gtk::DropDown,
    format: gtk::DropDown,
    filling: Cell<bool>, // the lists are being filled in: not picked by the user
}

impl State {
    pub fn new(wanted: bool) -> Self {
        let quality = frame::choice(&[], 0);
        quality.set_width_request(316);
        let format = frame::choice(&[], 0);
        format.set_hexpand(true);
        State { wanted, not_found: RefCell::default(), probed: Probed::default(), found: RefCell::default(),
                plans: RefCell::default(), containers: RefCell::default(), quality, format,
                filling: Cell::new(false) }
    }

    /// yt-dlp found no video at this address.
    pub fn not_here(&self, url: &str) -> bool {
        *self.not_found.borrow() == url
    }

    fn plan(&self) -> Option<Plan> {
        self.plans.borrow().get(self.quality.selected() as usize).cloned()
    }
}

/// [Quality ▾]  Format [MP4 ▾], in the form (shown once a video is found).
pub fn build(ctx: &Rc<Ctx>, form: &Rc<Form>) {
    let state = &form.video;
    form.quality_row.append(&state.quality);
    let name = frame::name("Format");
    name.set_width_request(64);
    form.quality_row.append(&name);
    form.quality_row.append(&state.format);
    let (f, c) = (Rc::downgrade(form), Rc::downgrade(ctx));
    state.quality.connect_selected_notify(move |q| {
        let (Some(f), Some(c)) = (f.upgrade(), c.upgrade()) else { return };
        if !f.video.filling.get() {
            pick(&c, &f, q.selected() as usize);
        }
    });
    let (f, c) = (Rc::downgrade(form), Rc::downgrade(ctx));
    state.format.connect_selected_notify(move |choice| {
        let (Some(f), Some(c)) = (f.upgrade(), c.upgrade()) else { return };
        if f.video.filling.get() {
            return;
        }
        let container = f.video.containers.borrow().get(choice.selected() as usize).copied();
        if let Some(container) = container {
            let want = f.video.plan().map(|p| p.want);
            fill(&c, &f, container, want);
        }
    });
}

/// Look for a video at `url`.
pub fn look(ctx: &Rc<Ctx>, form: &Rc<Form>, url: &str) {
    set_title(form, "Download video");
    form.info.set_text("Looking for the video…");
    let req = &form.req;
    let headers = Headers { user_agent: req.user_agent.clone(), referrer: req.referrer.clone(),
                            cookies: req.cookies.clone(), extra: vec![] };
    let (client, probed, number) = (ctx.manager.shared.client(), form.video.probed.clone(), form.probe_id.get());
    let (url, timeout) = (url.to_string(), ctx.manager.settings().timeout_secs);
    ctx.rt.spawn(async move {
        let answer = video::probe(&client, &url, &headers, timeout).await;
        *probed.lock().unwrap() = Some((number, answer));
    });
}

fn set_title(form: &Form, title: &str) {
    form.window.set_title(Some(title));
    form.title.set_text(title);
}

/// Back to a file (the address changed): the video's rows go.
pub fn leave(form: &Form) {
    if form.video.found.borrow_mut().take().is_some() {
        form.video.plans.borrow_mut().clear();
        show_rows(form, false);
    }
    if !form.video.wanted {
        set_title(form, "Download file info");
    }
}

fn show_rows(form: &Form, on: bool) {
    form.quality_row.set_visible(on);
    form.quality_name.set_visible(on);
    form.video_title.set_visible(on);
}

/// What yt-dlp found (every 150 ms, from the dialog's timer).
pub fn watch(ctx: &Rc<Ctx>, form: &Rc<Form>) {
    let answer = form.video.probed.lock().unwrap().take();
    match answer {
        Some((number, _)) if number != form.probe_id.get() => {} // the address changed meanwhile
        Some((_, Ok(found))) => show(ctx, form, found),
        // (a video site's page, or one opened for its video: why not, rather than the page itself)
        Some((_, Err(err))) if form.video.wanted || video::is_video_site(&form.url()) => {
            form.info.set_text(&format!("Couldn't get the video: {err}"));
        }
        Some((_, Err(_))) => {
            // another page without a video: downloaded as it is, like any file
            *form.video.not_found.borrow_mut() = form.url();
            link::check(ctx, form);
        }
        None => {}
    }
}

/// The video found: its title, formats and qualities (what was picked last time, or the usual).
fn show(ctx: &Rc<Ctx>, form: &Rc<Form>, found: VideoInfo) {
    let settings = ctx.manager.settings();
    // the quality picked in the browser's menu, else the one picked last time
    let choice = form.req.video.as_ref().map(|v| v.choice.as_str()).filter(|c| !c.is_empty() && form.url() == form.req.url);
    let first = choice.and_then(|c| found.chosen(c))
        .unwrap_or_else(|| found.first(&settings.video_quality, &settings.video_format, tools::ffmpeg().is_some()));
    let containers = found.containers();
    let container = match first.want {
        Want::Video(_, container) => container,
        Want::Sound(_) => containers.iter().copied().find(|c| c.name() == settings.video_format).unwrap_or(Container::Mp4),
    };
    form.video_title.set_text(&video_title_line(&found));
    *form.video.found.borrow_mut() = Some(found);
    *form.video.containers.borrow_mut() = containers.clone();
    let names: Vec<&str> = containers.iter().map(|c| c.name()).collect();
    let selected = containers.iter().position(|c| *c == container).unwrap_or(0);
    form.video.filling.set(true);
    form.video.format.set_model(Some(&gtk::StringList::new(&names)));
    form.video.format.set_selected(selected as u32);
    form.video.filling.set(false);
    show_rows(form, true);
    fill(ctx, form, container, Some(first.want));
}

/// The Quality list for `container`, with the closest to `want` picked.
fn fill(ctx: &Rc<Ctx>, form: &Rc<Form>, container: Container, want: Option<Want>) {
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
    *form.video.plans.borrow_mut() = plans;
    form.video.filling.set(true);
    form.video.quality.set_model(Some(&gtk::StringList::new(&items)));
    form.video.quality.set_selected(index as u32);
    form.video.filling.set(false);
    pick(ctx, form, index);
}

/// One was picked: its file name (extension) and what it takes; it downloads meanwhile.
fn pick(ctx: &Rc<Ctx>, form: &Rc<Form>, _index: usize) {
    let Some(plan) = form.video.plan() else { return };
    // (the format is a video's: the sound alone says its own)
    let video = matches!(plan.want, Want::Video(..));
    form.video.format.set_sensitive(video && form.video.containers.borrow().len() > 1);
    let title = form.video.found.borrow().as_ref().map(|v| v.title.clone()).unwrap_or_default();
    let (folder, name) = form.target(ctx);
    let (folder, name) = match (form.chosen.get(), name) {
        (true, Some(name)) => (folder, with_ext(&name, &plan.ext)), // the user's name and folder
        _ => {
            let name = video::filename(&title, &plan.ext);
            (ctx.manager.settings().folder_for(&name), name)
        }
    };
    let chosen = form.chosen.get();
    form.set_path(&folder.join(&name));
    form.chosen.set(chosen);
    form.info.set_text(&video_hint(&plan));
    start_early(ctx, form, &plan, &folder, &name);
}

/// Which download a quality's early one is: the video's page and the choice.
fn key(url: &str, plan: &Plan) -> String {
    format!("{url}#{}", video::choice_id(plan.want))
}

/// The quality picked (`plan`, to go to `folder` as `name`): download it now, unless that's
/// turned off (Options), or it's left to yt-dlp (fragments), or it needs FFmpeg first.
fn start_early(ctx: &Rc<Ctx>, form: &Rc<Form>, plan: &Plan, folder: &Path, name: &str) {
    let Some(found) = form.video.found.borrow().clone() else { return };
    let key = key(&found.url, plan);
    if form.early.borrow().as_ref().is_some_and(|(early, _)| *early == key) {
        return; // (picked again: it's downloading already)
    }
    form.take_early(ctx, None); // another quality's
    let direct = !plan.sources.is_empty() && plan.sources.iter().all(Source::plain);
    if !ctx.manager.settings().predownload || !direct || (plan.needs_ffmpeg() && tools::ffmpeg().is_none()) {
        return;
    }
    let id = ctx.manager.prefetch(AddRequest {
        url: found.url.clone(),
        filename: Some(name.to_string()),
        directory: Some(folder.to_path_buf()),
        video: Some(VideoJob::new(&found, plan)),
        ..form.req.clone()
    });
    *form.early.borrow_mut() = Some((key, id));
}

/// Start (true) or Download later.
pub fn submit(ctx: &Rc<Ctx>, form: &Rc<Form>, start_now: bool) {
    let found = form.video.found.borrow().clone();
    let (Some(found), Some(plan)) = (found, form.video.plan()) else {
        let looking = form.video.probed.lock().unwrap().is_none();
        form.info.set_text(if looking { "Still looking for the video…" } else { "There's no video to download here" });
        return;
    };
    if plan.needs_ffmpeg() && tools::ffmpeg().is_none() && !tools::CAN_FETCH_FFMPEG {
        form.info.set_text(tools::NO_FFMPEG);
        return;
    }
    let (directory, name) = form.target(ctx);
    let name = name.map(|n| with_ext(&n, &plan.ext)).unwrap_or_else(|| video::filename(&found.title, &plan.ext));
    remember_folder(ctx, form, &name, &directory);
    let (quality, format) = remembered(plan.want);
    let mut settings = ctx.manager.settings();
    if settings.video_quality != quality || format.is_some_and(|f| settings.video_format != f) {
        settings.video_quality = quality; // offered first next time
        if let Some(format) = format {
            settings.video_format = format.into();
        }
        ctx.manager.update_settings(settings);
    }
    let id = match form.take_early(ctx, Some(&key(&found.url, &plan))) {
        // downloading already: moved to the chosen name and folder, finished or not
        Some(id) => {
            let connections = ctx.manager.settings().connections;
            ctx.manager.confirm(&id, Confirm { filename: Some(name), directory, connections, start: start_now });
            id
        }
        None => ctx.manager.add(AddRequest { url: found.url.clone(), filename: Some(name), directory: Some(directory),
                                             start: start_now, video: Some(VideoJob::new(&found, &plan)),
                                             ..form.req.clone() }),
    };
    done(ctx, form, &id, start_now);
}
