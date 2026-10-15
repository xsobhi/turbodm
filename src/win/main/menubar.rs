//! The menu bar, as IDM's: Tasks, File, Downloads, View, Help in a row under the title bar. A
//! click (or Alt and the underlined letter) opens a menu, and moving to another name while one
//! is open opens that one instead, as Windows' own menu bars do.

use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::canvas::{self, Mouse};
use super::super::wnd::{px, Window};
use super::super::App;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Its height (96-DPI pixels); the menus' names (& marks the letter Alt goes with).
pub const HEIGHT: i32 = 28;
pub const MENUS: [&str; 5] = ["&Tasks", "&File", "&Downloads", "&View", "&Help"];

#[derive(Default)]
struct State {
    spans: RefCell<Vec<(i32, i32)>>, // each name's left and right (canvas pixels)
    hover: Cell<Option<usize>>,
    open: Cell<Option<usize>>,
}

impl State {
    fn at(&self, x: i32) -> Option<usize> {
        self.spans.borrow().iter().position(|&(left, right)| (left..right).contains(&x))
    }
}

pub struct MenuBar {
    pub hwnd: HWND,
    state: Rc<State>,
}

thread_local! {
    // while a menu is open: the bar, and the menu the mouse moved to (opened when this one closes)
    static TRACKING: RefCell<Option<(HWND, Rc<State>)>> = const { RefCell::new(None) };
    static NEXT: Cell<Option<usize>> = const { Cell::new(None) };
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
                    super::super::post(move |app| open(app, i));
                }
            }
            _ => {}
        });
        MenuBar { hwnd, state }
    }
}

fn paint(p: &Painter, s: &State) {
    p.fill(p.rect, theme::CHROME);
    let mut x = px(6);
    let mut spans = vec![];
    for (i, name) in MENUS.iter().enumerate() {
        let name = name.replace('&', "");
        let width = theme::measure(&name, Font::Body) + px(20);
        let r = windows::Win32::Foundation::RECT { left: x, top: px(3), right: x + width, bottom: p.rect.bottom - px(3) };
        let fill = match (s.open.get(), s.hover.get()) {
            (Some(open), _) if open == i => Some(theme::BUTTON_PRESSED),
            (None, Some(hover)) if hover == i => Some(theme::mix(theme::CHROME, theme::BORDER, 0.55)),
            _ => None,
        };
        if fill.is_some() {
            p.round(r, px(theme::RADIUS), fill, None);
        }
        p.text(r, &name, Font::Body, theme::TEXT, Align::Center);
        spans.push((r.left, r.right));
        x = r.right + px(2);
    }
    *s.spans.borrow_mut() = spans;
}

/// Open menu `index` under its name; while it's open, the mouse on another name opens that one.
pub fn open(app: &Rc<App>, index: usize) {
    let bar = &app.main.menubar;
    let mut next = Some(index);
    while let Some(i) = next.take() {
        let Some(&(left, _)) = bar.state.spans.borrow().get(i) else { break };
        bar.state.open.set(Some(i));
        canvas::redraw(bar.hwnd);
        let mut point = POINT { x: left, y: px(HEIGHT) - px(3) };
        let menu = super::menu::bar_menu(app, i);
        TRACKING.with(|t| *t.borrow_mut() = Some((bar.hwnd, bar.state.clone())));
        NEXT.with(|n| n.set(None));
        // SAFETY: our menu shown under the bar; a hook on this thread's menu loop, removed after
        unsafe {
            let _ = ClientToScreen(bar.hwnd, &mut point);
            let hook = SetWindowsHookExW(WH_MSGFILTER, Some(watch), None, GetCurrentThreadId()).ok();
            let owner = app.main.window.hwnd();
            let _ = SetForegroundWindow(owner);
            let _ = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_LEFTBUTTON, point.x, point.y, None, owner, None);
            if let Some(hook) = hook {
                let _ = UnhookWindowsHookEx(hook);
            }
            let _ = DestroyMenu(menu);
        }
        TRACKING.with(|t| t.borrow_mut().take());
        next = NEXT.with(Cell::take);
    }
    bar.state.open.set(None);
    bar.state.hover.set(None);
    canvas::redraw(bar.hwnd);
}

/// The menu loop's messages: the mouse over another of the bar's names closes this menu, to
/// open that one.
unsafe extern "system" fn watch(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == MSGF_MENU as i32 {
        // SAFETY: WH_MSGFILTER passes the message being handled
        let msg = unsafe { &*(lparam.0 as *const MSG) };
        if msg.message == WM_MOUSEMOVE {
            TRACKING.with(|t| {
                if let Some((hwnd, state)) = t.borrow().as_ref() {
                    let mut point = msg.pt;
                    // SAFETY: plain conversion to the bar's pixels
                    unsafe { let _ = ScreenToClient(*hwnd, &mut point); }
                    let on_bar = (0..px(HEIGHT)).contains(&point.y);
                    if let Some(i) = state.at(point.x).filter(|_| on_bar)
                        && state.open.get() != Some(i) {
                        NEXT.with(|n| n.set(Some(i)));
                        // SAFETY: closes the menu this thread shows
                        unsafe { let _ = EndMenu(); }
                    }
                }
            });
        }
    }
    // SAFETY: the next hook in the chain gets the message too
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
