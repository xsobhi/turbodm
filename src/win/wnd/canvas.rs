//! Controls TurboDM draws itself, the way Windows 11's own apps look (the toolbar, the sidebar,
//! tabs, progress bars…): child windows whose painting (double-buffered: nothing flickers) and
//! mouse go to Rust closures, with tooltips for their parts.

use super::{instance, Window};
use crate::win::theme::Painter;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Once;
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: PCWSTR = w!("TurboDMCanvas");

/// What the mouse did, at (x, y) in the canvas' pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mouse {
    Move(i32, i32),
    Leave,
    Down(i32, i32),
    Up(i32, i32),
    Double(i32, i32),
}

struct Canvas {
    paint: Box<dyn Fn(&Painter)>,
    mouse: RefCell<Option<Rc<dyn Fn(Mouse)>>>,
    tracking: Cell<bool>,
    tips: Cell<HWND>, // its tooltip window, once it has tips
    tip_count: Cell<usize>,
    hit: RefCell<Option<Rc<dyn Fn(i32, i32) -> bool>>>,
}

thread_local! {
    static CANVASES: RefCell<HashMap<isize, Rc<Canvas>>> = RefCell::default();
}

fn get(hwnd: HWND) -> Option<Rc<Canvas>> {
    CANVASES.with(|c| c.borrow().get(&(hwnd.0 as isize)).cloned())
}

impl Window {
    /// A control that `paint` draws (all of it, in its own pixels); see `on_mouse`.
    pub fn canvas(&self, paint: impl Fn(&Painter) + 'static) -> HWND {
        register();
        let id = self.next_id();
        // SAFETY: a child of our window; Windows destroys it with the window
        let hwnd = unsafe {
            CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, PCWSTR::null(), WS_CHILD | WS_VISIBLE, 0, 0, 0, 0,
                            Some(self.hwnd()), Some(HMENU(id as usize as *mut _)), Some(instance()), None)
        }.unwrap_or_default();
        let canvas = Canvas { paint: Box::new(paint), mouse: RefCell::default(), tracking: Cell::new(false),
                              tips: Cell::default(), tip_count: Cell::new(0), hit: RefCell::default() };
        CANVASES.with(|c| c.borrow_mut().insert(hwnd.0 as isize, Rc::new(canvas)));
        hwnd
    }
}

/// What to do when the mouse moves over, leaves, or clicks the canvas.
pub fn on_mouse(hwnd: HWND, f: impl Fn(Mouse) + 'static) {
    if let Some(canvas) = get(hwnd) {
        *canvas.mouse.borrow_mut() = Some(Rc::new(f));
    }
}

/// Which parts of the canvas are its own: elsewhere (`f` is false at that x, y) the mouse goes
/// to the window under it (on a title bar: the caption, to drag the window by).
pub fn on_hit(hwnd: HWND, f: impl Fn(i32, i32) -> bool + 'static) {
    if let Some(canvas) = get(hwnd) {
        *canvas.hit.borrow_mut() = Some(Rc::new(f));
    }
}

/// Draw it again: what it shows changed.
pub fn redraw(hwnd: HWND) {
    // SAFETY: plain call on our control
    unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
}

/// Tooltips for parts of the canvas: (area in its pixels, text). Replaces any earlier ones.
pub fn set_tips(hwnd: HWND, tips: &[(RECT, String)]) {
    let Some(canvas) = get(hwnd) else { return };
    // SAFETY: Windows' tooltip control, owned by the canvas; texts are copied by TTM_ADDTOOL
    unsafe {
        if canvas.tips.get().is_invalid() {
            let style = WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX);
            let tips = CreateWindowExW(WS_EX_TOPMOST, TOOLTIPS_CLASSW, PCWSTR::null(), style, CW_USEDEFAULT, CW_USEDEFAULT,
                                       CW_USEDEFAULT, CW_USEDEFAULT, Some(hwnd), None, Some(instance()), None).unwrap_or_default();
            canvas.tips.set(tips);
        }
        let window = canvas.tips.get();
        let tool = |id: usize| TTTOOLINFOW { cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32, uFlags: TTF_SUBCLASS,
                                              hwnd, uId: id, ..Default::default() };
        for id in 0..canvas.tip_count.get() {
            SendMessageW(window, TTM_DELTOOLW, None, Some(LPARAM(&tool(id) as *const _ as isize)));
        }
        for (id, (rect, text)) in tips.iter().enumerate() {
            let text = HSTRING::from(text.as_str());
            let info = TTTOOLINFOW { rect: *rect, lpszText: PWSTR(text.as_ptr() as *mut _), ..tool(id) };
            SendMessageW(window, TTM_ADDTOOLW, None, Some(LPARAM(&info as *const _ as isize)));
        }
        canvas.tip_count.set(tips.len());
    }
}

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: plain class registration with a static name and procedure
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(procedure),
            hInstance: instance(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
}

extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(canvas) = get(hwnd) else {
        // SAFETY: the default handling
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
    let mouse = |event: Mouse| {
        let f = canvas.mouse.borrow().clone(); // the handler may set another
        if let Some(f) = f {
            f(event);
        }
    };
    // SAFETY: standard handling of our own window's messages
    unsafe {
        match msg {
            WM_ERASEBKGND => return LRESULT(1), // painted whole in WM_PAINT
            WM_NCHITTEST => {
                let hit = canvas.hit.borrow().clone();
                if let Some(hit) = hit {
                    let mut point = windows::Win32::Foundation::POINT { x, y };
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut point);
                    if !hit(point.x, point.y) {
                        return LRESULT(HTTRANSPARENT as isize);
                    }
                }
            }
            WM_PAINT => {
                Painter::paint(hwnd, |p| (canvas.paint)(p));
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                if !canvas.tracking.replace(true) {
                    let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                                      dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut track);
                }
                mouse(Mouse::Move(x, y));
            }
            WM_MOUSELEAVE => {
                canvas.tracking.set(false);
                mouse(Mouse::Leave);
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd); // the release comes here even off the canvas
                mouse(Mouse::Down(x, y));
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                mouse(Mouse::Up(x, y));
            }
            WM_LBUTTONDBLCLK => mouse(Mouse::Double(x, y)),
            WM_NCDESTROY => {
                CANVASES.with(|c| c.borrow_mut().remove(&(hwnd.0 as isize)));
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
