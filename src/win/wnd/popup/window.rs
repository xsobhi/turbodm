//! A menu's window: a layered pop-up (its pixels have their own transparency: the round corners,
//! the shadow around) that never takes the keyboard or activates. What it shows is drawn into a
//! bitmap of its own (`Surface::show`), which Windows puts on the screen as it is. (Pictures of
//! it: PrintWindow's PW_RENDERFULLCONTENT, on black; or tests.rs, as it is.)

use super::layout::{RADIUS, SHADOW};
use crate::win::theme::Painter;
use crate::win::wnd::{instance, px};
use std::cell::Cell;
use std::sync::Once;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    /// Windows asked the menus to close (another window took the mouse, TurboDM lost the focus).
    pub static CANCELLED: Cell<bool> = const { Cell::new(false) };
}

/// A menu's window and the bitmap it shows.
pub struct Surface {
    pub hwnd: HWND,
    pub body: RECT, // the menu itself, on the screen (the window is bigger: its shadow)
    margin: i32,
    size: (i32, i32),
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    mask: Vec<(u8, u8)>, // each pixel: how much of it is the menu's (rounded corners), its shadow
}

impl Surface {
    pub fn new(owner: HWND, body: RECT) -> Surface {
        register();
        let margin = px(SHADOW);
        let (width, height) = (body.right - body.left + 2 * margin, body.bottom - body.top + 2 * margin);
        let ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        // SAFETY: a pop-up owned by our window, and a bitmap of its size (both freed in Drop)
        unsafe {
            let hwnd = CreateWindowExW(ex, CLASS, w!("TurboDM menu"), WS_POPUP, body.left - margin, body.top - margin,
                                       width, height, Some(owner), None, Some(instance()), None).unwrap_or_default();
            let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: width, biHeight: -height, // top down
                biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut bits = std::ptr::null_mut();
            let dc = CreateCompatibleDC(None);
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
            let old = SelectObject(dc, bitmap.into());
            let mask = mask(width, height, margin, px(RADIUS) as f32);
            Surface { hwnd, body, margin, size: (width, height), dc, bitmap, old, bits: bits.cast(), mask }
        }
    }

    /// Draw it with `paint` (the menu's top left is at `origin` in the painter) and show it.
    pub fn show(&self, paint: impl FnOnce(&Painter, POINT)) {
        let (width, height) = self.size;
        if self.bits.is_null() {
            return;
        }
        let whole = RECT { left: 0, top: 0, right: width, bottom: height };
        Painter::over(self.dc, whole, |p| paint(p, POINT { x: self.margin, y: self.margin }));
        // SAFETY: the bitmap's pixels, `width` × `height` of them, once GDI is done with them
        unsafe {
            let _ = GdiFlush();
            let pixels = std::slice::from_raw_parts_mut(self.bits, (width * height) as usize);
            for (pixel, &(cover, shadow)) in pixels.iter_mut().zip(&self.mask) {
                *pixel = blend(*pixel, cover, shadow);
            }
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8,
                                        ..Default::default() };
            let (at, size, origin) = (POINT { x: self.body.left - self.margin, y: self.body.top - self.margin },
                                      SIZE { cx: width, cy: height }, POINT::default());
            let _ = UpdateLayeredWindow(self.hwnd, None, Some(&raw const at), Some(&raw const size), Some(self.dc),
                                        Some(&raw const origin), Default::default(), Some(&raw const blend), ULW_ALPHA);
            if !IsWindowVisible(self.hwnd).as_bool() {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    /// What it shows: premultiplied BGRA pixels, row by row (tests).
    #[cfg(test)]
    pub fn pixels(&self) -> (Vec<u32>, i32, i32) {
        let (width, height) = self.size;
        // SAFETY: the bitmap's pixels, as many as it has
        let pixels = unsafe { std::slice::from_raw_parts(self.bits, (width * height) as usize) };
        (pixels.to_vec(), width, height)
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: ours, made in `new`
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// A pixel drawn opaque, made `cover` of itself over `shadow` (black), premultiplied as layered
/// windows want them.
fn blend(pixel: u32, cover: u8, shadow: u8) -> u32 {
    let scale = |channel: u32| ((channel & 0xff) * cover as u32 + 127) / 255;
    let alpha = cover as u32 + (shadow as u32 * (255 - cover as u32) + 127) / 255;
    (alpha << 24) | (scale(pixel >> 16) << 16) | (scale(pixel >> 8) << 8) | scale(pixel)
}

/// For each pixel of a `width` × `height` window with the menu `margin` in from its edges: how
/// much of the pixel is the menu (its rounded corners smoothed), and the shadow's darkness there
/// (soft, a little lower than the menu, as GTK draws it).
fn mask(width: i32, height: i32, margin: i32, radius: f32) -> Vec<(u8, u8)> {
    let (w, h) = ((width - 2 * margin) as f32, (height - 2 * margin) as f32);
    let (blur, drop, darkest) = (px(5) as f32, px(2) as f32, 0.2);
    // distance from the rounded rectangle at (x0, y0), w × h (negative inside)
    let distance = |x: f32, y: f32, x0: f32, y0: f32| {
        let (qx, qy) = ((x - x0 - w / 2.0).abs() - (w / 2.0 - radius), (y - y0 - h / 2.0).abs() - (h / 2.0 - radius));
        (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - radius
    };
    let m = margin as f32;
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let cover = (0.5 - distance(cx, cy, m, m)).clamp(0.0, 1.0);
            let d = distance(cx, cy, m, m + drop);
            let shadow = darkest / (1.0 + (1.6 * d / blur).exp());
            out.push(((cover * 255.0).round() as u8, (shadow * 255.0).round() as u8));
        }
    }
    out
}

const CLASS: windows::core::PCWSTR = w!("TurboDMMenu");

fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: plain class registration with a static name and procedure
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
}

fn cancel(hwnd: HWND) {
    CANCELLED.with(|c| c.set(true));
    // SAFETY: wakes the menus' loop (track.rs), which then closes them
    unsafe { let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)); }
}

extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_CANCELMODE => cancel(hwnd),
        WM_CAPTURECHANGED if lparam.0 != hwnd.0 as isize => cancel(hwnd),
        WM_ACTIVATEAPP if wparam.0 == 0 => cancel(hwnd),
        _ => {}
    }
    // SAFETY: the default handling
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn premultiplied() {
        assert_eq!(super::blend(0x00ff8040, 255, 0), 0xffff8040); // the menu: as drawn, opaque
        assert_eq!(super::blend(0x00ff8040, 0, 51), 0x33000000); // outside: the shadow alone
        assert_eq!(super::blend(0x00ff8040, 0, 0), 0); // nothing
        assert_eq!(super::blend(0x00ffffff, 128, 0) >> 24, 128); // a corner's edge
    }

    #[test]
    fn corners_and_shadow() {
        let (w, h, margin) = (60, 50, 10);
        let mask = super::mask(w, h, margin, 4.0);
        let at = |x: i32, y: i32| mask[(y * w + x) as usize];
        assert_eq!(at(30, 25).0, 255); // inside
        assert_eq!(at(margin, 25).0, 255); // its left edge, straight
        assert_eq!(at(margin, margin).0, 0); // the very corner is round: outside it
        assert_eq!(at(margin - 1, 25).0, 0); // outside
        assert!(at(margin - 1, 25).1 > at(1, 25).1); // the shadow fades out
        assert!(at(30, h - margin).1 > at(30, margin - 1).1); // and is lower than the menu
    }
}
