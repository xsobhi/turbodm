//! The 3D icons of the toolbar and the sidebar (tools/icons.py: TurboDM's tiles, with Fluent UI symbols,
//! MIT: data/icons/symbols/LICENSE), part of TurboDM, drawn smoothly scaled.

use super::d2d::{self, Engine};
use super::fallback::gdiplus;
use super::Painter;
use crate::win::wnd::px;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::null_mut;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::GdiPlus::*;
use windows::Win32::System::Com::IStream;
use windows::Win32::UI::Shell::SHCreateMemStream;

/// A PNG of TurboDM's (64 pixels square).
pub type Picture = &'static [u8];
const SOURCE: i32 = 64;

macro_rules! picture {
    ($name:literal) => {
        include_bytes!(concat!("../../../data/icons/", $name, ".png"))
    };
}

pub const ADD: Picture = picture!("add");
pub const RESUME: Picture = picture!("resume");
pub const PAUSE: Picture = picture!("pause");
pub const RESUME_ALL: Picture = picture!("resume-all");
pub const PAUSE_ALL: Picture = picture!("pause-all");
pub const DELETE: Picture = picture!("delete");
pub const CLEAR: Picture = picture!("clear");
pub const FOLDER: Picture = picture!("folder");
pub const OPTIONS: Picture = picture!("options");
pub const ALL: Picture = picture!("all");
pub const DOWNLOADING: Picture = picture!("downloading");
pub const UNFINISHED: Picture = picture!("unfinished");
pub const COMPLETED: Picture = picture!("completed");
pub const VIDEO: Picture = picture!("video");
pub const MUSIC: Picture = picture!("music");
pub const DOCUMENTS: Picture = picture!("documents");
pub const COMPRESSED: Picture = picture!("compressed");
pub const PROGRAMS: Picture = picture!("programs");
pub const IMAGES: Picture = picture!("images");
pub const OTHER: Picture = picture!("other");

/// The picture, decoded once (with the stream it reads from, which must stay).
fn bitmap(png: Picture) -> Option<*mut GpBitmap> {
    thread_local! {
        static DECODED: RefCell<HashMap<usize, (IStream, usize)>> = RefCell::default();
    }
    let key = png.as_ptr() as usize;
    if let Some(bitmap) = DECODED.with(|d| d.borrow().get(&key).map(|(_, b)| *b)) {
        return Some(bitmap as *mut GpBitmap);
    }
    if !gdiplus() {
        return None;
    }
    // SAFETY: decodes our PNG from memory; both stay for the program's life
    unsafe {
        let stream = SHCreateMemStream(Some(png))?;
        let mut bitmap = null_mut();
        if GdipCreateBitmapFromStream(&stream, &mut bitmap) != Ok {
            return None;
        }
        DECODED.with(|d| d.borrow_mut().insert(key, (stream, bitmap as usize)));
        Some(bitmap)
    }
}

/// GDI+'s 32-bit premultiplied pixels (PixelFormat32bppPARGB).
const PARGB: i32 = 0x000E_200B;

impl Painter {
    /// A picture, `size` pixels (96 DPI) in the middle of `r`; grey and faint when `faded`.
    pub fn picture(&self, r: RECT, png: Picture, size: i32, faded: bool) {
        let size = px(size);
        let (x, y) = (r.left + (r.right - r.left - size) / 2, r.top + (r.bottom - r.top - size) / 2);
        let to = RECT { left: x, top: y, right: x + size, bottom: y + size };
        if self.pass() && d2d::with(|e| {
            let bitmap = scaled(e, png, size, faded)?;
            // SAFETY: drawing during the painter's pass, one pixel for one
            unsafe { e.target.DrawBitmap(&bitmap, Some(&d2d::rect_f(to)), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None) };
            Some(())
        }).is_some() {
            return;
        }
        let Some(source) = bitmap(png) else { return };
        // SAFETY: GDI+ objects made and freed here, drawing into our DC
        unsafe {
            let mut graphics = null_mut();
            if GdipCreateFromHDC(self.gdi(), &mut graphics) == Ok {
                draw_scaled(graphics, source, to, faded);
                GdipDeleteGraphics(graphics);
            }
        }
    }
}

/// Draw the source picture into `to`, smoothly scaled; grey and faint when `faded`.
unsafe fn draw_scaled(graphics: *mut GpGraphics, source: *mut GpBitmap, to: RECT, faded: bool) {
    // SAFETY: the caller's live GDI+ objects; ours made and freed here
    unsafe {
        GdipSetInterpolationMode(graphics, InterpolationModeHighQualityBicubic);
        GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf);
        let mut attributes = null_mut();
        if faded {
            // grey (by brightness), at 40%
            #[rustfmt::skip]
            let grey = ColorMatrix { m: [0.3, 0.3, 0.3, 0.0, 0.0,
                                         0.59, 0.59, 0.59, 0.0, 0.0,
                                         0.11, 0.11, 0.11, 0.0, 0.0,
                                         0.0, 0.0, 0.0, 0.4, 0.0,
                                         0.0, 0.0, 0.0, 0.0, 1.0] };
            GdipCreateImageAttributes(&mut attributes);
            GdipSetImageAttributesColorMatrix(attributes, ColorAdjustTypeBitmap, true, &grey, null_mut(), ColorMatrixFlagsDefault);
        }
        GdipDrawImageRectRectI(graphics, source.cast(), to.left, to.top, to.right - to.left, to.bottom - to.top,
                               0, 0, SOURCE, SOURCE, UnitPixel, attributes, 0, null_mut());
        if !attributes.is_null() {
            GdipDisposeImageAttributes(attributes);
        }
    }
}

/// The picture at `size` pixels as a Direct2D bitmap: scaled smoothly (GDI+) once, then kept.
fn scaled(e: &Engine, png: Picture, size: i32, faded: bool) -> Option<ID2D1Bitmap> {
    let key = (png.as_ptr() as usize, size, faded);
    if let Some(bitmap) = e.bitmaps.borrow().get(&key) {
        return Some(bitmap.clone());
    }
    let source = bitmap(png)?;
    // SAFETY: GDI+ and Direct2D objects made here; the pixels are copied before they're freed
    unsafe {
        let mut canvas = null_mut();
        if GdipCreateBitmapFromScan0(size, size, 0, PARGB, None, &mut canvas) != Ok {
            return None;
        }
        let mut graphics = null_mut();
        if GdipGetImageGraphicsContext(canvas.cast(), &mut graphics) == Ok {
            draw_scaled(graphics, source, RECT { left: 0, top: 0, right: size, bottom: size }, faded);
            GdipDeleteGraphics(graphics);
        }
        let mut pixels = BitmapData::default();
        let area = Rect { X: 0, Y: 0, Width: size, Height: size };
        let mut made = None;
        if GdipBitmapLockBits(canvas, &area, ImageLockModeRead.0 as u32, PARGB, &mut pixels) == Ok {
            let format = D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED };
            let properties = D2D1_BITMAP_PROPERTIES { pixelFormat: format, dpiX: 96.0, dpiY: 96.0 };
            made = e.target.CreateBitmap(D2D_SIZE_U { width: size as u32, height: size as u32 }, Some(pixels.Scan0),
                                         pixels.Stride as u32, &properties).ok();
            GdipBitmapUnlockBits(canvas, &mut pixels);
        }
        GdipDisposeImage(canvas.cast());
        let made = made?;
        e.bitmaps.borrow_mut().insert(key, made.clone());
        Some(made)
    }
}
