//! TurboDM's looks (Options → Look): Linux Mint's, Windows', Ubuntu's, GNOME's, KDE's… light
//! and dark, and well-liked colour schemes; the user's own colours over any of them. A look is
//! its colours, how round its corners are, its font, its title bar's buttons, and how what's
//! chosen shows (filled with the accent colour, or tinted with it, as on Windows 11).

use super::{mix, rgb, WHITE};
use std::sync::{OnceLock, RwLock};
use windows::Win32::Foundation::COLORREF;

/// The font: Ubuntu (TurboDM's own copy, as on Mint and Ubuntu) or Windows' Segoe UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Ubuntu,
    Segoe,
}

/// The title bar's buttons: round, close in the accent colour (GTK's), or Windows' square ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buttons {
    Round,
    Square,
}

/// What's chosen (a row, a sidebar entry, a menu item): filled with the accent colour, or
/// tinted with it (Windows').
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Filled,
    Subtle,
}

/// A look's colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub chrome: COLORREF, // around the content: toolbar, status line, tab row, dialogs
    pub view: COLORREF,   // the content: the list, the sidebar, the tabs' pages
    pub border: COLORREF, // lines between areas
    pub edge: COLORREF,   // the line around a window (Windows 10: wnd/frame.rs)
    pub separator: COLORREF, // between the list's rows
    pub text: COLORREF,
    pub dim: COLORREF, // names of values, headings, small print
    pub disabled: COLORREF,
    pub accent: COLORREF,
    pub on_accent: COLORREF, // text on the accent colour
    pub button: COLORREF,
    pub button_hover: COLORREF,
    pub button_pressed: COLORREF,
    pub button_border: COLORREF,
    pub entry_border: COLORREF,
    pub track: COLORREF, // the empty part of progress bars
    pub green: COLORREF, // a connection at work, on the connections map
    pub danger: COLORREF, // errors; the download disk almost full
}

impl Palette {
    /// The rest of a palette, from its four main colours (as 0xRRGGBB).
    pub fn derive(chrome: u32, view: u32, text: u32, accent: u32) -> Palette {
        let (c, v, t, a) = (rgb(chrome), rgb(view), rgb(text), rgb(accent));
        let dark = is_dark(v);
        let toward = |amount_light: f64, amount_dark: f64| mix(c, t, if dark { amount_dark } else { amount_light });
        let border = toward(0.13, 0.12);
        Palette {
            chrome: c,
            view: v,
            border,
            edge: toward(0.22, 0.18),
            separator: mix(v, t, if dark { 0.06 } else { 0.08 }),
            text: t,
            dim: mix(t, v, 0.35),
            disabled: mix(t, v, 0.62),
            accent: a,
            on_accent: on(a),
            button: if dark { toward(0.0, 0.08) } else { mix(c, WHITE, 0.7) },
            button_hover: if dark { toward(0.0, 0.14) } else { WHITE },
            button_pressed: toward(0.06, 0.04),
            button_border: toward(0.20, 0.20),
            entry_border: toward(0.20, 0.20),
            track: toward(0.08, 0.14),
            green: rgb(0x33d17a),
            danger: rgb(if dark { 0xff6b6b } else { 0xe0393e }),
        }
    }
}

/// A look.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub id: &'static str, // as saved in the settings
    pub name: &'static str,
    pub palette: Palette,
    pub radius: i32, // corners of buttons, fields and highlights (96-DPI pixels)
    pub face: Face,
    pub buttons: Buttons,
    pub selection: Selection,
}

impl Theme {
    pub fn dark(&self) -> bool {
        is_dark(self.palette.view)
    }
}

