//! Drawing TurboDM's own controls: double-buffered (nothing flickers while downloads update
//! many times a second), in one Direct2D pass per paint (d2d.rs): smooth shapes and text.

use super::{brush, d2d, font, Font};
use std::cell::Cell;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Draws into a device context; `rect` is the whole area (from 0, 0 when double-buffered).
pub struct Painter {
    pub dc: HDC,
    pub rect: RECT,
    drawing: Cell<bool>, // a Direct2D pass is on
}

impl Painter {
    fn new(dc: HDC, rect: RECT) -> Self {
        Painter { dc, rect, drawing: Cell::new(false) }
    }

    /// The Direct2D pass, started if it isn't on; false when Direct2D isn't there (use GDI).
    pub(super) fn pass(&self) -> bool {
        if !self.drawing.get() {
            self.drawing.set(d2d::with(|e| e.begin(self.dc, self.rect)).is_some());
        }
        self.drawing.get()
    }

    /// End the Direct2D pass: what it drew goes into the DC.
    fn finish(&self) {
        if self.drawing.replace(false) {
            d2d::with(|e| { e.end(); Some(()) });
        }
    }

    /// The DC, to draw into with GDI directly (what was drawn so far is put in it first).
    pub fn gdi(&self) -> HDC {
        self.finish();
        self.dc
    }

    /// Paint `hwnd` (on WM_PAINT): `draw` paints all of it into a buffer, shown at once.
    pub fn paint(hwnd: HWND, draw: impl FnOnce(&Painter)) {
        // SAFETY: the standard paint cycle on our window, with a memory buffer we free
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            let (width, height) = (rect.right.max(1), rect.bottom.max(1));
            let buffer = CreateCompatibleDC(Some(dc));
            let bitmap = CreateCompatibleBitmap(dc, width, height);
            let old = SelectObject(buffer, bitmap.into());
            SetBkMode(buffer, TRANSPARENT);
            let painter = Painter::new(buffer, rect);
            draw(&painter);
            painter.finish();
            let _ = BitBlt(dc, 0, 0, width, height, Some(buffer), 0, 0, SRCCOPY);
            SelectObject(buffer, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(buffer);
            let _ = EndPaint(hwnd, &ps);
        }
    }

    /// Draw straight into `dc` (a control's own drawing: custom-drawn buttons and list cells).
    pub fn over(dc: HDC, rect: RECT, draw: impl FnOnce(&Painter)) {
        // SAFETY: plain state changes on the DC we were given, put back afterwards
        unsafe {
            let saved = SaveDC(dc);
            SetBkMode(dc, TRANSPARENT);
            let painter = Painter::new(dc, rect);
            draw(&painter);
            painter.finish();
            let _ = RestoreDC(dc, saved);
        }
    }

    pub fn fill(&self, r: RECT, color: COLORREF) {
        // (during a Direct2D pass, in it; before one, GDI is quicker)
        if self.drawing.get() && d2d::with(|e| { e.fill(r, color); Some(()) }).is_some() {
            return;
        }
        // SAFETY: drawing into our DC
        unsafe { FillRect(self.dc, &r, brush(color)) };
    }

    /// A rounded rectangle with smooth edges: filled, and/or outlined with a 1-pixel line.
    pub fn round(&self, r: RECT, radius: i32, fill: Option<COLORREF>, line: Option<COLORREF>) {
        if r.right <= r.left || r.bottom <= r.top {
            return;
        }
        let radius = (radius as f32).min((r.right - r.left) as f32 / 2.0).min((r.bottom - r.top) as f32 / 2.0);
        if self.pass() && d2d::with(|e| { e.round(r, radius, fill, line); Some(()) }).is_some() {
            return;
        }
        super::fallback::round(self.dc, r, radius, fill, line);
    }

    /// One line of text in `r`, cut with "…" when too long.
    pub fn text(&self, r: RECT, text: &str, which: Font, color: COLORREF, align: Align) {
        let align = match align {
            Align::Left => DT_LEFT,
            Align::Center => DT_CENTER,
            Align::Right => DT_RIGHT,
        };
        self.draw_text(r, text, which, color, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX | align);
    }

    /// An icon from Windows' icon font, `size` pixels (96 DPI), in the middle of `r`.
    pub fn icon(&self, r: RECT, glyph: char, size: i32, color: COLORREF) {
        self.text(r, &glyph.to_string(), Font::Icons(size), color, Align::Center);
    }

    /// Text with Windows' own `flags` (DT_…).
    pub fn draw_text(&self, r: RECT, text: &str, which: Font, color: COLORREF, flags: DRAW_TEXT_FORMAT) {
        if text.is_empty() {
            return; // (Windows reads the text even when told it's empty: an empty Vec has no real address)
        }
        if self.pass() && super::text::draw(r, text, which, color, flags) {
            return; // smooth, as on Linux; GDI below only if DirectWrite isn't there
        }
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut r = r;
        // SAFETY: drawing into our DC; the font is put back
        unsafe {
            let old = SelectObject(self.dc, font(which).into());
            SetTextColor(self.dc, color);
            DrawTextW(self.dc, &mut wide, &mut r, flags);
            SelectObject(self.dc, old);
        }
    }

}

/// How wide `text` is in `which` on the screen (pixels): for laying out before drawing.
pub fn measure(text: &str, which: Font) -> i32 {
    // SAFETY: the screen's DC, released after
    unsafe {
        let dc = GetDC(None);
        let width = text_width(dc, text, which);
        ReleaseDC(None, dc);
        width
    }
}

pub fn text_width(dc: HDC, text: &str, which: Font) -> i32 {
    if text.is_empty() {
        return 0;
    }
    if let Some(width) = super::text::width(text, which) {
        return width;
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut size = SIZE::default();
    // SAFETY: a measurement in our DC; the font is put back
    unsafe {
        let old = SelectObject(dc, font(which).into());
        let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
        SelectObject(dc, old);
    }
    size.cx
}
