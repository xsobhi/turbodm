//! Answering the browser's `VideoQualities`: the page's look (at once when the browser had it
//! looked at while the video played), as the menu of its qualities.

use super::{Message, Reply, VideoChoice};
use crate::engine::http::Headers;
use crate::engine::video::{self, Plan, Want};
use crate::engine::Shared;

/// The reply to `VideoQualities`. Without the helpers it says so, and has the app offer them.
pub async fn answer(shared: &Shared, to_ui: &async_channel::Sender<Message>, message: Message) -> Reply {
    let Message::VideoQualities { url, referrer, cookies, user_agent } = message else {
        return Reply::error("not a video");
    };
    if !video::tools::missing().is_empty() {
        let ask = Message::VideoQualities { url, referrer, cookies, user_agent };
        let _ = to_ui.send(ask).await; // (the app shows its window that gets them)
        return failed(video::NEEDS_HELPERS);
    }
    let headers = Headers { user_agent, referrer, cookies, extra: vec![] };
    match video::qualities(shared, &url, &headers).await {
        Ok((title, plans)) => Reply { title: Some(title), choices: plans.iter().map(choice).collect(), ..Reply::ok() },
        Err(error) => failed(&error),
    }
}

fn failed(error: &str) -> Reply {
    Reply { ok: false, error: Some(error.into()), ..Reply::ok() }
}

fn choice(plan: &Plan) -> VideoChoice {
    VideoChoice { id: video::choice_id(plan.want), label: plan.label.clone(), size: plan.size,
                  audio: matches!(plan.want, Want::Sound(_)) }
}
