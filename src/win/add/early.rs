//! A video's early download, like a file's (link.rs): the quality picked downloads behind the
//! dialog on one connection while it's open, so Start feels instant; Start moves it to the name
//! and folder chosen and gives it all its connections (`Manager::confirm`), even if it finished
//! meanwhile; another quality or format, or Cancel, throws it away (with what it downloaded).

use super::super::App;
use super::Form;
use std::path::Path;
use std::rc::Rc;
use turbodm::engine::video::{self, tools, Plan, Source, VideoJob};
use turbodm::engine::AddRequest;

/// Which download a quality's early one is: the video's page and the choice.
pub fn key(url: &str, plan: &Plan) -> String {
    format!("{url}#{}", video::choice_id(plan.want))
}

/// The quality picked (`plan`, to go to `folder` as `name`): download it now, unless that's
/// turned off (Preferences), or it's left to yt-dlp (fragments), or it needs FFmpeg first.
pub fn start(app: &Rc<App>, form: &Rc<Form>, plan: &Plan, folder: &Path, name: &str) {
    let Some(found) = form.video.found.borrow().clone() else { return };
    let key = key(&found.url, plan);
    if form.early.borrow().as_ref().is_some_and(|(early, _)| *early == key) {
        return; // (picked again: it's downloading already)
    }
    form.take_early(app, None); // another quality's
    let direct = !plan.sources.is_empty() && plan.sources.iter().all(Source::plain);
    if !app.manager.settings().predownload || !direct || (plan.needs_ffmpeg() && tools::ffmpeg().is_none()) {
        return;
    }
    let id = app.manager.prefetch(AddRequest {
        url: found.url.clone(),
        filename: Some(name.to_string()),
        directory: Some(folder.to_path_buf()),
        video: Some(VideoJob::new(&found, plan)),
        ..form.req.clone()
    });
    *form.early.borrow_mut() = Some((key, id));
}
