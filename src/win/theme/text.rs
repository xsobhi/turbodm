//! Text drawn with DirectWrite, as modern Windows apps draw it (browsers, VS Code…): letters keep
//! the font's true shapes and sit at fractional positions, smoothed as on Linux, Macs and phones.
//! (Windows' classic text drawing, GDI, snaps them to the pixel grid: crisp, but pixelated.)
//! If DirectWrite can't start, the painter falls back to GDI.

use super::{icon_face, Font, UBUNTU};
use crate::win::wnd::px;
use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use windows::core::{w, Interface, HSTRING, IUnknown};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows_numerics::Vector2;

struct Engine {
    factory: IDWriteFactory,
    ubuntu: Option<IDWriteFontCollection>, // Mint's font, private to TurboDM
    target: ID2D1DCRenderTarget,
    formats: RefCell<HashMap<Font, IDWriteTextFormat>>,
    brushes: RefCell<HashMap<u32, ID2D1SolidColorBrush>>,
}

thread_local! {
    static ENGINE: OnceCell<Option<Engine>> = const { OnceCell::new() };
}

fn with<T>(f: impl FnOnce(&Engine) -> Option<T>) -> Option<T> {
    ENGINE.with(|engine| engine.get_or_init(start).as_ref().and_then(f))
}

fn start() -> Option<Engine> {
    // SAFETY: COM objects made here and kept for the program's life
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE, // small pieces of text: no trips to the GPU
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
        Some(Engine { factory, ubuntu, target, formats: RefCell::default(), brushes: RefCell::default() })
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

impl Engine {
    fn format(&self, which: Font) -> Option<IDWriteTextFormat> {
        if let Some(format) = self.formats.borrow().get(&which) {
            return Some(format.clone());
        }
        let (pixels, bold) = match which {
            Font::Body => (13, false),
            Font::Strong => (13, true),
            Font::Caption => (12, false),
            Font::CaptionStrong => (12, true),
            Font::Title => (19, true),
            Font::Icons(size) => (size, false),
        };
        let (family, collection) = match (which, &self.ubuntu) {
            (Font::Icons(_), _) => (icon_face(), None),
            (_, Some(ubuntu)) => ("Ubuntu", Some(ubuntu)),
            (_, None) => ("Segoe UI", None),
        };
        let weight = if bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
        // SAFETY: plain object creation
        let format = unsafe {
            self.factory.CreateTextFormat(&HSTRING::from(family), collection, weight, DWRITE_FONT_STYLE_NORMAL,
                                          DWRITE_FONT_STRETCH_NORMAL, px(pixels) as f32, w!("en-us")).ok()?
        };
        self.formats.borrow_mut().insert(which, format.clone());
        Some(format)
    }

    fn brush(&self, color: COLORREF) -> Option<ID2D1SolidColorBrush> {
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

    /// `text` laid out in a box, the way Windows' DT_… `flags` ask.
    fn layout(&self, text: &str, which: Font, width: f32, height: f32, flags: DRAW_TEXT_FORMAT) -> Option<IDWriteTextLayout> {
        let format = self.format(which)?;
        let (text, mnemonic) = mnemonic(text, flags);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let has = |flag: DRAW_TEXT_FORMAT| flags.0 & flag.0 != 0;
        // SAFETY: plain object creation and settings
        unsafe {
            let layout = self.factory.CreateTextLayout(&wide, &format, width.max(1.0), height.max(1.0)).ok()?;
            let align = if has(DT_CENTER) { DWRITE_TEXT_ALIGNMENT_CENTER } else if has(DT_RIGHT) { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_LEADING };
            let _ = layout.SetTextAlignment(align);
            let middle = has(DT_VCENTER) && has(DT_SINGLELINE);
            let _ = layout.SetParagraphAlignment(if middle { DWRITE_PARAGRAPH_ALIGNMENT_CENTER } else { DWRITE_PARAGRAPH_ALIGNMENT_NEAR });
            let _ = layout.SetWordWrapping(if has(DT_WORDBREAK) { DWRITE_WORD_WRAPPING_WRAP } else { DWRITE_WORD_WRAPPING_NO_WRAP });
            if has(DT_END_ELLIPSIS) || has(DT_PATH_ELLIPSIS) {
                // a path keeps what's after its last \ (the file name), like DT_PATH_ELLIPSIS
                let path = has(DT_PATH_ELLIPSIS);
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                                                 delimiter: if path { '\\' as u32 } else { 0 }, delimiterCount: path as u32 };
                let sign = self.factory.CreateEllipsisTrimmingSign(&format).ok();
                let _ = layout.SetTrimming(&trimming, sign.as_ref());
            }
            if let Some(at) = mnemonic.filter(|_| !has(DT_HIDEPREFIX)) {
                let _ = layout.SetUnderline(true, DWRITE_TEXT_RANGE { startPosition: at, length: 1 });
            }
            Some(layout)
        }
    }

    fn draw(&self, dc: HDC, r: RECT, text: &str, which: Font, color: COLORREF, flags: DRAW_TEXT_FORMAT) -> Option<()> {
        let (width, height) = (r.right - r.left, r.bottom - r.top);
        if width <= 0 || height <= 0 {
            return Some(());
        }
        let layout = self.layout(text, which, width as f32, height as f32, flags)?;
        let brush = self.brush(color)?;
        // SAFETY: drawing into the DC we were given, only within `r`
        unsafe {
            self.target.BindDC(dc, &r).ok()?;
            self.target.BeginDraw();
            self.target.DrawTextLayout(Vector2 { X: 0.0, Y: 0.0 }, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
            self.target.EndDraw(None, None).ok()
        }
    }
}

/// `text` without Windows' & marks (&& is an &), and where the underlined letter is.
fn mnemonic(text: &str, flags: DRAW_TEXT_FORMAT) -> (String, Option<u32>) {
    if flags.0 & DT_NOPREFIX.0 != 0 || !text.contains('&') {
        return (text.to_string(), None);
    }
    let (mut out, mut at, mut chars) = (String::new(), None, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, chars.peek().copied()) {
            ('&', Some('&')) => {
                out.push('&');
                chars.next();
            }
            ('&', Some(_)) => at = at.or(Some(out.encode_utf16().count() as u32)),
            _ => out.push(c),
        }
    }
    (out, at)
}

/// Draw text with DirectWrite; false when it isn't available (draw it with GDI then).
pub fn draw(dc: HDC, r: RECT, text: &str, which: Font, color: COLORREF, flags: DRAW_TEXT_FORMAT) -> bool {
    with(|engine| engine.draw(dc, r, text, which, color, flags)).is_some()
}

/// How wide `text` is, as DirectWrite lays it out (pixels).
pub fn width(text: &str, which: Font) -> Option<i32> {
    with(|engine| {
        let layout = engine.layout(text, which, 100_000.0, 1_000.0, DT_SINGLELINE | DT_NOPREFIX)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: a query into our struct
        unsafe { layout.GetMetrics(&mut metrics).ok()? };
        Some(metrics.widthIncludingTrailingWhitespace.ceil() as i32)
    })
}
