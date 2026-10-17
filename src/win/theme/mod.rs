//! How TurboDM looks on Windows: at first like it does on Linux Mint (GTK with the Mint-Y theme):
//! flat light greys around white content, crisp 1-pixel lines, small corners, Mint's green for
//! what's chosen, Mint's font (Ubuntu), and symbolic icons from Windows' own icon font (Segoe
//! Fluent Icons, or Segoe MDL2 Assets on Windows 10). Or another look (themes.rs: Options → Look):
//! everything asks the look in use for its colours as it draws.

mod d2d;
mod fallback;
mod paint;
pub mod pictures;
mod text;
pub mod themes;

pub use d2d::stop;
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

pub const WHITE: COLORREF = rgb(0xffffff);

// The look in use's colours (themes.rs: Palette says what each is for)
macro_rules! colors {
    ($($name:ident),*) => {
        $(pub fn $name() -> COLORREF { themes::current().palette.$name })*
    };
}
colors!(chrome, view, border, edge, separator, text, dim, disabled, accent, on_accent, button, button_hover,
        button_pressed, button_border, entry_border, track, green, danger);

/// Corners of buttons, fields and highlights (96-DPI pixels): small, as Mint's.
pub fn radius() -> i32 {
    themes::current().radius
}

/// What's chosen (a row, a sidebar entry, a menu item): the accent colour, or (Windows' look)
/// a tint of it on the content's colour.
pub fn selected() -> COLORREF {
    let theme = themes::current();
    match theme.selection {
        themes::Selection::Filled => theme.palette.accent,
        themes::Selection::Subtle => mix(theme.palette.view, theme.palette.accent, if theme.dark() { 0.3 } else { 0.16 }),
    }
}

/// Text on what's chosen; `dim` for its small print.
pub fn on_selected(dim: bool) -> COLORREF {
    let theme = themes::current();
    match (theme.selection, dim) {
        (themes::Selection::Filled, _) => theme.palette.on_accent,
        (themes::Selection::Subtle, false) => theme.palette.text,
        (themes::Selection::Subtle, true) => theme.palette.dim,
    }
}

/// Use the look `id` with the user's colours over it (Options → Look), from now on; returns the
/// look's palette until now. Windows already open take it with `wnd::retheme`.
pub fn apply(id: &str, own: &turbodm::config::ThemeColors) -> themes::Palette {
    let own = themes::Own { accent: own.accent, window: own.window, content: own.content, text: own.text };
    let before = themes::set(themes::resolve(id, own));
    d2d::forget_formats(); // (its font may be another)
    before.palette
}

/// Between `a` (0.0) and `b` (1.0).
pub fn mix(a: COLORREF, b: COLORREF, t: f64) -> COLORREF {
    let channel = |shift: u32| {
        let (x, y) = (((a.0 >> shift) & 0xff) as f64, ((b.0 >> shift) & 0xff) as f64);
        ((x + (y - x) * t).round() as u32) << shift
    };
    COLORREF(channel(0) | channel(8) | channel(16))
}

/// What the mouse is on (a row, a toolbar button, a tab), over `bg`: a little darker, or on a
/// dark colour a little lighter.
pub fn hover(bg: COLORREF) -> COLORREF {
    if themes::is_dark(bg) { mix(bg, WHITE, 0.07) } else { mix(bg, COLORREF(0), 0.05) }
}

/// …while it's pressed.
pub fn pressed(bg: COLORREF) -> COLORREF {
    if themes::is_dark(bg) { mix(bg, WHITE, 0.13) } else { mix(bg, COLORREF(0), 0.1) }
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

/// The look's font: Ubuntu (if TurboDM's copy loads), or Windows' own Segoe UI.
fn text_face() -> &'static str {
    static UBUNTU_LOADED: OnceLock<bool> = OnceLock::new();
    let ubuntu = *UBUNTU_LOADED.get_or_init(|| {
        let mut loaded = 0;
        for data in UBUNTU {
            let mut count = 0u32;
            // SAFETY: the font data is part of the program, so it outlives the fonts made from it
            let handle = unsafe { AddFontMemResourceEx(data.as_ptr().cast(), data.len() as u32, None, (&raw mut count).cast_const()) };
            loaded += (!handle.is_invalid()) as usize;
        }
        loaded == UBUNTU.len()
    });
    if ubuntu && themes::current().face == themes::Face::Ubuntu { "Ubuntu" } else { "Segoe UI" }
}

thread_local! {
    // the fonts made, by kind and face (all kept: controls may still use one of another look's)
    static FONTS: RefCell<HashMap<(Font, &'static str), isize>> = RefCell::default();
}

pub fn font(which: Font) -> HFONT {
    let key = (which, if matches!(which, Font::Icons(_)) { icon_face() } else { text_face() });
    let handle = FONTS.with(|fonts| *fonts.borrow_mut().entry(key).or_insert_with(|| create(which, key.1)));
    HFONT(handle as *mut _)
}

/// The look's font of the kind `old` is (another look's): for controls, when the look changes.
pub fn refont(old: HFONT) -> Option<HFONT> {
    let kind = FONTS.with(|fonts| fonts.borrow().iter().find(|(_, h)| **h == old.0 as isize).map(|((kind, _), _)| *kind))?;
    Some(font(kind))
}

fn create(which: Font, face: &str) -> isize {
    let (pixels, bold) = match which {
        Font::Body => (13, false),
        Font::Strong => (13, true),
        Font::Caption => (12, false),
        Font::CaptionStrong => (12, true),
        Font::Title => (19, true),
        Font::Icons(size) => (size, false),
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

/// Small line icons from Windows' icon font, for buttons (the toolbar's and the sidebar's are
/// `pictures`).
pub mod icon {
    pub const ADD: char = '\u{E710}';
    pub const REMOVE: char = '\u{E738}'; // minus
    pub const CHECK: char = '\u{E73E}';
    pub const FOLDER_OPEN: char = '\u{ED25}';
    pub const OPEN_FILE: char = '\u{E8E5}';
    pub const CHEVRON_DOWN: char = '\u{E70D}';
}
