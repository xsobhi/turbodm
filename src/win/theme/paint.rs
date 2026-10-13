//! Drawing TurboDM's own controls: double-buffered (nothing flickers while downloads update
//! many times a second), with smooth rounded shapes (GDI+) and Windows' ClearType text.

use super::{brush, font, Font};
use std::ptr::null_mut;
use std::sync::OnceLock;
use windows::Win32::Foundation::{COLORREF, HWND, RECT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::GdiPlus::*;
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
}

impl Painter {
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
            draw(&Painter { dc: buffer, rect });
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
            draw(&Painter { dc, rect });
            let _ = RestoreDC(dc, saved);
        }
    }

    pub fn fill(&self, r: RECT, color: COLORREF) {
        // SAFETY: drawing into our DC
        unsafe { FillRect(self.dc, &r, brush(color)) };
    }

    /// A rounded rectangle with smooth edges: filled, and/or outlined with a 1-pixel line.
    pub fn round(&self, r: RECT, radius: i32, fill: Option<COLORREF>, line: Option<COLORREF>) {
        if r.right <= r.left || r.bottom <= r.top {
            return;
        }
        if !gdiplus() {
            if let Some(color) = fill.or(line) {
                self.fill(r, color);
            }
            return;
        }
        let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
        let radius = (radius as f32).min(w / 2.0).min(h / 2.0);
        // SAFETY: GDI+ objects made and freed here, drawing into our DC
        unsafe {
            let mut graphics = null_mut();
            if GdipCreateFromHDC(self.dc, &mut graphics) != Ok {
                return;
            }
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf);
            if let Some(color) = fill {
                let path = rounded(x, y, w, h, radius);
                let mut solid = null_mut();
                GdipCreateSolidFill(argb(color), &mut solid);
                GdipFillPath(graphics, solid.cast(), path);
                GdipDeleteBrush(solid.cast());
                GdipDeletePath(path);
            }
            if let Some(color) = line {
                let path = rounded(x + 0.5, y + 0.5, w - 1.0, h - 1.0, (radius - 0.5).max(0.0));
                let mut pen = null_mut();
                GdipCreatePen1(argb(color), 1.0, UnitPixel, &mut pen);
                GdipDrawPath(graphics, pen, path);
                GdipDeletePen(pen);
                GdipDeletePath(path);
            }
            GdipDeleteGraphics(graphics);
        }
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

    /// Text wrapped onto as many lines as it takes, from the top of `r`.
    pub fn paragraph(&self, r: RECT, text: &str, which: Font, color: COLORREF) {
        self.draw_text(r, text, which, color, DT_WORDBREAK | DT_LEFT | DT_NOPREFIX);
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
        if super::text::draw(self.dc, r, text, which, color, flags) {
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

    /// How wide `text` is in `which` (pixels).
    pub fn width(&self, text: &str, which: Font) -> i32 {
        text_width(self.dc, text, which)
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

fn rounded(x: f32, y: f32, w: f32, h: f32, radius: f32) -> *mut GpPath {
    let d = radius * 2.0;
    let mut path = null_mut();
    // SAFETY: a new path of four corner arcs; the caller frees it
    unsafe {
        GdipCreatePath(FillModeAlternate, &mut path);
        if d <= 0.0 {
            GdipAddPathRectangle(path, x, y, w, h);
            return path;
        }
        GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(path);
    }
    path
}

fn argb(color: COLORREF) -> u32 {
    let c = color.0;
    0xff00_0000 | ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff)
}

/// GDI+ is running (started once).
pub(super) fn gdiplus() -> bool {
    static STARTED: OnceLock<bool> = OnceLock::new();
    *STARTED.get_or_init(|| {
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        let mut token = 0usize;
        // SAFETY: starts GDI+ for the program's lifetime
        unsafe { GdiplusStartup(&mut token, &input, null_mut()) == Ok }
    })
}
