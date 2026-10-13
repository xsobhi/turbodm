//! How TurboDM looks on Windows: like it does on Linux Mint (GTK with the Mint-Y theme): flat
//! light greys around white content, crisp 1-pixel lines, small corners, Mint's green for what's
//! chosen, Mint's font (Ubuntu), and symbolic icons from Windows' own icon font (Segoe Fluent
//! Icons, or Segoe MDL2 Assets on Windows 10).

mod paint;
pub mod pictures;
mod text;

pub use paint::{measure, Align, Painter};

use super::wnd::px;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;
use windows::Win32::Foundation::{COLORREF, LPARAM};
use windows::Win32::Graphics::Gdi::*;

/// 0xRRGGBB, as Windows wants it (0x00BBGGRR).
pub const fn rgb(hex: u32) -> COLORREF {
    COLORREF(((hex & 0xff) << 16) | (hex & 0xff00) | ((hex >> 16) & 0xff))
}

// Mint-Y (light)
pub const CHROME: COLORREF = rgb(0xefeff0); // around the content: toolbar, status line, tab row, dialogs
pub const VIEW: COLORREF = rgb(0xffffff); // the content: the list, the sidebar, the tabs' pages
pub const BORDER: COLORREF = rgb(0xd6d6da); // lines between areas
pub const SEPARATOR: COLORREF = rgb(0xededef); // between the list's rows
pub const TEXT: COLORREF = rgb(0x303030);
pub const DIM: COLORREF = rgb(0x77777c); // names of values, headings, small print
pub const DISABLED: COLORREF = rgb(0xb2b2b6);
pub const ACCENT: COLORREF = rgb(0x35a854); // Mint-Y's green
pub const WHITE: COLORREF = rgb(0xffffff);
pub const BUTTON: COLORREF = rgb(0xfafafa);
pub const BUTTON_HOVER: COLORREF = rgb(0xffffff);
pub const BUTTON_PRESSED: COLORREF = rgb(0xe4e4e7);
pub const BUTTON_BORDER: COLORREF = rgb(0xc8c8cc);
pub const ENTRY_BORDER: COLORREF = rgb(0xc8c8cc);
pub const TRACK: COLORREF = rgb(0xdedee2); // the empty part of progress bars
pub const GREEN: COLORREF = rgb(0x33d17a); // a connection at work, on the connections map
pub const DANGER: COLORREF = rgb(0xe0393e); // errors; the download disk almost full
/// Corners of buttons, fields and highlights (96-DPI pixels): small, as Mint's.
pub const RADIUS: i32 = 3;

/// Between `a` (0.0) and `b` (1.0).
pub fn mix(a: COLORREF, b: COLORREF, t: f64) -> COLORREF {
    let channel = |shift: u32| {
        let (x, y) = (((a.0 >> shift) & 0xff) as f64, ((b.0 >> shift) & 0xff) as f64);
        ((x + (y - x) * t).round() as u32) << shift
    };
    COLORREF(channel(0) | channel(8) | channel(16))
}

/// What the mouse is on (a row, a toolbar button, a tab), over `bg`.
pub fn hover(bg: COLORREF) -> COLORREF {
    mix(bg, COLORREF(0), 0.05)
}

/// …while it's pressed.
pub fn pressed(bg: COLORREF) -> COLORREF {
    mix(bg, COLORREF(0), 0.1)
}

/// Text sizes and weights, in Mint's font (Ubuntu, 10 pt); `Icons` is Windows' icon font at
/// that many pixels (96 DPI).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Font {
    Body,
    Strong,        // bold
    Caption,       // 9 pt: toolbar names, small print
    CaptionStrong, // the sidebar's headings
    Title,         // dialog headings
    Icons(i32),
}

/// Mint's font, Ubuntu (Ubuntu Font Licence, data/fonts/UFL.txt): part of TurboDM, for
/// TurboDM only (nothing is installed in Windows).
const UBUNTU: [&[u8]; 2] = [include_bytes!("../../../data/fonts/Ubuntu-Regular.ttf"),
                            include_bytes!("../../../data/fonts/Ubuntu-Bold.ttf")];

fn text_face() -> &'static str {
    static FACE: OnceLock<&str> = OnceLock::new();
    FACE.get_or_init(|| {
        let mut loaded = 0;
        for data in UBUNTU {
            let mut count = 0u32;
            // SAFETY: the font data is part of the program, so it outlives the fonts made from it
            let handle = unsafe { AddFontMemResourceEx(data.as_ptr().cast(), data.len() as u32, None, &mut count) };
            loaded += (!handle.is_invalid()) as usize;
        }
        if loaded == UBUNTU.len() { "Ubuntu" } else { "Segoe UI" }
    })
}

