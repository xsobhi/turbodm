//! "TurboDM downloads", as on Windows (src/win/queue): while download windows are hidden (Hide,
//! or minimized), a second icon in the notification area, beside TurboDM's (tray.rs), with the
//! downloads' green arrow, says how many. Its menu lists the downloads, how far each is, and
//! picking one brings its window back; a click shows them all.

use super::{progress, Ctx};
use gtk::prelude::*;
use gtk::{gdk, glib};
use ksni::menu::StandardItem;
use ksni::TrayMethods;
use std::cell::RefCell;
use std::rc::Rc;
use turbodm::engine::Status;
use turbodm::util::human_speed;

/// The most downloads the menu lists.
const MOST: usize = 20;

/// What the icon shows: its text and the menu's downloads (id, line).
#[derive(Clone, Default, PartialEq)]
struct Shown {
    tip: String,
    lines: Vec<(String, String)>,
}

enum Cmd {
    Open(String),
    ShowAll,
    Main,
}

struct Queue {
    shown: Shown,
    icon: Vec<ksni::Icon>,
    tx: async_channel::Sender<Cmd>,
}

impl ksni::Tray for Queue {
    fn id(&self) -> String {
        "turbodm-downloads".into()
    }
    fn title(&self) -> String {
        self.shown.tip.clone()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: self.shown.tip.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.try_send(Cmd::ShowAll);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let item = |label: String, enabled: bool, cmd: fn(&str) -> Cmd, id: String| -> ksni::MenuItem<Self> {
            StandardItem {
                label,
                enabled,
                activate: Box::new(move |tray: &mut Self| {
                    let _ = tray.tx.try_send(cmd(&id));
                }),
                ..Default::default()
            }.into()
        };
        let mut items: Vec<ksni::MenuItem<Self>> = self.shown.lines.iter()
            .map(|(id, line)| item(line.replace('_', "__"), true, |id| Cmd::Open(id.to_string()), id.clone())).collect();
        if items.is_empty() {
            items.push(item("No downloads under way".into(), false, |_| Cmd::Main, String::new()));
        }
        items.push(ksni::MenuItem::Separator);
        items.push(item("Show _all download windows".into(), true, |_| Cmd::ShowAll, String::new()));
        items.push(item("_Open TurboDM".into(), true, |_| Cmd::Main, String::new()));
        items
    }
}

thread_local! {
    static TO_TRAY: RefCell<Option<async_channel::Sender<Option<Shown>>>> = const { RefCell::new(None) };
    static LAST: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// The icon's text, for `n` hidden download windows.
fn tip(n: usize) -> String {
    match n {
        1 => "TurboDM: 1 download".to_string(),
        n => format!("TurboDM: {n} downloads"),
    }
}

/// Start listening (once): the icon comes and goes with hidden download windows.
pub fn start(ctx: &Rc<Ctx>) {
    let (to_tray, from_ui) = async_channel::unbounded::<Option<Shown>>();
    let (tx, rx) = async_channel::unbounded::<Cmd>();
    let icon = arrow();
    ctx.rt.spawn(async move {
        let mut handle: Option<ksni::Handle<Queue>> = None;
        while let Ok(shown) = from_ui.recv().await {
            match (shown, &handle) {
                (None, Some(h)) => {
                    h.shutdown().await;
                    handle = None;
                }
                (Some(shown), Some(h)) => {
                    h.update(move |q| q.shown = shown).await;
                }
                (Some(shown), None) => {
                    let tray = Queue { shown, icon: icon.clone(), tx: tx.clone() };
                    handle = tray.spawn().await.ok();
                }
                (None, None) => {}
            }
        }
    });
    TO_TRAY.with(|t| *t.borrow_mut() = Some(to_tray));
    let ctx = ctx.clone();
    glib::spawn_future_local(async move {
        while let Ok(cmd) = rx.recv().await {
            match cmd {
                Cmd::Open(id) => progress::open(&ctx, &id),
                Cmd::ShowAll => {
                    let ids: Vec<String> = ctx.progress.borrow().keys().cloned().collect();
                    ids.iter().for_each(|id| progress::open(&ctx, id));
                }
                Cmd::Main => ctx.show_all(),
            }
        }
    });
}

/// Show the icon while a download window is hidden (with how far each download is), take it
/// away when none is.
pub fn update(ctx: &Ctx) {
    let hidden = ctx.progress.borrow().values().filter(|p| !p.window.is_visible()).count();
    let shown = (hidden > 0).then(|| Shown { tip: tip(hidden), lines: lines(ctx) });
    let changed = LAST.with(|last| {
        let mut last = last.borrow_mut();
        let changed = *last != shown;
        *last = shown.clone();
        changed
    });
    if changed {
        TO_TRAY.with(|t| if let Some(t) = t.borrow().as_ref() { let _ = t.try_send(shown); });
    }
}

/// The downloads with windows, then the others under way: "name    42%  1.2 MB/s".
fn lines(ctx: &Ctx) -> Vec<(String, String)> {
    let mut ids: Vec<String> = ctx.progress.borrow().keys().cloned().collect();
    ids.sort();
    let others = ctx.manager.snapshot_where(|id, status| status.is_active() && !ids.iter().any(|i| i == id));
    ids.extend(others.into_iter().map(|s| s.id));
    ids.iter().filter_map(|id| ctx.manager.get(id)).take(MOST).map(|s| {
        let how = match (s.status, s.progress) {
            (Status::Completed, _) => "Done".to_string(),
            (status, Some(p)) if status.is_active() => format!("{:.0}%  {}", p * 100.0, human_speed(s.speed)),
            (status, Some(p)) => format!("{:.0}%  {}", p * 100.0, status.label()),
            (status, None) => status.label().to_string(),
        };
        (s.id.clone(), format!("{}    {how}", s.filename))
    }).collect()
}

/// The downloads' green arrow (data/icons/downloading.png), as the tray wants it: ARGB32.
fn arrow() -> Vec<ksni::Icon> {
    let png: &'static [u8] = include_bytes!("../../data/icons/downloading.png");
    let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_static(png)) else { return vec![] };
    let (width, height) = (texture.width(), texture.height());
    let mut data = vec![0u8; (width * height * 4) as usize];
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::A8r8g8b8);
    let (bytes, stride) = downloader.download_bytes();
    for y in 0..height as usize {
        let row = &bytes[y * stride..y * stride + width as usize * 4];
        data[y * width as usize * 4..(y + 1) * width as usize * 4].copy_from_slice(row);
    }
    vec![ksni::Icon { width, height, data }]
}
