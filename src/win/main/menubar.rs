//! The menu bar, as IDM's: Tasks, File, Downloads, View, Help in a row under the title bar. A
//! click (or Alt and the underlined letter) opens a menu, and moving to another name while one
//! is open opens that one instead, as Windows' own menu bars do. As Mint-Y draws it, the name
//! whose menu is open is in Mint's green, the menu right under it (wnd/popup).

use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::canvas::{self, Mouse};
use super::super::wnd::popup::{self, At, Bar, Options, Outcome};
use super::super::wnd::{px, Window};
use super::super::App;
use std::cell::Cell;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient, UpdateWindow};

/// Its height (96-DPI pixels); the menus' names (& marks the letter Alt goes with).
pub const HEIGHT: i32 = 28;
pub const MENUS: [&str; 5] = ["&Tasks", "&File", "&Downloads", "&View", "&Help"];

#[derive(Default)]
struct State {
    hover: Cell<Option<usize>>,
    open: Cell<Option<usize>>,
}

impl State {
    fn at(&self, x: i32) -> Option<usize> {
        spans().iter().position(|&(left, right)| (left..right).contains(&x))
    }
}

/// Each name's left and right (canvas pixels).
fn spans() -> Vec<(i32, i32)> {
    let mut x = px(6);
    MENUS.iter().map(|name| {
        let left = x;
        x += theme::measure(&name.replace('&', ""), Font::Body) + px(20);
        let span = (left, x);
        x += px(2);
        span
    }).collect()
}

pub struct MenuBar {
    pub hwnd: HWND,
    state: Rc<State>,
}

impl MenuBar {
    pub fn new(window: &Window) -> MenuBar {
        let state = Rc::new(State::default());
        let s = state.clone();
        let hwnd = window.canvas(move |p| paint(p, &s));
        let s = state.clone();
        canvas::on_mouse(hwnd, move |event| match event {
            Mouse::Move(x, _) => {
                let at = s.at(x);
                if s.hover.replace(at) != at {
                    canvas::redraw(hwnd);
                }
            }
            Mouse::Leave => {
                s.hover.set(None);
                canvas::redraw(hwnd);
            }
            Mouse::Down(x, _) => {
                if let Some(i) = s.at(x) {
                    super::super::post(move |app| open(app, i, false));
                }
            }
            _ => {}
        });
        MenuBar { hwnd, state }
    }

    /// The bar's name under `p` (screen pixels).
    fn name_at(&self, p: POINT) -> Option<usize> {
        let mut p = p;
        // SAFETY: plain conversion to the bar's pixels
        unsafe { let _ = ScreenToClient(self.hwnd, &mut p); }
        (0..px(HEIGHT)).contains(&p.y).then(|| self.state.at(p.x)).flatten()
    }
}

fn paint(p: &Painter, s: &State) {
    p.fill(p.rect, theme::CHROME);
    for (i, (name, (left, right))) in MENUS.iter().zip(spans()).enumerate() {
        let name = name.replace('&', "");
        let r = RECT { left, top: px(3), right, bottom: p.rect.bottom - px(3) };
        let (fill, color) = match (s.open.get(), s.hover.get()) {
            (Some(open), _) if open == i => (Some(theme::ACCENT), theme::WHITE), // its menu is open
            (None, Some(hover)) if hover == i => (Some(theme::mix(theme::CHROME, theme::BORDER, 0.55)), theme::TEXT),
            _ => (None, theme::TEXT),
        };
        if fill.is_some() {
            p.round(r, px(theme::RADIUS), fill, None);
        }
        p.text(r, &name, Font::Body, color, Align::Center);
    }
}

/// Open menu `index` under its name (`keyboard`: from Alt and its letter); while it's open, the
/// mouse on another name, or Left and Right, open the others. The command chosen goes to the
/// window as WM_COMMAND, as from Windows' menus (menu.rs `run`).
pub fn open(app: &Rc<App>, index: usize, keyboard: bool) {
    let bar = &app.main.menubar;
    let owner = app.main.window.hwnd();
    let mut next = Some((index, keyboard));
    while let Some((i, keyboard)) = next.take() {
        let Some(&(left, right)) = spans().get(i) else { break };
        bar.state.open.set(Some(i));
        canvas::redraw(bar.hwnd);
        // SAFETY: plain calls on our own control: drawn green now, under the menu that opens
        unsafe { let _ = UpdateWindow(bar.hwnd); }
        let mut corners = [POINT { x: left, y: px(3) }, POINT { x: right, y: px(HEIGHT) }];
        for corner in &mut corners {
            // SAFETY: plain conversion to the screen's pixels
            unsafe { let _ = ClientToScreen(bar.hwnd, corner); }
        }
        let name = RECT { left: corners[0].x, top: corners[0].y, right: corners[1].x, bottom: corners[1].y };
        let items = super::menu::bar_menu(app, i);
        let at = |p: POINT| bar.name_at(p);
        let options = Options { keyboard, bar: Some(Bar { index: i, count: MENUS.len(), at: &at }), ..Default::default() };
        match popup::show(owner, &items, At::Below(name), options) {
            Outcome::Chose(id) => popup::post_command(owner, id),
            Outcome::Switch(other, keyboard) => next = Some((other, keyboard)),
            Outcome::Closed => {}
        }
    }
    bar.state.open.set(None);
    bar.state.hover.set(None);
    canvas::redraw(bar.hwnd);
}
