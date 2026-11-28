//! How TurboDM looks: the same looks as on Windows (turbodm::looks: Options → Look), Mint-Y at
//! first, turned into GTK's CSS. GTK's own built-in theme is the base under it, so TurboDM looks
//! the same whatever theme the desktop uses, and the same as on Windows.

mod font;
mod icons;
mod tiles;

use gtk::prelude::*;
use gtk::{gdk, glib};
use std::cell::RefCell;
use turbodm::looks::{self, mix, Color, Face, Selection, Theme, WHITE};

pub use tiles::picture;

thread_local! {
    static CURRENT: RefCell<Option<Theme>> = const { RefCell::new(None) };
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static TITLES: RefCell<Vec<(glib::WeakRef<gtk::Label>, glib::WeakRef<gtk::Label>)>> = const { RefCell::new(Vec::new()) };
}

/// The look in use.
pub fn current() -> Theme {
    CURRENT.with(|c| c.borrow().unwrap_or(looks::all()[0]))
}

/// Call before the first window is built, and again to change the look (every window takes it
/// at once).
pub fn apply(id: &str, own: &turbodm::config::ThemeColors) {
    let theme = looks::resolve(id, looks::Own { accent: own.accent, window: own.window, content: own.content,
                                                text: own.text });
    CURRENT.with(|c| *c.borrow_mut() = Some(theme));
    let Some(display) = gdk::Display::default() else { return };
    let settings = gtk::Settings::for_display(&display);
    let first = PROVIDER.with(|p| p.borrow().is_none());
    if first {
        // GTK's own theme under ours, whatever the desktop's is; crisp text
        settings.set_gtk_theme_name(Some("Default"));
        settings.set_gtk_decoration_layout(Some(":minimize,maximize,close"));
        settings.set_gtk_xft_antialias(1);
        settings.set_gtk_xft_hinting(1);
        settings.set_gtk_xft_hintstyle(Some("hintslight"));
        font::install();
        icons::install(&display);
    }
    settings.set_gtk_application_prefer_dark_theme(theme.dark());
    let provider = PROVIDER.with(|p| p.borrow_mut().get_or_insert_with(|| {
        let provider = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
        provider
    }).clone());
    provider.load_from_string(&css(&theme));
    TITLES.with(|titles| {
        titles.borrow_mut().retain(|(t, _)| t.upgrade().is_some());
        for (middle, left) in titles.borrow().iter() {
            if let (Some(middle), Some(left)) = (middle.upgrade(), left.upgrade()) {
                place_title(&middle, &left, &theme);
            }
        }
    });
}

/// "#rrggbb"
pub fn hex(c: Color) -> String {
    format!("#{c:06x}")
}

/// A colour for Cairo (r, g, b in 0..1).
pub fn rgb(c: Color) -> (f64, f64, f64) {
    (((c >> 16) & 0xff) as f64 / 255.0, ((c >> 8) & 0xff) as f64 / 255.0, (c & 0xff) as f64 / 255.0)
}

/// What the mouse is on, over `bg`: a little darker, or on a dark colour a little lighter.
pub fn hover(bg: Color) -> Color {
    if looks::is_dark(bg) { mix(bg, WHITE, 0.07) } else { mix(bg, 0, 0.05) }
}

/// …while it's pressed.
pub fn pressed(bg: Color) -> Color {
    if looks::is_dark(bg) { mix(bg, WHITE, 0.13) } else { mix(bg, 0, 0.1) }
}

/// What's chosen (a row, a sidebar entry, a menu item), and its text (and small print).
pub fn selected(theme: &Theme) -> (Color, Color, Color) {
    let p = theme.palette;
    match theme.selection {
        Selection::Filled => (p.accent, p.on_accent, p.on_accent),
        Selection::Subtle => (mix(p.view, p.accent, if theme.dark() { 0.3 } else { 0.16 }), p.text, p.dim),
    }
}