pub fn font(which: Font) -> HFONT {
    thread_local! {
        static FONTS: RefCell<HashMap<Font, isize>> = RefCell::default();
    }
    let handle = FONTS.with(|fonts| *fonts.borrow_mut().entry(which).or_insert_with(|| create(which)));
    HFONT(handle as *mut _)
}

fn create(which: Font) -> isize {
    let (face, pixels, bold) = match which {
        Font::Body => (text_face(), 13, false),
        Font::Strong => (text_face(), 13, true),
        Font::Caption => (text_face(), 12, false),
        Font::CaptionStrong => (text_face(), 12, true),
        Font::Title => (text_face(), 19, true),
        Font::Icons(size) => (icon_face(), size, false),
    };
    let mut logfont = LOGFONTW {
        lfHeight: -px(pixels),
        lfWeight: if bold { FW_BOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
        lfCharSet: DEFAULT_CHARSET,
        lfQuality: CLEARTYPE_QUALITY,
        ..Default::default()
    };
    for (i, c) in face.encode_utf16().take(31).enumerate() {
        logfont.lfFaceName[i] = c;
    }
    // SAFETY: a font from a filled-in description; it lives as long as the program
    unsafe { CreateFontIndirectW(&logfont).0 as isize }
}

/// Windows 11's icon font, or Windows 10's (the same icons, mostly).
fn icon_face() -> &'static str {
    static FACE: OnceLock<&str> = OnceLock::new();
    FACE.get_or_init(|| if installed("Segoe Fluent Icons") { "Segoe Fluent Icons" } else { "Segoe MDL2 Assets" })
}

fn installed(face: &str) -> bool {
    unsafe extern "system" fn found(_: *const LOGFONTW, _: *const TEXTMETRICW, _: u32, flag: LPARAM) -> i32 {
        // SAFETY: `flag` points at the bool below, alive during the enumeration
        unsafe { *(flag.0 as *mut bool) = true };
        0
    }
    let mut logfont = LOGFONTW { lfCharSet: DEFAULT_CHARSET, ..Default::default() };
    for (i, c) in face.encode_utf16().take(31).enumerate() {
        logfont.lfFaceName[i] = c;
    }
    let mut yes = false;
    // SAFETY: enumerates the fonts by that name into `yes`
    unsafe {
        let dc = GetDC(None);
        EnumFontFamiliesExW(dc, &logfont, Some(found), LPARAM(&mut yes as *mut bool as isize), 0);
        ReleaseDC(None, dc);
    }
    yes
}

/// A brush of `color` (kept: there are only a few colours).
pub fn brush(color: COLORREF) -> HBRUSH {
    thread_local! {
        static BRUSHES: RefCell<HashMap<u32, isize>> = RefCell::default();
    }
    // SAFETY: a plain solid brush; it lives as long as the program
    let handle = BRUSHES.with(|b| *b.borrow_mut().entry(color.0).or_insert_with(|| unsafe { CreateSolidBrush(color).0 as isize }));
    HBRUSH(handle as *mut _)
}

/// Icons from Windows' icon font: the same pictures as the Linux look's Fluent icons.
pub mod icon {
    pub const MENU: char = '\u{E700}';
    pub const ADD: char = '\u{E710}';
    pub const REMOVE: char = '\u{E738}'; // minus
    pub const SETTINGS: char = '\u{E713}';
    pub const VIDEO: char = '\u{E714}';
    pub const STOP: char = '\u{E71A}';
    pub const SEARCH: char = '\u{E721}';
    pub const CHECK: char = '\u{E73E}';
    pub const DELETE: char = '\u{E74D}';
    pub const PLAY: char = '\u{E768}';
    pub const PAUSE: char = '\u{E769}';
    pub const DOCUMENT: char = '\u{E8A5}';
    pub const DOWNLOAD: char = '\u{E896}';
    pub const MUSIC: char = '\u{EC4F}';
    pub const IMAGE: char = '\u{EB9F}';
    pub const PACKAGE: char = '\u{E7B8}';
    pub const APPS: char = '\u{ECAA}';
    pub const FAST_FORWARD: char = '\u{EB9D}';
    pub const BROOM: char = '\u{EA99}';
    pub const PAGE: char = '\u{E7C3}';
    pub const FOLDER_OPEN: char = '\u{ED25}';
    pub const CHEVRON_DOWN: char = '\u{E70D}';
}
