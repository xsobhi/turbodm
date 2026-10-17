//! Options → Look: the look (theme/themes.rs) from a list, and the user's own colours over it,
//! each picked in Windows' colour picker. Every window takes a change at once; Cancel goes back
//! to the look there was.

use super::theme::{self, themes, Align, Font};
use super::wnd::canvas::{self, Mouse};
use super::wnd::{self, px, Window};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::config::{Settings, ThemeColors};
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_ANYCOLOR, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};

/// Which of the user's colours a box is.
#[derive(Clone, Copy)]
enum Own {
    Accent,
    Window,
    Content,
    Text,
}

/// The look chosen so far, and the one there was.
struct Chosen {
    theme: String,
    colors: ThemeColors,
    before: (String, ThemeColors),
}

pub struct LookPage {
    pub theme: HWND,
    pub colors: [HWND; 4], // accent, window, content, text
    pub reset: HWND,
    chosen: Rc<RefCell<Chosen>>,
}

impl LookPage {
    pub fn new(window: &Window, s: &Settings) -> LookPage {
        let chosen = Rc::new(RefCell::new(Chosen { theme: s.theme.clone(), colors: s.theme_colors,
                                                   before: (s.theme.clone(), s.theme_colors) }));
        let names: Vec<&str> = themes::all().iter().map(|t| t.name).collect();
        let now = themes::all().iter().position(|t| t.id == s.theme).unwrap_or(0);
        let c = chosen.clone();
        let theme = window.choice(&names, now, move |i| {
            c.borrow_mut().theme = themes::all()[i].id.to_string();
            show(&c.borrow());
        });
        let swatches = Rc::new(Cell::new([HWND::default(); 4]));
        let colors = [Own::Accent, Own::Window, Own::Content, Own::Text].map(|which| swatch(window, &chosen, which, &swatches));
        swatches.set(colors);
        let c = chosen.clone();
        let reset = window.button("Use the theme's", move || {
            c.borrow_mut().colors = ThemeColors::default();
            show(&c.borrow());
            swatches.get().iter().for_each(|s| canvas::redraw(*s));
        });
        LookPage { theme, colors, reset, chosen }
    }

    /// What to save (OK).
    pub fn chosen(&self) -> (String, ThemeColors) {
        let chosen = self.chosen.borrow();
        (chosen.theme.clone(), chosen.colors)
    }

    /// Back to the look there was (Cancel), if it was changed.
    pub fn revert(&self) {
        let mut chosen = self.chosen.borrow_mut();
        if (chosen.theme.as_str(), chosen.colors) != (chosen.before.0.as_str(), chosen.before.1) {
            (chosen.theme, chosen.colors) = chosen.before.clone();
            show(&chosen);
        }
    }
}

/// The chosen look, on every window at once.
fn show(chosen: &Chosen) {
    let before = theme::apply(&chosen.theme, &chosen.colors);
    wnd::retheme(&before);
}

/// A box of one of the user's colours (the look's, until they pick one), with its #RRGGBB on it;
/// a click opens Windows' colour picker.
fn swatch(window: &Window, chosen: &Rc<RefCell<Chosen>>, which: Own, all: &Rc<Cell<[HWND; 4]>>) -> HWND {
    let current = move || {
        let p = themes::current().palette;
        match which {
            Own::Accent => p.accent,
            Own::Window => p.chrome,
            Own::Content => p.view,
            Own::Text => p.text,
        }
    };
    let hot = Rc::new(Cell::new(false));
    let h = hot.clone();
    let hwnd = window.canvas(move |p| {
        p.fill(p.rect, theme::view());
        let color = current();
        let r = RECT { left: p.rect.left + 1, top: p.rect.top + 1, right: p.rect.right - 1, bottom: p.rect.bottom - 1 };
        let line = if h.get() { theme::accent() } else { theme::button_border() };
        p.round(r, px(theme::radius().max(2)), Some(color), Some(line));
        let ink = if themes::contrast(theme::WHITE, color) >= 2.3 { theme::WHITE } else { theme::rgb(0x111111) };
        p.text(r, &hex(color), Font::Caption, ink, Align::Center);
    });
    let (c, me, all) = (chosen.clone(), Rc::new(Cell::new(hwnd)), all.clone());
    canvas::on_mouse(hwnd, move |event| match event {
        Mouse::Move(..) if !hot.get() => {
            hot.set(true);
            canvas::redraw(me.get());
        }
        Mouse::Leave => {
            hot.set(false);
            canvas::redraw(me.get());
        }
        Mouse::Up(..) => {
            let Some(picked) = pick(me.get(), current()) else { return };
            {
                let mut chosen = c.borrow_mut();
                let slot = match which {
                    Own::Accent => &mut chosen.colors.accent,
                    Own::Window => &mut chosen.colors.window,
                    Own::Content => &mut chosen.colors.content,
                    Own::Text => &mut chosen.colors.text,
                };
                *slot = Some(picked);
            }
            show(&c.borrow());
            all.get().iter().for_each(|s| canvas::redraw(*s)); // (the others may follow from it)
        }
        _ => {}
    });
    canvas::set_tips(hwnd, &[(RECT { left: 0, top: 0, right: px(200), bottom: px(60) }, "Pick a colour".into())]);
    hwnd
}

/// "#RRGGBB".
fn hex(c: COLORREF) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0 & 0xff, (c.0 >> 8) & 0xff, (c.0 >> 16) & 0xff)
}

/// Windows' colour picker, starting at `now`; the colour picked, as 0xRRGGBB.
fn pick(owner: HWND, now: COLORREF) -> Option<u32> {
    thread_local! {
        static CUSTOM: RefCell<[COLORREF; 16]> = const { RefCell::new([theme::WHITE; 16]) }; // its "custom colours"
    }
    CUSTOM.with(|custom| {
        let mut custom = custom.borrow_mut();
        let mut choose = CHOOSECOLORW {
            lStructSize: std::mem::size_of::<CHOOSECOLORW>() as u32,
            // SAFETY: plain query
            hwndOwner: unsafe { GetAncestor(owner, GA_ROOT) },
            rgbResult: now,
            lpCustColors: custom.as_mut_ptr(),
            Flags: CC_RGBINIT | CC_FULLOPEN | CC_ANYCOLOR,
            ..Default::default()
        };
        // SAFETY: a modal dialog over our window, with our 16 custom colours
        unsafe { ChooseColorW(&mut choose) }.as_bool().then(|| {
            let c = choose.rgbResult.0;
            ((c & 0xff) << 16) | (c & 0xff00) | ((c >> 16) & 0xff)
        })
    })
}
