//! Windows' controls: creating them on a window, their text, state and font.

use super::{instance, Window};
use std::sync::OnceLock;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, HFONT, FW_SEMIBOLD};
use windows::Win32::System::SystemServices::{SS_ENDELLIPSIS, SS_LEFT, SS_NOPREFIX};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 96-DPI pixels to this screen's.
pub fn px(v: i32) -> i32 {
    static DPI: OnceLock<i32> = OnceLock::new();
    // SAFETY: plain query
    let dpi = *DPI.get_or_init(|| unsafe { GetDpiForSystem() } as i32);
    (v * dpi + 48) / 96
}

/// Windows' dialog font (Segoe UI on Windows 7 and later), and a bigger one for headings.
pub fn font(heading: bool) -> HFONT {
    static FONTS: OnceLock<(isize, isize)> = OnceLock::new();
    let (normal, big) = *FONTS.get_or_init(|| unsafe {
        // SAFETY: plain queries; the fonts live as long as the program
        let mut metrics = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let _ = SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, metrics.cbSize, Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()),
                                      SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let mut logfont = metrics.lfMessageFont;
        let normal = CreateFontIndirectW(&logfont);
        logfont.lfHeight = logfont.lfHeight * 4 / 3;
        logfont.lfWeight = FW_SEMIBOLD.0 as i32;
        (normal.0 as isize, CreateFontIndirectW(&logfont).0 as isize)
    });
    HFONT((if heading { big } else { normal }) as *mut _)
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
        set_font(hwnd, font(false));
        (hwnd, id)
    }

    pub fn label(&self, text: &str) -> HWND {
        self.control(WC_STATICW, text, WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0 | SS_ENDELLIPSIS.0), WINDOW_EX_STYLE(0)).0
    }

    /// Text that wraps onto more lines.
    pub fn note(&self, text: &str) -> HWND {
        self.control(WC_STATICW, text, WINDOW_STYLE(SS_LEFT.0 | SS_NOPREFIX.0), WINDOW_EX_STYLE(0)).0
    }

    pub fn heading(&self, text: &str) -> HWND {
        let label = self.label(text);
        set_font(label, font(true));
        label
    }

    pub fn button(&self, text: &str, on_click: impl Fn() + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_BUTTONW, text, WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32), WINDOW_EX_STYLE(0));
        self.on_command(id, move |code| if code == BN_CLICKED { on_click() });
        hwnd
    }

    /// The button Enter presses.
    pub fn default_button(&self, text: &str, on_click: impl Fn() + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_BUTTONW, text, WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32), WINDOW_EX_STYLE(0));
        self.on_command(id, move |code| if code == BN_CLICKED { on_click() });
        self.set_default(id);
        hwnd
    }

    pub fn checkbox(&self, text: &str, checked: bool, on_toggle: impl Fn(bool) + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_BUTTONW, text, WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32), WINDOW_EX_STYLE(0));
        set_checked(hwnd, checked);
        self.on_command(id, move |code| if code == BN_CLICKED { on_toggle(is_checked(hwnd)) });
        hwnd
    }

    pub fn edit(&self, text: &str, on_change: impl Fn() + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_EDITW, text, WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), WS_EX_CLIENTEDGE);
        self.on_command(id, move |code| if code == EN_CHANGE { on_change() });
        hwnd
    }

    /// A number field with up/down arrows.
    pub fn spin(&self, min: i32, max: i32, value: i32, on_change: impl Fn(i32) + 'static) -> HWND {
        let (edit, id) = self.control(WC_EDITW, &value.to_string(), WS_TABSTOP | WINDOW_STYLE(ES_NUMBER as u32), WS_EX_CLIENTEDGE);
        let style = WINDOW_STYLE(UDS_SETBUDDYINT | UDS_ALIGNRIGHT | UDS_ARROWKEYS | UDS_NOTHOUSANDS);
        let (arrows, _) = self.control(UPDOWN_CLASSW, "", style, WINDOW_EX_STYLE(0));
        send(arrows, UDM_SETBUDDY, edit.0 as usize, 0);
        send(arrows, UDM_SETRANGE32, min as usize, max as isize);
        send(arrows, UDM_SETPOS32, 0, value as isize);
        self.on_command(id, move |code| if code == EN_CHANGE { on_change(text(edit).parse().unwrap_or(min).clamp(min, max)) });
        edit
    }

    /// A drop-down list to pick one of `items` from.
    pub fn choice(&self, items: &[&str], selected: usize, on_pick: impl Fn(usize) + 'static) -> HWND {
        let (hwnd, id) = self.control(WC_COMBOBOXW, "", WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32), WINDOW_EX_STYLE(0));
        for item in items {
            let item = HSTRING::from(*item);
            send(hwnd, CB_ADDSTRING, 0, item.as_ptr() as isize);
        }
        send(hwnd, CB_SETCURSEL, selected, 0);
        self.on_command(id, move |code| if code == CBN_SELCHANGE { on_pick(send(hwnd, CB_GETCURSEL, 0, 0).max(0) as usize) });
        hwnd
    }

    pub fn progress_bar(&self) -> HWND {
        let hwnd = self.control(PROGRESS_CLASSW, "", WINDOW_STYLE(0), WINDOW_EX_STYLE(0)).0;
        send(hwnd, PBM_SETRANGE32, 0, 1000);
        hwnd
    }
}

pub fn set_text(hwnd: HWND, value: &str) {
    if text(hwnd) != value {
        // SAFETY: plain call on our control
        unsafe { let _ = SetWindowTextW(hwnd, &HSTRING::from(value)); }
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

pub fn enable(hwnd: HWND, on: bool) {
    // SAFETY: plain call
    unsafe { let _ = EnableWindow(hwnd, on); }
}

pub fn set_visible(hwnd: HWND, on: bool) {
    let show = if on { SW_SHOW } else { SW_HIDE };
    // SAFETY: plain calls
    unsafe {
        let _ = ShowWindow(hwnd, show);
        if let Some(arrows) = super::arrows_of(hwnd) {
            let _ = ShowWindow(arrows, show);
        }
    }
}

/// 0.0–1.0 on a progress bar.
pub fn set_progress(hwnd: HWND, fraction: f64) {
    send(hwnd, PBM_SETPOS, (fraction.clamp(0.0, 1.0) * 1000.0) as usize, 0);
}

/// The window size that gives `width` × `height` of content.
pub fn outer_size(width: i32, height: i32, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE) -> (i32, i32) {
    let mut rect = RECT { left: 0, top: 0, right: width, bottom: height };
    // SAFETY: plain calculation
    unsafe { let _ = AdjustWindowRectEx(&mut rect, style, false, ex); }
    (rect.right - rect.left, rect.bottom - rect.top)
}
