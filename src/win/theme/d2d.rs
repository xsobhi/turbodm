//! Drawing with Direct2D, as modern Windows apps draw: all of a window's (or a list row's) shapes,
//! text and pictures in one pass, smoothed (antialiased shapes, DirectWrite text: the font's true
//! shapes), then shown at once. That's what keeps resizing smooth: one pass per paint, not one per
//! word. If Direct2D can't start, the painter falls back to GDI.

use super::UBUNTU;
use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::{Interface, IUnknown};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::HDC;
use windows_numerics::Matrix3x2;

pub struct Engine {
    pub factory: IDWriteFactory,
    pub ubuntu: Option<IDWriteFontCollection>, // Mint's font, private to TurboDM
    pub target: ID2D1DCRenderTarget,
    pub formats: RefCell<HashMap<super::Font, IDWriteTextFormat>>,
    brushes: RefCell<HashMap<u32, ID2D1SolidColorBrush>>,
    pub bitmaps: RefCell<HashMap<(usize, i32, bool), ID2D1Bitmap>>, // pictures, by size
}

thread_local! {
    static ENGINE: RefCell<Option<Option<Engine>>> = const { RefCell::new(None) }; // (None: not started yet)
}

pub fn with<T>(f: impl FnOnce(&Engine) -> Option<T>) -> Option<T> {
    ENGINE.with(|engine| {
        if engine.borrow().is_none() {
            let started = start();
            *engine.borrow_mut() = Some(started);
        }
        engine.borrow().as_ref()?.as_ref().and_then(f)
    })
}

/// Let Direct2D go, before TurboDM exits: its software renderer (WARP) crashes the exit if it's
/// still there while Windows ends the process. Painting goes on with GDI.
pub fn stop() {
    ENGINE.with(|engine| *engine.borrow_mut() = Some(None));
}

fn start() -> Option<Engine> {
    // SAFETY: COM objects made here and kept for the program's life
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE, // small areas, often: no trips to the GPU
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0, // one unit is one pixel: sizes are already scaled (px)
            dpiY: 96.0,
            ..Default::default()
        };
        let target = d2d.CreateDCRenderTarget(&properties).ok()?;
        // Windows' own ClearType settings, but "natural symmetric": not snapped to the pixel grid
        let system = factory.CreateRenderingParams().ok()?;
        let params = factory.CreateCustomRenderingParams(system.GetGamma(), system.GetEnhancedContrast(),
            system.GetClearTypeLevel(), system.GetPixelGeometry(), DWRITE_RENDERING_MODE_NATURAL_SYMMETRIC).ok()?;
        target.SetTextRenderingParams(&params);
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
        let ubuntu = ubuntu(&factory);
        Some(Engine { factory, ubuntu, target, formats: RefCell::default(), brushes: RefCell::default(),
                      bitmaps: RefCell::default() })
    }
}

/// The Ubuntu font, from TurboDM's own copy (DirectWrite has its own font list, not GDI's).
fn ubuntu(factory: &IDWriteFactory) -> Option<IDWriteFontCollection> {
    // SAFETY: the font data is part of the program, so it outlives the collection
    unsafe {
        let factory5: IDWriteFactory5 = factory.cast().ok()?; // Windows 10 (1703) and later
        let loader = factory5.CreateInMemoryFontFileLoader().ok()?;
        factory.RegisterFontFileLoader(&loader).ok()?;
        let builder = factory5.CreateFontSetBuilder().ok()?;
        for data in UBUNTU {
            let file = loader.CreateInMemoryFontFileReference(factory, data.as_ptr().cast(), data.len() as u32, None::<&IUnknown>).ok()?;
            builder.AddFontFile(&file).ok()?;
        }
        let set = builder.CreateFontSet().ok()?;
        factory5.CreateFontCollectionFromFontSet(&set).ok()?.cast().ok()
    }
}

pub fn rect_f(r: RECT) -> D2D_RECT_F {
    D2D_RECT_F { left: r.left as f32, top: r.top as f32, right: r.right as f32, bottom: r.bottom as f32 }
}

impl Engine {
    pub fn brush(&self, color: COLORREF) -> Option<ID2D1SolidColorBrush> {
        if let Some(brush) = self.brushes.borrow().get(&color.0) {
            return Some(brush.clone());
        }
        let channel = |shift: u32| ((color.0 >> shift) & 0xff) as f32 / 255.0;
        let rgba = D2D1_COLOR_F { r: channel(0), g: channel(8), b: channel(16), a: 1.0 };
        // SAFETY: plain object creation
        let brush = unsafe { self.target.CreateSolidColorBrush(&rgba, None).ok()? };
        self.brushes.borrow_mut().insert(color.0, brush.clone());
        Some(brush)
    }

    /// Start drawing into `dc`, within `bounds` (in its coordinates).
    pub fn begin(&self, dc: HDC, bounds: RECT) -> Option<()> {
        // SAFETY: binds our target to the DC Windows gave us; ended by `end`
        unsafe {
            self.target.BindDC(dc, &bounds).ok()?;
            self.target.BeginDraw();
            let shift = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: -bounds.left as f32, M32: -bounds.top as f32 };
            self.target.SetTransform(&shift); // draw in the DC's coordinates
        }
        Some(())
    }

    /// Show what was drawn since `begin`.
    pub fn end(&self) {
        // SAFETY: ends the drawing begun in `begin`
        if unsafe { self.target.EndDraw(None, None) }.is_err() {
            self.bitmaps.borrow_mut().clear(); // (made for the target: made again next time)
        }
    }

    pub fn fill(&self, r: RECT, color: COLORREF) {
        let Some(brush) = self.brush(color) else { return };
        // SAFETY: drawing during a session
        unsafe { self.target.FillRectangle(&rect_f(r), &brush) };
    }

    /// A rounded rectangle: filled and/or outlined (1 pixel, inside its edge).
    pub fn round(&self, r: RECT, radius: f32, fill: Option<COLORREF>, line: Option<COLORREF>) {
        let shape = |inset: f32| D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: r.left as f32 + inset, top: r.top as f32 + inset,
                               right: r.right as f32 - inset, bottom: r.bottom as f32 - inset },
            radiusX: (radius - inset).max(0.0),
            radiusY: (radius - inset).max(0.0),
        };
        // SAFETY: drawing during a session
        unsafe {
            if let Some(brush) = fill.and_then(|c| self.brush(c)) {
                self.target.FillRoundedRectangle(&shape(0.0), &brush);
            }
            if let Some(brush) = line.and_then(|c| self.brush(c)) {
                self.target.DrawRoundedRectangle(&shape(0.5), &brush, 1.0, None);
            }
        }
    }
}
