//! Asking for a video (the menu's Download video…, the browser's), and Start (or Download later)
//! for one: what was picked is added like any download (TurboDM downloads its streams itself, on
//! all its connections); FFmpeg, if it's needed and missing, is got first (helpers.rs). Also what
//! a choice takes: its file's extension, and the line under the title saying it.

use super::super::wnd::set_text;
use super::super::{browsers, helpers, progress, App};
use super::{done, remember_folder, Form};
use std::rc::Rc;
use turbodm::engine::video::{self, remembered, tools, Output, Plan, VideoJob};
use turbodm::engine::AddRequest;
use turbodm::util::human_size;

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

/// Start (true) or Download later.
pub(super) fn submit(app: &Rc<App>, form: &Rc<Form>, start_now: bool) {
    let found = form.video.found.borrow().clone();
    let (Some(found), Some(plan)) = (found, form.video.plan()) else {
        let looking = form.video.probed.lock().unwrap().is_none();
        set_text(form.info.get(), if looking { "Still looking for the video…" } else { "There's no video to download here" });
        return;
    };
    if plan.needs_ffmpeg() && tools::ffmpeg().is_none() {
        if !tools::CAN_FETCH_FFMPEG {
            set_text(form.info.get(), tools::NO_FFMPEG);
            return;
        }
        let f = Rc::downgrade(form);
        helpers::with_helpers(app, move |app| if let Some(form) = f.upgrade() { submit(app, &form, start_now) });
        return;
    }
    let (directory, name) = form.target(app);
    let name = name.map(|n| with_ext(&n, &plan.ext)).unwrap_or_else(|| video::filename(&found.title, &plan.ext));
    remember_folder(app, form, &name, &directory);
    let (quality, format) = remembered(plan.want);
    let mut settings = app.manager.settings();
    if settings.video_quality != quality || format.is_some_and(|f| settings.video_format != f) {
        settings.video_quality = quality; // offered first next time
        if let Some(format) = format {
            settings.video_format = format.into();
        }
        app.manager.update_settings(settings);
    }
    let id = app.manager.add(AddRequest { url: found.url.clone(), filename: Some(name), directory: Some(directory),
                                          start: start_now, video: Some(VideoJob::new(&found, &plan)),
                                          ..form.req.clone() });
    done(app, form, &id, start_now);
}

/// `name` with `ext` instead of its media extension ("a.mp4" → "a.m4a"; "a.b" → "a.b.m4a").
pub(super) fn with_ext(name: &str, ext: &str) -> String {
    const MEDIA: [&str; 11] = ["mp4", "m4a", "webm", "mkv", "mp3", "opus", "ogg", "flv", "3gp", "mov", "aac"];
    let stem = match name.rsplit_once('.') {
        Some((stem, e)) if MEDIA.contains(&e.to_ascii_lowercase().as_str()) => stem,
        _ => name,
    };
    format!("{stem}.{ext}")
}

/// Under the title: what the choice takes.
pub(super) fn hint(plan: &Plan) -> String {
    let size = plan.size.map(|s| format!("about {}", human_size(Some(s)))).unwrap_or_else(|| "not known yet".into());
    if plan.needs_ffmpeg() && tools::ffmpeg().is_none() {
        return if tools::CAN_FETCH_FFMPEG { format!("Size: {size}  ·  needs FFmpeg, got when you start") }
               else { tools::NO_FFMPEG.into() };
    }
    match plan.output {
        Output::Mp3 => format!("Size: {size}  ·  the sound, converted to MP3"),
        Output::Single if plan.sources.len() == 1 && matches!(plan.want, video::Want::Sound(_)) => {
            format!("Size: {size}  ·  the sound as the site sends it")
        }
        Output::Single => format!("Size: {size}"),
        _ => format!("Size: {size}  ·  video and sound joined into one file"),
    }
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
