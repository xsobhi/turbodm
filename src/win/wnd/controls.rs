//! Windows' controls: creating them on a window, their text, state and font.

use super::look::{self, mark};
use super::labels::Kind;
use super::popup::{At, Item, Options, Outcome};
use super::{field, instance, Window};
use crate::win::theme::{self, Font};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::OnceLock;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, HFONT};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 96-DPI pixels to this screen's.
pub fn px(v: i32) -> i32 {
    static DPI: OnceLock<i32> = OnceLock::new();
    // SAFETY: plain query
    let dpi = *DPI.get_or_init(|| unsafe { GetDpiForSystem() } as i32);
    (v * dpi + 48) / 96
}

pub fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    // SAFETY: messages to our own controls, with arguments their documentation asks for
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))).0 }
}

pub fn set_font(hwnd: HWND, f: HFONT) {
    send(hwnd, WM_SETFONT, f.0 as usize, 1);
}

impl Window {
    /// A child control of `class`; returns it and its id.
    pub fn control(&self, class: PCWSTR, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE) -> (HWND, u16) {
        let id = self.next_id();
        // SAFETY: a child of our window; Windows owns and destroys it with the window
        let hwnd = unsafe {
            CreateWindowExW(ex, class, &HSTRING::from(text), WS_CHILD | WS_VISIBLE | style, 0, 0, 0, 0,
                            Some(self.hwnd()), Some(HMENU(id as usize as *mut _)), Some(instance()), None)
        }.unwrap_or_default();
        set_font(hwnd, theme::font(Font::Body));
        (hwnd, id)
    }

    /// One line of text (labels are drawn by TurboDM: see labels.rs).
    pub fn label(&self, text: &str) -> HWND {
        self.text_label(text, Font::Body, Kind::Line)
    }

    /// Text that wraps onto more lines.
    pub fn note(&self, text: &str) -> HWND {
        self.text_label(text, Font::Body, Kind::Paragraph)
    }

    /// A label against the right edge (a form's names, before their fields, as on Linux).
    pub fn name(&self, text: &str) -> HWND {
        self.text_label(text, Font::Body, Kind::Right)
    }

    pub fn heading(&self, text: &str) -> HWND {
        self.text_label(text, Font::Title, Kind::Line)
    }

    pub fn button(&self, text: &str, on_click: impl Fn() + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_BUTTONW, text, WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0));
        self.on_command(id, move |code| if code == BN_CLICKED { on_click() });
        hwnd
    }

    /// The button Enter presses, in the accent colour.
    pub fn default_button(&self, text: &str, on_click: impl Fn() + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_BUTTONW, text, WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), WINDOW_EX_STYLE(0));
        self.on_command(id, move |code| if code == BN_CLICKED { on_click() });
        self.set_default(id);
        mark(hwnd, look::PRIMARY);
        hwnd
    }

    /// A small borderless button showing an icon (Windows' icon font).
    pub fn icon_button(&self, glyph: char, on_click: impl Fn() + 'static) -> HWND {
        let hwnd = self.button(&glyph.to_string(), on_click);
        mark(hwnd, look::FLAT);
        mark(hwnd, look::ICON);
        hwnd
    }

    pub fn checkbox(&self, text: &str, checked: bool, on_toggle: impl Fn(bool) + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_BUTTONW, text, WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0));
        set_checked(hwnd, checked);
        self.on_command(id, move |code| if code == BN_CLICKED { on_toggle(is_checked(hwnd)) });
        hwnd
    }

    /// A switch (a check box drawn as GTK's switches), for settings.
    pub fn switch(&self, on: bool, on_toggle: impl Fn(bool) + 'static) -> HWND {
        let hwnd = self.checkbox("", on, on_toggle);
        mark(hwnd, look::SWITCH);
        hwnd
    }

    /// A text field (see field.rs).
    pub fn edit(&self, text: &str, on_change: impl Fn() + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_EDITW, text, WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WINDOW_EX_STYLE(0));
        self.on_command(id, move |code| if code == EN_CHANGE { on_change() });
        field::make_field(hwnd, vec![]);
        hwnd
    }

    /// A number field with − and + buttons, like the Linux one's.
    pub fn spin(&self, min: i32, max: i32, value: i32, on_change: impl Fn(i32) + 'static) -> HWND {
        let (edit, id) = self.control(WC_EDITW, &value.to_string(), WS_TABSTOP | WINDOW_STYLE(ES_NUMBER as u32), WINDOW_EX_STYLE(0));
        let step = move |by: i32| {
            let now: i32 = text(edit).parse().unwrap_or(min);
            set_text(edit, &now.saturating_add(by).clamp(min, max).to_string()); // its EN_CHANGE tells
        };
        let buttons = [(theme::icon::REMOVE, -1), (theme::icon::ADD, 1)].map(|(glyph, by)| self.icon_button(glyph, move || step(by)));
        self.on_command(id, move |code| if code == EN_CHANGE { on_change(text(edit).parse().unwrap_or(min).clamp(min, max)) });
        field::make_field(edit, buttons.to_vec());
        edit
    }

    /// A drop-down, as GTK's on Linux: a button showing the choice, which opens the list.
    pub fn choice(&self, items: &[&str], selected: usize, on_pick: impl Fn(usize) + 'static) -> HWND {
        let items: Vec<String> = items.iter().map(|s| s.to_string()).collect();
        let (picked, button, owner) = (Rc::new(Cell::new(selected)), Rc::new(Cell::new(HWND::default())), self.hwnd());
        let (p, b, list) = (picked.clone(), button.clone(), items.clone());
        let hwnd = self.button(&items[selected.min(items.len() - 1)], move || {
            let mut r = RECT::default();
            // SAFETY: plain query
            unsafe { let _ = GetWindowRect(b.get(), &mut r); }
            // a menu of the choices under the button, as wide; the one picked ticked
            let lines: Vec<Item> = list.iter().enumerate()
                .map(|(i, item)| Item::Tick(i as u16 + 1, item.replace('&', "&&"), i == p.get())).collect();
            let below = RECT { top: r.top - px(2), bottom: r.bottom + px(2), ..r };
            let options = Options { min_width: r.right - r.left, ..Default::default() };
            let chosen = match super::popup::show(owner, &lines, At::Below(below), options) {
                Outcome::Chose(id) => id as usize,
                _ => 0,
            };
            if let Some((i, item)) = chosen.checked_sub(1).and_then(|i| list.get(i).map(|item| (i, item))) {
                p.set(i);
                set_text(b.get(), item);
                on_pick(i);
            }
        });
        button.set(hwnd);
        mark(hwnd, look::DROPDOWN);
        hwnd
    }
}

pub fn set_text(hwnd: HWND, value: &str) {
    if text(hwnd) != value {
        // SAFETY: plain calls on our control (drawn again: our labels draw themselves)
        unsafe {
            let _ = SetWindowTextW(hwnd, &HSTRING::from(value));
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

pub fn text(hwnd: HWND) -> String {
    // SAFETY: the buffer is as long as we say
    unsafe {
        let mut buf = vec![0u16; GetWindowTextLengthW(hwnd) as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

pub fn is_checked(hwnd: HWND) -> bool {
    send(hwnd, BM_GETCHECK, 0, 0) == 1
}

pub fn set_checked(hwnd: HWND, on: bool) {
    send(hwnd, BM_SETCHECK, on as usize, 0);
}