/// A window's title bar, as TurboDM draws it on Windows: 46 pixels, the title in the middle
/// (round buttons) or on the left (Windows' square ones). `buttons` as GTK's decoration layout
/// (":close", ":minimize,close"…).
pub fn title_bar(title: &str, buttons: &str) -> (gtk::HeaderBar, gtk::Label) {
    let label = gtk::Label::builder().label(title).css_classes(["title"]).ellipsize(gtk::pango::EllipsizeMode::End).build();
    // (the same title on the left, shown instead with Windows' square buttons)
    let left = gtk::Label::builder().css_classes(["title"]).ellipsize(gtk::pango::EllipsizeMode::End)
        .margin_start(12).build();
    label.bind_property("label", &left, "label").sync_create().build();
    let header = gtk::HeaderBar::builder().title_widget(&label).decoration_layout(buttons).build();
    header.pack_start(&left);
    place_title(&label, &left, &current());
    TITLES.with(|t| t.borrow_mut().push((label.downgrade(), left.downgrade())));
    (header, label)
}

fn place_title(middle: &gtk::Label, left: &gtk::Label, theme: &Theme) {
    let square = theme.buttons == looks::Buttons::Square;
    middle.set_opacity(if square { 0.0 } else { 1.0 }); // (kept: it holds the bar's height and text)
    left.set_visible(square);
}

/// GTK's name for an icon: Fluent's symbolic icons, as Windows' icon font draws them, where
/// TurboDM has them.
pub fn icon_name(name: &str) -> String {
    icons::fluent(name).unwrap_or_else(|| name.to_string())
}

/// The stylesheet for `theme`.
fn css(theme: &Theme) -> String {
    let p = theme.palette;
    let (selected, on_selected, on_selected_dim) = selected(theme);
    // a bar on what's chosen: in the text's colour when that's filled with the accent
    let bar_on_selected = if selected == p.accent { on_selected } else { p.accent };
    let colors: [(&str, Color); 33] = [
        ("chrome", p.chrome), ("view", p.view), ("border", p.border), ("edge", p.edge),
        ("separator", p.separator), ("text", p.text), ("dim", p.dim), ("disabled", p.disabled),
        ("accent", p.accent), ("on_accent", p.on_accent), ("button", p.button),
        ("button_hover", p.button_hover), ("button_pressed", p.button_pressed),
        ("button_border", p.button_border), ("entry_border", p.entry_border), ("track", p.track),
        ("green", p.green), ("danger", p.danger), ("selected", selected), ("on_selected", on_selected),
        ("on_selected_dim", on_selected_dim), ("bar_on_selected", bar_on_selected),
        ("track_on_selected", mix(selected, on_selected, 0.25)),
        ("hover_view", hover(p.view)), ("hover_chrome", hover(p.chrome)), ("pressed_chrome", pressed(p.chrome)),
        ("menubar_hover", mix(p.chrome, p.border, 0.55)), ("focus_inner", mix(p.accent, p.view, 0.6)),
        ("card", mix(p.view, p.chrome, 0.6)), ("accent_border", mix(p.accent, 0, 0.12)),
        ("menu_line", mix(p.view, p.border, 0.75)), ("accent_hover", mix(p.accent, WHITE, 0.08)),
        ("accent_pressed", mix(p.accent, 0, 0.1)),
    ];
    let mut css = String::new();
    for (name, color) in colors {
        css += &format!("@define-color tdm_{name} {};\n", hex(color));
    }
    let face = match theme.face {
        Face::Ubuntu => "\"Ubuntu\", \"Segoe UI\", sans-serif",
        Face::Segoe => "\"Segoe UI\", \"Selawik\", \"Noto Sans\", \"Cantarell\", sans-serif",
    };
    css += &include_str!("look.css").replace("FONT", face).replace("RADIUS", &format!("{}px", theme.radius))
        .replace("ROUND2", &format!("{}px", theme.radius + 2));
    css += match theme.buttons {
        looks::Buttons::Round => include_str!("round.css"),
        looks::Buttons::Square => include_str!("square.css"),
    };
    if theme.selection == Selection::Subtle {
        css += include_str!("subtle.css");
    }
    css
}
