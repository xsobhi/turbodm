//! Start (or Download later) for a video: FFmpeg first when the quality needs it and there's
//! none, asked right in the dialog (the info line asks "FFmpeg is needed for this quality
//! (84.7 MB): download it?" and Start becomes "Download FFmpeg"), then the video is added.
//! Also what a quality takes: its file's extension, and the line saying so under the title.

use super::super::wnd::{enable, set_text};
use super::super::App;
use super::{done, remember_folder, Form};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use turbodm::engine::video::{self, quality_of, tools, Choice, VideoJob};
use turbodm::engine::AddRequest;
use turbodm::util::human_size;

/// FFmpeg's size (to ask), and its download.
#[derive(Default)]
pub struct Ffmpeg {
    size: Mutex<Option<u64>>,
    sized: AtomicBool, // its size was asked for
    fetch: tools::Fetch,
    busy: AtomicBool,
    result: Mutex<Option<Result<(), String>>>,
}

/// Ask how big FFmpeg is (once), to say it when asking for it.
pub(super) fn size_ffmpeg(app: &App, form: &Form) {
    let ffmpeg = &form.video.ffmpeg;
    if tools::CAN_FETCH_FFMPEG && !ffmpeg.sized.swap(true, Ordering::Relaxed) {
        let (client, ffmpeg) = (app.manager.shared.client(), ffmpeg.clone());
        app.rt.spawn(async move {
            *ffmpeg.size.lock().unwrap() = tools::ffmpeg_size(&client).await;
        });
    }
}

/// FFmpeg's download: how far it got, and once it's there, what was asked before it was needed.
pub(super) fn watch(app: &Rc<App>, form: &Rc<Form>) {
    let ffmpeg = &form.video.ffmpeg;
    let result = ffmpeg.result.lock().unwrap().take();
    match result {
        Some(Ok(())) => {
            ffmpeg.busy.store(false, Ordering::Relaxed);
            let asked = form.video.asking.get();
            stop_asking(form);
            if let Some(start_now) = asked {
                submit(app, form, start_now);
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
        // the question, with FFmpeg's size once it's known
        None if form.video.asking.get().is_some() => set_text(form.info.get(), &ask_text(form)),
        None => {}
    }
}

/// "FFmpeg is needed for this quality (84.7 MB): download it?"
fn ask_text(form: &Form) -> String {
    let size = form.video.ffmpeg.size.lock().unwrap().map(|s| human_size(Some(s))).unwrap_or_else(|| "about 85 MB".into());
    format!("FFmpeg is needed for this quality ({size}): download it?")
}

/// The question about FFmpeg goes (another quality, or it's answered).
pub(super) fn stop_asking(form: &Form) {
    if form.video.asking.take().is_some() {
        let [_, later, start] = form.buttons.get();
        set_text(start, "Start download");
        enable(later, true);
        enable(start, true);
        form.layout(); // the button's width, for its words
    }
}

/// Start (true) or Download later for a video: FFmpeg first if needed, then add it.
pub(super) fn submit(app: &Rc<App>, form: &Rc<Form>, start_now: bool) {
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
        if form.video.asking.get().is_none() {
            // first: ask, right here
            form.video.asking.set(Some(start_now));
            set_text(start, "Download FFmpeg");
            enable(later, false);
            form.layout();
            set_text(form.info.get(), &ask_text(form));
        } else {
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

/// `name` with `ext` instead of its media extension ("a.mp4" → "a.m4a"; "a.b" → "a.b.m4a").
pub(super) fn with_ext(name: &str, ext: &str) -> String {
    const MEDIA: [&str; 11] = ["mp4", "m4a", "webm", "mkv", "mp3", "opus", "ogg", "flv", "3gp", "mov", "aac"];
    let stem = match name.rsplit_once('.') {
        Some((stem, e)) if MEDIA.contains(&e.to_ascii_lowercase().as_str()) => stem,
        _ => name,
    };
    format!("{stem}.{ext}")
}

/// Under the title: what the quality takes.
pub(super) fn hint(form: &Form, choice: &Choice) -> String {
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

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_the_name_changes_the_extension() {
        assert_eq!(super::with_ext("My clip.mp4", "m4a"), "My clip.m4a");
        assert_eq!(super::with_ext("My clip", "mp4"), "My clip.mp4");
        assert_eq!(super::with_ext("v1.2 final", "mp4"), "v1.2 final.mp4");
    }
}