/// Every look, in the order Options lists them; the first is the one TurboDM starts with.
pub fn all() -> &'static [Theme] {
    static ALL: OnceLock<Vec<Theme>> = OnceLock::new();
    ALL.get_or_init(|| {
        use Buttons::*;
        use Face::*;
        use Selection::*;
        let look = |id, name, (chrome, view, text, accent), radius, face, buttons, selection| Theme {
            id, name, palette: Palette::derive(chrome, view, text, accent), radius, face, buttons, selection,
        };
        vec![
            Theme { id: "mint-y", name: "Mint-Y", palette: MINT_Y, radius: 3, face: Ubuntu, buttons: Round, selection: Filled },
            look("mint-y-dark", "Mint-Y Dark", (0x353535, 0x2b2b2b, 0xdadada, 0x35a854), 3, Ubuntu, Round, Filled),
            look("mint-x", "Mint-X", (0xd6d6d6, 0xffffff, 0x3c3c3c, 0x9ab87c), 2, Ubuntu, Round, Filled),
            look("windows-11", "Windows 11", (0xf3f3f3, 0xffffff, 0x1b1b1b, 0x005fb8), 4, Segoe, Square, Subtle),
            look("windows-11-dark", "Windows 11 Dark", (0x202020, 0x2b2b2b, 0xffffff, 0x60cdff), 4, Segoe, Square, Subtle),
            look("windows-10", "Windows 10", (0xf0f0f0, 0xffffff, 0x000000, 0x0078d7), 0, Segoe, Square, Subtle),
            look("windows-10-dark", "Windows 10 Dark", (0x2b2b2b, 0x191919, 0xffffff, 0x0078d7), 0, Segoe, Square, Subtle),
            look("ubuntu", "Ubuntu", (0xf6f5f4, 0xffffff, 0x3d3d3d, 0xe95420), 6, Ubuntu, Round, Filled),
            look("ubuntu-dark", "Ubuntu Dark", (0x2c2c2c, 0x262626, 0xf7f7f7, 0xe95420), 6, Ubuntu, Round, Filled),
            look("gnome", "GNOME", (0xebebeb, 0xffffff, 0x333333, 0x3584e4), 6, Segoe, Round, Filled),
            look("gnome-dark", "GNOME Dark", (0x303030, 0x1e1e1e, 0xffffff, 0x3584e4), 6, Segoe, Round, Filled),
            look("kde", "KDE Breeze", (0xeff0f1, 0xfcfcfc, 0x232627, 0x3daee9), 3, Segoe, Round, Filled),
            look("kde-dark", "KDE Breeze Dark", (0x2a2e32, 0x1b1e20, 0xfcfcfc, 0x3daee9), 3, Segoe, Round, Filled),
            look("elementary", "elementary OS", (0xfafafa, 0xffffff, 0x333333, 0x3689e6), 6, Segoe, Round, Filled),
            look("pop", "Pop!_OS", (0x353330, 0x2a2826, 0xf2f2f2, 0x48b9c7), 8, Segoe, Round, Filled),
            look("manjaro", "Manjaro", (0x2a2e2f, 0x1f2324, 0xeeeeee, 0x35bf5c), 4, Segoe, Round, Filled),
            look("arch", "Arch", (0x262a31, 0x1c1f24, 0xe8e8e8, 0x1793d1), 2, Segoe, Square, Filled),
            look("nord", "Nord", (0x2e3440, 0x3b4252, 0xeceff4, 0x88c0d0), 4, Segoe, Square, Filled),
            look("dracula", "Dracula", (0x21222c, 0x282a36, 0xf8f8f2, 0xbd93f9), 4, Segoe, Square, Filled),
            look("catppuccin-mocha", "Catppuccin Mocha", (0x181825, 0x1e1e2e, 0xcdd6f4, 0xcba6f7), 6, Segoe, Round, Filled),
            look("catppuccin-latte", "Catppuccin Latte", (0xe6e9ef, 0xeff1f5, 0x4c4f69, 0x8839ef), 6, Segoe, Round, Filled),
            look("solarized-light", "Solarized Light", (0xeee8d5, 0xfdf6e3, 0x586e75, 0x268bd2), 3, Segoe, Square, Filled),
            look("solarized-dark", "Solarized Dark", (0x073642, 0x002b36, 0x93a1a1, 0x268bd2), 3, Segoe, Square, Filled),
        ]
    })
}

