//! Drawing a menu as Mint-Y does: white inside a light grey line, slightly rounded; the line
//! under the mouse (or the keyboard) in Mint's green with white text; shortcuts dim on the right,
//! greyed lines lighter; ticks and sub-menus' arrows from Windows' icon font; thin separators.

use super::layout::{split, Geometry, RADIUS};
use super::Item;
use crate::win::theme::{self, icon, Align, Font, Painter};
use crate::win::wnd::px;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::*;

const CHEVRON_RIGHT: char = '\u{E76C}';

/// What's drawn: the items as `geo` lays them out, `selected` highlighted; `cues`: the letters
/// typed for them underlined.
pub struct Look<'a> {
    pub items: &'a [Item],
    pub geo: &'a Geometry,
    pub selected: Option<usize>,
    pub cues: bool,
}

/// Draw the menu with its top left at `o` (the painter's pixels).
pub fn menu(p: &Painter, o: POINT, look: &Look) {
    let geo = look.geo;
    let at = |left: i32, top: i32, right: i32, bottom: i32| RECT { left: o.x + left, top: o.y + top, right: o.x + right, bottom: o.y + bottom };
    // the line around it is what's left of the border's colour, under the white
    let body = at(0, 0, geo.width, geo.height);
    p.fill(p.rect, theme::border());
    p.round(RECT { left: body.left + 1, top: body.top + 1, right: body.right - 1, bottom: body.bottom - 1 },
            px(RADIUS) - 1, Some(theme::view()), None);
    for (i, item) in look.items.iter().enumerate() {
        let (top, bottom) = geo.rows[i];
        if let Item::Line = item {
            let y = (top + bottom) / 2;
            p.fill(at(1, y, geo.width - 1, y + 1), theme::mix(theme::view(), theme::border(), 0.75));
            continue;
        }
        let row = at(1, top, geo.width - 1, bottom);
        let lit = look.selected == Some(i) && item.enabled();
        if lit {
            p.fill(row, theme::selected());
        }
        let color = if !item.enabled() { theme::disabled() } else if lit { theme::on_selected(false) } else { theme::text() };
        let faint = if !item.enabled() { theme::disabled() } else if lit { theme::mix(theme::on_selected(false), theme::selected(), 0.18) } else { theme::dim() };
        if let Item::Tick(_, _, true) = item {
            p.icon(at(px(8), top, geo.text - px(4), bottom), icon::CHECK, 12, color);
        }
        let (name, shortcut) = split(item.text());
        let font = if matches!(item, Item::Bold(..)) { Font::Strong } else { Font::Body };
        let hide = if look.cues { DRAW_TEXT_FORMAT(0) } else { DT_HIDEPREFIX };
        let room = if shortcut.is_empty() { 0 } else { theme::measure(shortcut, Font::Body) + px(16) };
        p.draw_text(at(geo.text, top, geo.shortcut - room, bottom), name, font, color,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_LEFT | hide);
        if !shortcut.is_empty() {
            p.text(at(geo.text, top, geo.shortcut, bottom), shortcut, Font::Body, faint, Align::Right);
        }
        if let Item::Sub(..) = item {
            p.icon(at(geo.shortcut, top, geo.width - px(8), bottom), CHEVRON_RIGHT, 10, faint);
        }
    }
}
