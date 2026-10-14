//! Drawing without Direct2D (it couldn't start): rounded shapes with GDI+.

use std::ptr::null_mut;
use std::sync::OnceLock;
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::Graphics::GdiPlus::*;

/// A rounded rectangle with smooth edges: filled, and/or outlined with a 1-pixel line.
pub fn round(dc: HDC, r: RECT, radius: f32, fill: Option<COLORREF>, line: Option<COLORREF>) {
    if !gdiplus() {
        if let Some(color) = fill.or(line) {
            // SAFETY: drawing into our DC
            unsafe { windows::Win32::Graphics::Gdi::FillRect(dc, &r, super::brush(color)) };
        }
        return;
    }
    let (x, y, w, h) = (r.left as f32, r.top as f32, (r.right - r.left) as f32, (r.bottom - r.top) as f32);
    // SAFETY: GDI+ objects made and freed here, drawing into our DC
    unsafe {
        let mut graphics = null_mut();
        if GdipCreateFromHDC(dc, &mut graphics) != Ok {
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
pub fn gdiplus() -> bool {
    static STARTED: OnceLock<bool> = OnceLock::new();
    *STARTED.get_or_init(|| {
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        let mut token = 0usize;
        // SAFETY: starts GDI+ for the program's lifetime
        unsafe { GdiplusStartup(&mut token, &input, null_mut()) == Ok }
    })
}
