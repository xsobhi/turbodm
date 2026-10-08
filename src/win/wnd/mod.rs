//! Window plumbing: one window class whose procedure hands messages to Rust closures, and the
//! child controls (Windows' own buttons, fields, lists…) placed in DPI-scaled pixels.

mod class;
mod controls;
mod layout;

pub use controls::*;
pub use layout::*;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use class::{register_class, CLASS};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::NMHDR;
use windows::Win32::UI::WindowsAndMessaging::*;

pub type Hook = Rc<dyn Fn(u32, WPARAM, LPARAM) -> Option<LRESULT>>;
type Notify = Rc<dyn Fn(&NMHDR, LPARAM) -> Option<LRESULT>>;
type Command = Rc<dyn Fn(u32)>;

/// A top-level window (the main window, a dialog) and what to do with its messages.
pub struct Window {
    pub hwnd: Cell<HWND>,
    commands: RefCell<HashMap<u16, Command>>, // control id → on WM_COMMAND(notification)
    notifies: RefCell<HashMap<u16, Notify>>,           // control id → on WM_NOTIFY
    hooks: RefCell<Vec<Hook>>,                         // anything else, in order
    on_close: RefCell<Option<Rc<dyn Fn() -> bool>>>,   // false: stay open
    next_id: Cell<u16>,
    default_id: Cell<u16>, // the button Enter presses
}

pub const APP_ICON: u16 = 1; // the program's icon resource (build.rs)

pub fn instance() -> HINSTANCE {
    // SAFETY: the running program's own module
    unsafe { GetModuleHandleW(None).map(|m| m.into()).unwrap_or_default() }
}

impl Window {
    /// Create a window `size` (in 96-DPI pixels) big; it shows when `show()` is called.
    pub fn new(title: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, owner: Option<HWND>, size: (i32, i32)) -> Rc<Self> {
        register_class();
        let win = Rc::new(Window {
            hwnd: Cell::new(HWND::default()),
            commands: RefCell::default(),
            notifies: RefCell::default(),
            hooks: RefCell::default(),
            on_close: RefCell::default(),
            next_id: Cell::new(100),
            default_id: Cell::new(0),
        });
        let (width, height) = outer_size(px(size.0), px(size.1), style, ex);
        // SAFETY: the window keeps one reference (GWLP_USERDATA), released in WM_NCDESTROY
        unsafe {
            let param = Rc::into_raw(win.clone());
            let created = CreateWindowExW(ex, CLASS, &HSTRING::from(title), style, CW_USEDEFAULT, CW_USEDEFAULT,
                                          width, height, owner, None, Some(instance()), Some(param.cast()));
            if created.is_err() {
                drop(Rc::from_raw(param));
            }
        }
        win
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd.get()
    }

    /// A new id for a child control.
    pub fn next_id(&self) -> u16 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        id
    }

    pub fn on_command(&self, id: u16, f: impl Fn(u32) + 'static) {
        self.commands.borrow_mut().insert(id, Rc::new(f));
    }

    pub fn on_notify(&self, id: u16, f: impl Fn(&NMHDR, LPARAM) -> Option<LRESULT> + 'static) {
        self.notifies.borrow_mut().insert(id, Rc::new(f));
    }

    pub fn hook(&self, f: impl Fn(u32, WPARAM, LPARAM) -> Option<LRESULT> + 'static) {
        self.hooks.borrow_mut().push(Rc::new(f));
    }

    /// Asked before closing (the close button, Escape, Alt+F4); `false` keeps it open.
    pub fn on_close(&self, f: impl Fn() -> bool + 'static) {
        *self.on_close.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_default(&self, id: u16) {
        self.default_id.set(id);
    }

    pub fn show(&self) {
        // SAFETY: plain calls on our own window
        unsafe {
            let _ = ShowWindow(self.hwnd(), SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd());
            if self.default_id.get() != 0 {
                // the keyboard starts on the main button, as in Windows' own dialogs
                if let Ok(button) = GetDlgItem(Some(self.hwnd()), self.default_id.get() as i32) {
                    let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(button));
                }
            }
        }
    }

    /// Close as the close button would (asks `on_close`).
    pub fn close(&self) {
        // SAFETY: as above
        unsafe { SendMessageW(self.hwnd(), WM_CLOSE, None, None) };
    }

    /// Close without asking.
    pub fn destroy(&self) {
        // SAFETY: as above
        unsafe { let _ = DestroyWindow(self.hwnd()); }
    }

    pub fn set_title(&self, title: &str) {
        set_text(self.hwnd(), title);
    }

    pub(super) fn handle(&self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let hooks = self.hooks.borrow().clone(); // a hook may add another
        if let Some(result) = hooks.iter().find_map(|hook| hook(msg, wparam, lparam)) {
            return Some(result);
        }
        match msg {
            WM_COMMAND => {
                let f = self.commands.borrow().get(&((wparam.0 & 0xffff) as u16)).cloned();
                f.map(|f| {
                    f(((wparam.0 >> 16) & 0xffff) as u32);
                    LRESULT(0)
                })
            }
            WM_NOTIFY => {
                // SAFETY: WM_NOTIFY's lparam points at an NMHDR (or a struct starting with one)
                let header = unsafe { &*(lparam.0 as *const NMHDR) };
                let f = self.notifies.borrow().get(&(header.idFrom as u16)).cloned();
                f.and_then(|f| f(header, lparam))
            }
            DM_GETDEFID if self.default_id.get() != 0 => {
                Some(LRESULT(((DC_HASDEFID as isize) << 16) | self.default_id.get() as isize))
            }
            WM_CLOSE => {
                let ask = self.on_close.borrow().clone();
                if ask.is_none_or(|ask| ask()) {
                    self.destroy();
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    /// Drop the handlers (they may hold the window itself).
    pub(super) fn forget(&self) {
        self.commands.borrow_mut().clear();
        self.notifies.borrow_mut().clear();
        self.hooks.borrow_mut().clear();
        self.on_close.borrow_mut().take();
    }
}
