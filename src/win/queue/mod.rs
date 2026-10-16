//! "TurboDM downloads": while download windows are hidden (Hide, or minimized), a second icon in
//! the notification area, beside TurboDM's (tray.rs), with the downloads' green arrow, says how
//! many; like the others it can be dragged in with the hidden icons. A click on it shows the
//! downloads in a menu, how far each is, and picking one brings its window back.

use super::theme::pictures;
use super::wnd::popup::{self, At, Item};
use super::{progress, tray, App};
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;
use turbodm::engine::Status;
use turbodm::util::human_speed;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LRESULT, POINT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The icon's id on the main window (TurboDM's is 1), and the message its clicks come as.
const ID: u32 = 2;
const WM_QUEUE: u32 = WM_APP + 3;
/// The most downloads the menu lists.
const MOST: usize = 20;

type Notify = fn(NOTIFY_ICON_MESSAGE, &NOTIFYICONDATAW) -> bool;

thread_local! {
    static TIP: RefCell<Option<String>> = const { RefCell::new(None) }; // the icon's text, while it's there
    static NOTIFY: Cell<Notify> = const { Cell::new(shell_notify) }; // (the tests' stand-in for Windows)
    static ICON: OnceCell<HICON> = const { OnceCell::new() };
}

fn shell_notify(what: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
    // SAFETY: our icon, described in full
    unsafe { Shell_NotifyIconW(what, data).as_bool() }
}

/// The icon's text, for `n` hidden download windows.
fn tip(n: usize) -> String {
    match n {
        1 => "TurboDM: 1 download".to_string(),
        n => format!("TurboDM: {n} downloads"),
    }
}

/// What the icon needs now `hidden` windows are hidden, as it shows `shown` (its text) or isn't
/// there: added, its text changed, or taken away.
fn change(shown: Option<&str>, hidden: usize) -> Option<(NOTIFY_ICON_MESSAGE, String)> {
    let tip = tip(hidden);
    match shown {
        None if hidden > 0 => Some((NIM_ADD, tip)),
        Some(_) if hidden == 0 => Some((NIM_DELETE, String::new())),
        Some(now) if now != tip => Some((NIM_MODIFY, tip)),
        _ => None,
    }
}

/// Show the icon while a download window is hidden, take it away when none is.
pub fn update(app: &Rc<App>) {
    apply(app.main.window.hwnd(), hidden(app));
}

fn hidden(app: &App) -> usize {
    // SAFETY: plain queries on our own windows
    app.progress.borrow().values().filter(|p| unsafe { !IsWindowVisible(p.window.hwnd()).as_bool() }).count()
}

fn apply(hwnd: HWND, hidden: usize) {
    let shown = TIP.with(|t| t.borrow().clone());
    let Some((what, tip)) = change(shown.as_deref(), hidden) else { return };
    let mut data = tray::data(hwnd, ID);
    if what != NIM_DELETE {
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_QUEUE;
        data.hIcon = icon();
        tray::copy(&mut data.szTip, &tip);
    }
    let notify = NOTIFY.with(Cell::get);
    // (TaskbarCreated also comes with the icon still there, when the screen's scale changes: the
    // icon is just updated then; not there at all, the taskbar isn't: tried again next time)
    let done = notify(what, &data) || (what == NIM_ADD && notify(NIM_MODIFY, &data));
    if done || what == NIM_DELETE {
        TIP.with(|t| *t.borrow_mut() = (what != NIM_DELETE).then_some(tip));
    }
}

/// Explorer started again (TaskbarCreated): its new taskbar has none of our icons.
fn taskbar_back(hwnd: HWND, hidden: usize) {
    TIP.with(|t| t.borrow_mut().take());
    apply(hwnd, hidden);
}

/// The downloads' arrow, at the notification area's size.
fn icon() -> HICON {
    // SAFETY: plain queries
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) };
    ICON.with(|i| *i.get_or_init(|| pictures::hicon(pictures::DOWNLOADING, size).unwrap_or_default()))
}

/// Its clicks, and the taskbar coming back, on the main window (where TurboDM's icon is too).
pub fn start(app: &Rc<App>) {
    let hwnd = app.main.window.hwnd();
    // SAFETY: plain registration (tray.rs registers the same)
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    app.main.window.hook(move |msg, _, lparam| {
        if msg == taskbar_created {
            taskbar_back(hwnd, hidden(&super::app()));
            return None;
        }
        if msg != WM_QUEUE {
            return None;
        }
        if matches!((lparam.0 & 0xffff) as u32, WM_LBUTTONUP | WM_RBUTTONUP | WM_CONTEXTMENU) {
            choose(&super::app());
        }
        Some(LRESULT(0))
    });
}

/// TurboDM is quitting: the icon goes (it would stay until the mouse passed over it).
pub fn remove(app: &App) {
    if TIP.with(|t| t.borrow_mut().take()).is_some() {
        NOTIFY.with(Cell::get)(NIM_DELETE, &tray::data(app.main.window.hwnd(), ID));
    }
}

/// The menu of downloads, by the icon (where the mouse is).
fn choose(app: &Rc<App>) {
    // the ones with windows, then the others under way
    let mut ids: Vec<String> = app.progress.borrow().keys().cloned().collect();
    ids.sort();
    let others = app.manager.snapshot_where(|id, status| status.is_active() && !ids.iter().any(|i| i == id));
    ids.extend(others.into_iter().map(|s| s.id));
    let lines: Vec<(String, String)> = ids.iter().filter_map(|id| app.manager.get(id)).take(MOST).map(|s| {
        let how = match (s.status, s.progress) {
            (Status::Completed, _) => "Done".to_string(),
            (status, Some(p)) if status.is_active() => format!("{:.0}%  {}", p * 100.0, human_speed(s.speed)),
            (status, Some(p)) => format!("{:.0}%  {}", p * 100.0, status.label()),
            (status, None) => status.label().to_string(),
        };
        (s.id.clone(), format!("{}\t{how}", s.filename.replace('&', "&&"))) // (an & in a name isn't a letter to type)
    }).collect();
    let mut items: Vec<Item> = lines.iter().enumerate().map(|(i, (_, text))| Item::Do(i as u16 + 1, text.clone(), true)).collect();
    if items.is_empty() {
        items.push(Item::Do(0, "No downloads under way".to_string(), false));
    }
    let (all, main) = (lines.len() as u16 + 1, lines.len() as u16 + 2);
    items.extend([Item::Line, Item::Do(all, "Show &all download windows".to_string(), true),
                  Item::Do(main, "&Open TurboDM".to_string(), true)]);
    let mut point = POINT::default();
    // SAFETY: plain query
    unsafe { let _ = GetCursorPos(&mut point); }
    match popup::choose(app.main.window.hwnd(), &items, At::Point(point)) {
        None => {}
        Some(n) if n == all => show_all(app),
        Some(n) if n == main => app.show(),
        Some(n) => {
            if let Some((id, _)) = lines.get(n as usize - 1) {
                progress::open(app, id);
            }
        }
    }
}

/// Every download window back on screen.
fn show_all(app: &Rc<App>) {
    let ids: Vec<String> = app.progress.borrow().keys().cloned().collect();
    for id in ids {
        progress::open(app, &id);
    }
}

#[cfg(test)]
mod tests;