/// Mint-Y, as TurboDM always looked: its own exact colours.
const MINT_Y: Palette = Palette {
    chrome: rgb(0xefeff0),
    view: rgb(0xffffff),
    border: rgb(0xd6d6da),
    edge: rgb(0xc4c4c9),
    separator: rgb(0xededef),
    text: rgb(0x303030),
    dim: rgb(0x77777c),
    disabled: rgb(0xb2b2b6),
    accent: rgb(0x35a854),
    on_accent: WHITE,
    button: rgb(0xfafafa),
    button_hover: rgb(0xffffff),
    button_pressed: rgb(0xe4e4e7),
    button_border: rgb(0xc8c8cc),
    entry_border: rgb(0xc8c8cc),
    track: rgb(0xdedee2),
    green: rgb(0x33d17a),
    danger: rgb(0xe0393e),
};

/// The user's own colours over a look (as 0xRRGGBB): its accent, its window and content
/// backgrounds, its text. The rest follows from them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Own {
    pub accent: Option<u32>,
    pub window: Option<u32>,
    pub content: Option<u32>,
    pub text: Option<u32>,
}

/// The look `id` (Mint-Y if there's no such look), with the user's colours over it.
pub fn resolve(id: &str, own: Own) -> Theme {
    let mut theme = *all().iter().find(|t| t.id == id).unwrap_or(&all()[0]);
    let hex = |c: COLORREF| ((c.0 & 0xff) << 16) | (c.0 & 0xff00) | ((c.0 >> 16) & 0xff);
    let p = theme.palette;
    if own.window.is_some() || own.content.is_some() || own.text.is_some() {
        theme.palette = Palette::derive(own.window.unwrap_or(hex(p.chrome)), own.content.unwrap_or(hex(p.view)),
                                        own.text.unwrap_or(hex(p.text)), own.accent.unwrap_or(hex(p.accent)));
    } else if let Some(accent) = own.accent {
        theme.palette.accent = rgb(accent);
        theme.palette.on_accent = on(rgb(accent));
    }
    theme
}

fn current_lock() -> &'static RwLock<Theme> {
    static CURRENT: OnceLock<RwLock<Theme>> = OnceLock::new();
    CURRENT.get_or_init(|| RwLock::new(all()[0]))
}

/// The look in use.
pub fn current() -> Theme {
    *current_lock().read().unwrap()
}

/// Use `theme` from now on; returns the one used until now.
pub fn set(theme: Theme) -> Theme {
    std::mem::replace(&mut *current_lock().write().unwrap(), theme)
}

/// How bright a colour looks, 0 (black) to 1 (white): its relative luminance.
pub fn luminance(c: COLORREF) -> f64 {
    let linear = |shift: u32| {
        let v = ((c.0 >> shift) & 0xff) as f64 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(0) + 0.7152 * linear(8) + 0.0722 * linear(16)
}

pub fn is_dark(c: COLORREF) -> bool {
    luminance(c) < 0.2
}

/// Text on `c`: white, or near-black on light colours.
fn on(c: COLORREF) -> COLORREF {
    if luminance(c) > 0.45 { rgb(0x111111) } else { WHITE }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_look_reads_well() {
        let ids: Vec<_> = all().iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len(), "ids repeat");
        for theme in all() {
            let p = theme.palette;
            // text stands out from where it sits, and on the accent colour
            let contrast = |a: COLORREF, b: COLORREF| {
                let (x, y) = (luminance(a), luminance(b));
                (x.max(y) + 0.05) / (x.min(y) + 0.05)
            };
            assert!(contrast(p.text, p.view) >= 4.5, "{}: text on content", theme.id);
            assert!(contrast(p.text, p.chrome) >= 4.5, "{}: text on window", theme.id);
            assert!(contrast(p.on_accent, p.accent) >= 2.5, "{}: text on accent", theme.id);
        }
        assert_eq!(resolve("no-such-look", Own::default()).id, "mint-y");
        let own = resolve("windows-11", Own { accent: Some(0xff0000), ..Own::default() });
        assert_eq!((own.palette.accent, own.palette.chrome), (rgb(0xff0000), rgb(0xf3f3f3)));
        let own = resolve("mint-y", Own { content: Some(0x101010), ..Own::default() });
        assert!(is_dark(own.palette.view) && own.palette.accent == rgb(0x35a854));
    }
}
