//! Options → Look, as on Windows (src/win/looks.rs): the look (turbodm::looks) from a list, and
//! the user's own colours over it, each picked in a colour picker. Every window takes a change at
//! once; Cancel goes back to the look there was.

use super::settings::Line;
use super::style;
use gtk::prelude::*;
use gtk::{gdk, gio};
use std::cell::RefCell;
use std::rc::Rc;
use turbodm::config::{Settings, ThemeColors};
use turbodm::looks::{self, contrast, WHITE};

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

type Swatches = Rc<RefCell<Vec<(gtk::DrawingArea, gtk::Label, Own)>>>;

pub struct LookPage {
    theme: gtk::DropDown,
    colors: [gtk::Overlay; 4], // accent, window, content, text
    reset: gtk::Button,
    chosen: Rc<RefCell<Chosen>>,
}

impl LookPage {
    pub fn new(window: &gtk::Window, s: &Settings) -> LookPage {
        let chosen = Rc::new(RefCell::new(Chosen { theme: s.theme.clone(), colors: s.theme_colors,
                                                   before: (s.theme.clone(), s.theme_colors) }));
        let names: Vec<&str> = looks::all().iter().map(|t| t.name).collect();
        let now = looks::all().iter().position(|t| t.id == s.theme).unwrap_or(0);
        let theme = super::frame::choice(&names, now as u32);
        let swatches: Swatches = Rc::default();
        let colors = [Own::Accent, Own::Window, Own::Content, Own::Text]
            .map(|which| swatch(window, &chosen, which, &swatches));
        redraw(&swatches);
        let (c, all) = (chosen.clone(), swatches.clone());
        theme.connect_selected_notify(move |choice| {
            c.borrow_mut().theme = looks::all()[choice.selected() as usize].id.to_string();
            show(&c.borrow());
            redraw(&all);
        });
        let reset = gtk::Button::with_label("Use the theme's");
        let (c, all) = (chosen.clone(), swatches.clone());
        reset.connect_clicked(move |_| {
            c.borrow_mut().colors = ThemeColors::default();
            show(&c.borrow());
            redraw(&all);
        });
        LookPage { theme, colors, reset, chosen }
    }

    pub fn lines(&self) -> Vec<Line> {
        use Line::{Heading, Setting};
        let [accent, window, content, text] = self.colors.clone();
        vec![Heading("Look"),
             Setting("Theme", self.theme.clone().upcast(), 220),
             Heading("Your own colours, over the theme's"),
             Setting("Accent (what's chosen, main buttons)", accent.upcast(), 96),
             Setting("Window background", window.upcast(), 96),
             Setting("Content background (the list, pages)", content.upcast(), 96),
             Setting("Text", text.upcast(), 96),
             Setting("", self.reset.clone().upcast(), 150)]
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

/// The boxes again, with the look's colours now, each with its #RRGGBB.
fn redraw(all: &Swatches) {
    let p = style::current().palette;
    for (area, label, which) in all.borrow().iter() {
        let color = color_of(&p, *which);
        let ink = if contrast(WHITE, color) >= 2.3 { WHITE } else { 0x111111 };
        label.set_markup(&format!("<span foreground=\"#{ink:06x}\">#{color:06X}</span>"));
        area.queue_draw();
    }
}

fn color_of(p: &looks::Palette, which: Own) -> looks::Color {
    match which {
        Own::Accent => p.accent,
        Own::Window => p.chrome,
        Own::Content => p.view,
        Own::Text => p.text,
    }
}

/// The chosen look, on every window at once.
fn show(chosen: &Chosen) {
    style::apply(&chosen.theme, &chosen.colors);
}

/// A box of one of the user's colours (the look's, until they pick one), with its #RRGGBB on it;
/// a click opens the colour picker.
fn swatch(window: &gtk::Window, chosen: &Rc<RefCell<Chosen>>, which: Own, all: &Swatches) -> gtk::Overlay {
    let current = move || color_of(&style::current().palette, which);
    let area = gtk::DrawingArea::builder().tooltip_text("Pick a colour").css_classes(["swatch-area"])
        .cursor(&gdk::Cursor::from_name("pointer", None).unwrap_or_else(|| gdk::Cursor::from_name("default", None).unwrap()))
        .build();
    let hot = Rc::new(std::cell::Cell::new(false));
    let h = hot.clone();
    area.set_draw_func(move |_, cr, width, height| {
        let theme = style::current();
        let color = current();
        let (w, h2) = (width as f64 - 1.0, height as f64 - 1.0);
        let r = theme.radius.max(2) as f64;
        cr.new_sub_path();
        cr.arc(0.5 + w - r, 0.5 + r, r, -90f64.to_radians(), 0.0);
        cr.arc(0.5 + w - r, 0.5 + h2 - r, r, 0.0, 90f64.to_radians());
        cr.arc(0.5 + r, 0.5 + h2 - r, r, 90f64.to_radians(), 180f64.to_radians());
        cr.arc(0.5 + r, 0.5 + r, r, 180f64.to_radians(), 270f64.to_radians());
        cr.close_path();
        let (red, green, blue) = style::rgb(color);
        cr.set_source_rgb(red, green, blue);
        let _ = cr.fill_preserve();
        let line = if h.get() { theme.palette.accent } else { theme.palette.button_border };
        let (red, green, blue) = style::rgb(line);
        cr.set_source_rgb(red, green, blue);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
    });
    let motion = gtk::EventControllerMotion::new();
    let (a, h) = (area.downgrade(), hot.clone());
    motion.connect_enter(move |_, _, _| {
        h.set(true);
        if let Some(a) = a.upgrade() { a.queue_draw() }
    });
    let (a, h) = (area.downgrade(), hot);
    motion.connect_leave(move |_| {
        h.set(false);
        if let Some(a) = a.upgrade() { a.queue_draw() }
    });
    area.add_controller(motion);
    let click = gtk::GestureClick::new();
    let (c, every, w) = (chosen.clone(), all.clone(), window.downgrade());
    click.connect_released(move |_, _, _, _| {
        let now = current();
        let (red, green, blue) = style::rgb(now);
        let rgba = gdk::RGBA::new(red as f32, green as f32, blue as f32, 1.0);
        let dialog = gtk::ColorDialog::builder().title("Pick a colour").modal(true).with_alpha(false).build();
        let (c, all) = (c.clone(), every.clone());
        dialog.choose_rgba(w.upgrade().as_ref(), Some(&rgba), gio::Cancellable::NONE, move |picked| {
            let Ok(picked) = picked else { return };
            let channel = |v: f32| ((v.clamp(0.0, 1.0) * 255.0).round() as u32) & 0xff;
            let hex = (channel(picked.red()) << 16) | (channel(picked.green()) << 8) | channel(picked.blue());
            {
                let mut chosen = c.borrow_mut();
                let slot = match which {
                    Own::Accent => &mut chosen.colors.accent,
                    Own::Window => &mut chosen.colors.window,
                    Own::Content => &mut chosen.colors.content,
                    Own::Text => &mut chosen.colors.text,
                };
                *slot = Some(hex);
            }
            show(&c.borrow());
            redraw(&all); // (the others may follow from it)
        });
    });
    area.add_controller(click);
    let label = gtk::Label::builder().css_classes(["caption"]).can_target(false).build();
    let overlay = gtk::Overlay::builder().child(&area).build();
    overlay.add_overlay(&label);
    all.borrow_mut().push((area, label, which));
    overlay
}
