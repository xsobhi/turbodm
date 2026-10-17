//! Text with DirectWrite, as modern Windows apps draw it (browsers, VS Code…): letters keep the
//! font's true shapes and sit at fractional positions, smoothed as on Linux, Macs and phones.
//! (Windows' classic text drawing, GDI, snaps them to the pixel grid: crisp, but pixelated.)
//! Drawn within the painter's Direct2D pass (d2d.rs).

use super::d2d::{self, Engine};
use super::{icon_face, Font};
use crate::win::wnd::px;
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{COLORREF, RECT};
use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Gdi::*;
use windows_numerics::Vector2;

fn format(engine: &Engine, which: Font) -> Option<IDWriteTextFormat> {
    if let Some(format) = engine.formats.borrow().get(&which) {
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
    let face = super::themes::current().face;
    let (family, collection) = match (which, &engine.ubuntu) {
        (Font::Icons(_), _) => (icon_face(), None),
        (_, Some(ubuntu)) if face == super::themes::Face::Ubuntu => ("Ubuntu", Some(ubuntu)),
        _ => ("Segoe UI", None),
    };
    let weight = if bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
    // SAFETY: plain object creation
    let format = unsafe {
        engine.factory.CreateTextFormat(&HSTRING::from(family), collection, weight, DWRITE_FONT_STYLE_NORMAL,
                                        DWRITE_FONT_STRETCH_NORMAL, px(pixels) as f32, w!("en-us")).ok()?
    };
    engine.formats.borrow_mut().insert(which, format.clone());
    Some(format)
}

/// `text` laid out in a box, the way Windows' DT_… `flags` ask.
fn layout(engine: &Engine, text: &str, which: Font, width: f32, height: f32, flags: DRAW_TEXT_FORMAT) -> Option<IDWriteTextLayout> {
    let format = format(engine, which)?;
    let (text, mnemonic) = mnemonic(text, flags);
    let wide: Vec<u16> = text.encode_utf16().collect();
    let has = |flag: DRAW_TEXT_FORMAT| flags.0 & flag.0 != 0;
    // SAFETY: plain object creation and settings
    unsafe {
        let layout = engine.factory.CreateTextLayout(&wide, &format, width.max(1.0), height.max(1.0)).ok()?;
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
            let sign = engine.factory.CreateEllipsisTrimmingSign(&format).ok();
            let _ = layout.SetTrimming(&trimming, sign.as_ref());
        }
        if let Some(at) = mnemonic.filter(|_| !has(DT_HIDEPREFIX)) {
            let _ = layout.SetUnderline(true, DWRITE_TEXT_RANGE { startPosition: at, length: 1 });
        }
        Some(layout)
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

/// Draw text in the current Direct2D pass; false when DirectWrite isn't there.
pub fn draw(r: RECT, text: &str, which: Font, color: COLORREF, flags: DRAW_TEXT_FORMAT) -> bool {
    let (width, height) = (r.right - r.left, r.bottom - r.top);
    if width <= 0 || height <= 0 {
        return true;
    }
    d2d::with(|engine| {
        let layout = layout(engine, text, which, width as f32, height as f32, flags)?;
        let brush = engine.brush(color)?;
        let origin = Vector2 { X: r.left as f32, Y: r.top as f32 };
        // SAFETY: drawing during the painter's pass
        unsafe { engine.target.DrawTextLayout(origin, &layout, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP) };
        Some(())
    }).is_some()
}

/// How wide `text` is, as DirectWrite lays it out (pixels).
pub fn width(text: &str, which: Font) -> Option<i32> {
    d2d::with(|engine| {
        let layout = layout(engine, text, which, 100_000.0, 1_000.0, DT_SINGLELINE | DT_NOPREFIX)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        // SAFETY: a query into our struct
        unsafe { layout.GetMetrics(&mut metrics).ok()? };
        Some(metrics.widthIncludingTrailingWhitespace.ceil() as i32)
    })
}
