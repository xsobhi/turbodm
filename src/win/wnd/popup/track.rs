//! A menu open, in a loop of its own until it ends, as Windows' TrackPopupMenu runs one: the mouse
//! is held by the menus (SetCapture) and the keyboard read by them (mouse.rs, keys.rs); the rest
//! (painting, timers, TurboDM's jobs) goes on meanwhile. Sub-menus open beside their line.

use super::draw::{self, Look};
use super::layout::{self, At, Geometry, PAD};
use super::screen::{self, cursor, in_front, screen_point, system_cues, work_area};
use super::window::{Surface, CANCELLED};
use super::{Item, Options, Outcome};
use crate::win::theme::{self, Font};
use crate::win::wnd::{moves, px};
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

/// One menu on the screen: the main one, or a sub-menu (opened from its parent's line `opener`).
pub(super) struct Level<'a> {
    pub items: &'a [Item],
    pub geo: Geometry,
    pub surface: Surface,
    pub selected: Option<usize>,
    pub opener: Option<usize>,
}

pub(super) struct Menu<'a> {
    pub owner: HWND,
    pub levels: Vec<Level<'a>>,
    pub options: Options<'a>,
    pub cues: bool,                               // the typed letters underlined
    pub pending: Option<(usize, Option<usize>)>,  // when the timer goes: the sub-menu of that line
    pub last: POINT,                              // where the mouse was
    pub alt: bool,                                // Alt is down, alone so far
}

const SUB_TIMER: usize = 1;
const WATCH_TIMER: usize = 2;

thread_local! {
    static OPEN: Cell<bool> = const { Cell::new(false) }; // (one menu at a time)
}

pub fn run(owner: HWND, items: &[Item], at: At, options: Options) -> Outcome {
    if items.is_empty() || OPEN.with(|o| o.replace(true)) {
        return Outcome::Closed;
    }
    CANCELLED.with(|c| c.set(false));
    let offscreen = moves::offscreen(); // (tests: nothing on the screen, the mouse left alone)
    let keyboard = options.keyboard;
    let mut menu = Menu { owner, levels: vec![], options, cues: keyboard || system_cues(), pending: None,
                          last: cursor(), alt: false };
    if !offscreen {
        // SAFETY: plain call: the keyboard comes to our window, as for Windows' menus
        unsafe { let _ = SetForegroundWindow(owner); }
    }
    menu.open(items, at, None);
    if keyboard {
        menu.select(0, super::keys::step(items, None, true));
    }
    let root = menu.root();
    // SAFETY: plain calls: the mouse to the menu, and a timer that checks TurboDM is still in front
    unsafe {
        if !offscreen {
            SetCapture(root);
        }
        SetTimer(Some(root), WATCH_TIMER, 100, None);
    }
    let outcome = menu.track(offscreen);
    // SAFETY: as above, undone
    unsafe {
        let _ = KillTimer(Some(menu.root()), WATCH_TIMER);
        let _ = KillTimer(Some(menu.root()), SUB_TIMER);
        if GetCapture() == menu.root() {
            let _ = ReleaseCapture();
        }
    }
    drop(menu); // (its windows go)
    OPEN.with(|o| o.set(false));
    outcome
}

impl<'a> Menu<'a> {
    fn track(&mut self, offscreen: bool) -> Outcome {
        let mut msg = MSG::default();
        loop {
            if CANCELLED.with(Cell::get) {
                return Outcome::Closed;
            }
            // SAFETY: this thread's messages, as its main loop reads them
            match unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 {
                0 => {
                    // SAFETY: TurboDM is quitting: the main loop gets that too
                    unsafe { PostQuitMessage(msg.wParam.0 as i32) };
                    return Outcome::Closed;
                }
                -1 => return Outcome::Closed,
                _ => {}
            }
            let m = msg.message;
            let mouse = (WM_MOUSEFIRST..=WM_MOUSELAST).contains(&m) || (WM_NCMOUSEMOVE..=WM_NCXBUTTONDBLCLK).contains(&m);
            let done = match m {
                _ if mouse => self.mouse(m, screen_point(&msg)),
                WM_KEYDOWN | WM_SYSKEYDOWN => self.key(&msg),
                WM_KEYUP | WM_SYSKEYUP => (msg.wParam.0 == 0x12 && std::mem::take(&mut self.alt)).then_some(Outcome::Closed), // Alt alone
                WM_CHAR | WM_SYSCHAR => char::from_u32(msg.wParam.0 as u32).and_then(|c| self.letter(c)),
                WM_TIMER if msg.hwnd == self.root() && msg.wParam.0 == SUB_TIMER => self.timer(),
                WM_TIMER if msg.hwnd == self.root() && msg.wParam.0 == WATCH_TIMER => (!offscreen && !in_front(self.root())).then_some(Outcome::Closed),
                _ => {
                    // SAFETY: everything else goes where it was going
                    unsafe { DispatchMessageW(&msg) };
                    None
                }
            };
            if let Some(outcome) = done {
                return outcome;
            }
        }
    }

