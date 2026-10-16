//! TurboDM's menus, drawn as Linux Mint draws them (Mint-Y): white, a thin grey border, slightly
//! rounded corners and a soft shadow, roomy lines in Mint's font, the line under the mouse in
//! Mint's green. Windows' own menus can't look like that (their frame is Windows'), so these are
//! windows of TurboDM's own (window.rs, drawn in draw.rs) that behave as Windows' menus do
//! (track.rs): the mouse, the keyboard (arrows, Enter, Escape, the underlined letters), sub-menus,
//! and moving along a menu bar.

mod draw;
mod keys;
mod layout;
mod mouse;
mod screen;
#[cfg(test)]
mod tests;
mod track;
mod window;

pub use layout::At;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_COMMAND};

/// A menu's line. Texts mark the letter typed for it with & (&& is an &), and may have a tab
/// and what's shown on the right (a shortcut).
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A command: its id, its text, and whether it can be chosen (greyed if not).
    Do(u16, String, bool),
    /// A command with a tick when it's on.
    Tick(u16, String, bool),
    /// A command in bold: what clicking the icon itself does (the notification area's menus).
    Bold(u16, String),
    Sub(String, Vec<Item>),
    Line,
}

impl Item {
    pub fn text(&self) -> &str {
        match self {
            Item::Do(_, text, _) | Item::Tick(_, text, _) | Item::Bold(_, text) | Item::Sub(text, _) => text,
            Item::Line => "",
        }
    }

    /// The mouse and the keyboard can go to it.
    pub fn enabled(&self) -> bool {
        match self {
            Item::Do(_, _, on) => *on,
            Item::Line => false,
            _ => true,
        }
    }

    /// The command it gives, if it's one that can be chosen.
    fn command(&self) -> Option<u16> {
        match self {
            Item::Do(id, _, true) | Item::Tick(id, _, _) | Item::Bold(id, _) => Some(*id),
            _ => None,
        }
    }
}

/// The menu bar a menu opened from: the mouse moving to another of its names opens that one,
/// as do the Left and Right keys.
pub struct Bar<'a> {
    pub index: usize, // the menu open now
    pub count: usize,
    pub at: &'a dyn Fn(POINT) -> Option<usize>, // the bar's name at a point of the screen
}

#[derive(Default)]
pub struct Options<'a> {
    pub keyboard: bool, // opened from the keyboard: the first line chosen, the letters underlined
    pub min_width: i32, // (screen pixels)
    pub bar: Option<Bar<'a>>,
}

/// How a menu ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Chose(u16),
    Closed,
    /// Another of the bar's menus instead (opened from the keyboard?).
    Switch(usize, bool),
}

/// Show a menu of `items` for `owner` (it keeps the keyboard), until one is chosen or it's
/// closed.
pub fn show(owner: HWND, items: &[Item], at: At, options: Options) -> Outcome {
    track::run(owner, items, at, options)
}

/// The command chosen in a menu, if one was.
pub fn choose(owner: HWND, items: &[Item], at: At) -> Option<u16> {
    match show(owner, items, at, Options::default()) {
        Outcome::Chose(id) => Some(id),
        _ => None,
    }
}

/// A menu whose choice goes to `owner` as WM_COMMAND, as Windows' menus send it.
pub fn command(owner: HWND, items: &[Item], at: At) {
    if let Some(id) = choose(owner, items, at) {
        post_command(owner, id);
    }
}

pub fn post_command(owner: HWND, id: u16) {
    // SAFETY: posting to our own window
    unsafe { let _ = PostMessageW(Some(owner), WM_COMMAND, WPARAM(id as usize), LPARAM(0)); }
}

