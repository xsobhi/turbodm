//! The 3D icons of the toolbar and the sidebar: Microsoft's Fluent Emoji in their 3D style (MIT,
//! see data/icons/LICENSE), part of TurboDM, drawn smoothly scaled (GDI+).

use super::paint::gdiplus;
use super::Painter;
use crate::win::wnd::px;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::null_mut;
use windows::Win32::Foundation::RECT;
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

impl Painter {
    /// A picture, `size` pixels (96 DPI) in the middle of `r`; grey and faint when `faded`.
    pub fn picture(&self, r: RECT, png: Picture, size: i32, faded: bool) {
        let Some(bitmap) = bitmap(png) else { return };
        let size = px(size);
        let (x, y) = (r.left + (r.right - r.left - size) / 2, r.top + (r.bottom - r.top - size) / 2);
        // SAFETY: GDI+ objects made and freed here, drawing into our DC
        unsafe {
            let mut graphics = null_mut();
            if GdipCreateFromHDC(self.dc, &mut graphics) != Ok {
                return;
            }
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
                GdipSetImageAttributesColorMatrix(attributes, ColorAdjustTypeBitmap, true, &grey, null_mut(),
                                                  ColorMatrixFlagsDefault);
            }
            GdipDrawImageRectRectI(graphics, bitmap.cast(), x, y, size, size, 0, 0, SOURCE, SOURCE, UnitPixel,
                                   attributes, 0, null_mut());
            if !attributes.is_null() {
                GdipDisposeImageAttributes(attributes);
            }
            GdipDeleteGraphics(graphics);
        }
    }
}