    pub fn root(&self) -> HWND {
        self.levels[0].surface.hwnd
    }

    /// Open a menu of `items` `at` a place (a sub-menu: from the last one's line `opener`).
    pub fn open(&mut self, items: &'a [Item], at: At, opener: Option<usize>) {
        let min_width = if self.levels.is_empty() { self.options.min_width } else { 0 };
        let font = |bold: bool| if bold { Font::Strong } else { Font::Body };
        let geo = layout::measure(items, |text, bold| theme::measure(text, font(bold)), px, min_width);
        let at_point = match at {
            At::Point(p) => p,
            At::Below(r) => POINT { x: r.left, y: r.bottom },
            At::Beside(parent, top) => POINT { x: parent.right, y: top },
        };
        let p = screen::off_screen_in_tests(layout::place(at, (geo.width, geo.height), work_area(at_point), px(PAD)), geo.width);
        let body = RECT { left: p.x, top: p.y, right: p.x + geo.width, bottom: p.y + geo.height };
        let surface = Surface::new(self.owner, body);
        self.levels.push(Level { items, geo, surface, selected: None, opener });
        self.paint(self.levels.len() - 1);
    }

    /// Open the sub-menu of line `i` of menu `l` (`first`: with its first line chosen).
    pub fn open_sub(&mut self, l: usize, i: usize, first: bool) {
        self.close_after(l);
        let items: &'a [Item] = self.levels[l].items;
        let Item::Sub(_, sub) = &items[i] else { return };
        let (parent, top) = (self.levels[l].surface.body, self.levels[l].geo.rows[i].0);
        self.select(l, Some(i));
        self.open(sub, At::Beside(parent, parent.top + top), Some(i));
        if first {
            self.select(l + 1, super::keys::step(sub, None, true));
        }
    }

    /// Close the menus opened from menu `l`.
    pub fn close_after(&mut self, l: usize) {
        self.levels.truncate(l + 1);
    }

    pub fn select(&mut self, l: usize, item: Option<usize>) {
        if self.levels[l].selected != item {
            self.levels[l].selected = item;
            self.paint(l);
        }
    }

    pub fn paint(&self, l: usize) {
        let level = &self.levels[l];
        let look = Look { items: level.items, geo: &level.geo, selected: level.selected, cues: self.cues };
        level.surface.show(|p, origin| draw::menu(p, origin, &look));
    }

    /// In a while (Windows' menu delay), open line `item`'s sub-menu in menu `l` (closing others).
    pub fn later(&mut self, l: usize, item: Option<usize>) {
        if self.pending == Some((l, item)) {
            return;
        }
        self.pending = Some((l, item));
        let mut delay = 400u32;
        // SAFETY: a plain query into our variable, and a timer on our window
        unsafe {
            let _ = SystemParametersInfoW(SPI_GETMENUSHOWDELAY, 0, Some((&raw mut delay).cast()), Default::default());
            SetTimer(Some(self.root()), SUB_TIMER, delay.max(1), None);
        }
    }

    pub fn not_later(&mut self) {
        if self.pending.take().is_some() {
            // SAFETY: our own timer
            unsafe { let _ = KillTimer(Some(self.root()), SUB_TIMER); }
        }
    }

    fn timer(&mut self) -> Option<Outcome> {
        let pending = self.pending;
        self.not_later();
        let (l, item) = pending?;
        let open = self.levels.get(l + 1).and_then(|sub| sub.opener);
        if l >= self.levels.len() || (open.is_some() && open == item) {
            return None;
        }
        self.close_after(l);
        if let Some(i) = item.filter(|&i| matches!(self.levels[l].items[i], Item::Sub(..))) {
            self.open_sub(l, i, false);
        }
        None
    }
}
