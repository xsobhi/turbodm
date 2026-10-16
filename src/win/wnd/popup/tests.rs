//! A menu drawn for real (off the screen) and its pixels checked: Mint's green line with white
//! text, white inside a grey line, round corners, a soft shadow lower than the menu.
//! `TURBODM_MENU_PICTURE=file.bmp`: a picture of it, over white, as it shows over the list.

use super::draw::{self, Look};
use super::layout::{self, SHADOW};
use super::window::Surface;
use super::Item::{self, Do, Line, Sub, Tick};
use crate::win::theme::{self, Font};
use crate::win::wnd::px;
use windows::Win32::Foundation::{COLORREF, HWND, RECT};

fn sample() -> Vec<Item> {
    vec![Do(1, "&Add new download...\tCtrl+N".into(), true), Do(2, "Download &video...".into(), true),
         Do(3, "Open &with...".into(), false), Line, Tick(4, "&Categories".into(), true), Tick(5, "&Toolbar".into(), false),
         Sub("&Speed limiter".into(), vec![Do(6, "&No limit".into(), true)]), Line, Do(7, "E&xit\tCtrl+Q".into(), true)]
}

/// `color` as the bitmap has it, opaque (BGRA).
fn opaque(c: COLORREF) -> u32 {
    0xff00_0000 | ((c.0 & 0xff) << 16) | (c.0 & 0xff00) | ((c.0 >> 16) & 0xff)
}

#[test]
fn drawn_as_mint_y() {
    let items = sample();
    let font = |bold: bool| if bold { Font::Strong } else { Font::Body };
    let geo = layout::measure(&items, |text, bold| theme::measure(text, font(bold)), px, 0);
    let body = RECT { left: -30000, top: -30000, right: -30000 + geo.width, bottom: -30000 + geo.height };
    let surface = Surface::new(HWND::default(), body);
    let look = Look { items: &items, geo: &geo, selected: Some(1), cues: true };
    surface.show(|p, origin| draw::menu(p, origin, &look));
    let (pixels, width, height) = surface.pixels();
    let m = px(SHADOW);
    let at = |x: i32, y: i32| pixels[(y * width + x) as usize];
    let alpha = |x: i32, y: i32| at(x, y) >> 24;
    let (top, bottom) = geo.rows[1];
    // the chosen line: Mint's green, across, inside the line around; its name in white
    assert_eq!(at(m + 2, m + (top + bottom) / 2), opaque(theme::ACCENT));
    assert_eq!(at(m + geo.width - 2, m + top + 1), opaque(theme::ACCENT));
    let white = (m + geo.text..m + geo.text + px(80)).flat_map(|x| (m + top..m + bottom).map(move |y| (x, y)))
        .filter(|&(x, y)| at(x, y) == opaque(theme::WHITE)).count();
    assert!(white > 10, "{white} white pixels");
    // white inside a grey line, round corners, a shadow below more than above, none far off
    assert_eq!(at(m + geo.width / 2, m + 2), opaque(theme::VIEW));
    assert_eq!(at(m + geo.width / 2, m), opaque(theme::BORDER));
    assert!(alpha(m, m) < 128);
    assert!(alpha(m + geo.width / 2, m + geo.height + 2) > alpha(m + geo.width / 2, m - 3));
    assert!(alpha(m + geo.width / 2, m - 3) > 0);
    assert_eq!(alpha(0, 0), 0);
    if let Some(path) = std::env::var_os("TURBODM_MENU_PICTURE") {
        save(std::path::Path::new(&path), &pixels, width, height);
    }
}

/// A BMP of the premultiplied `pixels` laid over white.
fn save(path: &std::path::Path, pixels: &[u32], width: i32, height: i32) {
    let row = (width * 3 + 3) / 4 * 4;
    let size = 54 + row * height;
    let mut out = Vec::with_capacity(size as usize);
    let le = |out: &mut Vec<u8>, v: u32, n: usize| out.extend_from_slice(&v.to_le_bytes()[..n]);
    out.extend_from_slice(b"BM");
    for (v, n) in [(size as u32, 4), (0, 4), (54, 4), (40, 4), (width as u32, 4), (height as u32, 4), (1, 2), (24, 2),
                   (0, 4), ((row * height) as u32, 4), (2835, 4), (2835, 4), (0, 4), (0, 4)] {
        le(&mut out, v, n);
    }
    for y in (0..height).rev() {
        for x in 0..width {
            let p = pixels[(y * width + x) as usize];
            let a = p >> 24;
            for shift in [0, 8, 16] {
                out.push((((p >> shift) & 0xff) + 255 - a).min(255) as u8); // over white
            }
        }
        out.resize(out.len() + (row - width * 3) as usize, 0);
    }
    std::fs::write(path, out).expect("picture saved");
}
