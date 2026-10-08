//! How TurboDM looks. On Linux it's GTK's own style, at home on the desktop. On Windows it
//! looks like a Windows 11 app: Windows' title bars, its colours, accent colour and light or
//! dark mode, the Segoe UI font and Segoe Fluent icons.
//! `TURBODM_LOOK=windows` (or `windows-dark`) shows the Windows look elsewhere, for testing.

mod system;

use gtk::prelude::*;
use gtk::{gdk, glib};
use std::sync::OnceLock;

pub use system::Palette;

/// The Windows look is in use.
pub fn windows_look() -> bool {
    static LOOK: OnceLock<bool> = OnceLock::new();
    *LOOK.get_or_init(|| cfg!(windows) || std::env::var("TURBODM_LOOK").is_ok_and(|l| l.starts_with("windows")))
}

/// Windows' light/dark mode and accent colour (read once).
pub fn palette() -> &'static Palette {
    static PALETTE: OnceLock<Palette> = OnceLock::new();
    PALETTE.get_or_init(system::palette)
}

/// Call before the first window is built.
pub fn load() {
    if !windows_look() {
        return;
    }
    let Some(display) = gdk::Display::default() else { return };
    let palette = palette();
    let settings = gtk::Settings::for_display(&display);
    settings.set_gtk_application_prefer_dark_theme(palette.dark);
    // crisp text, like Windows' own, rather than GTK's blurrier default there
    let manual = settings.find_property("gtk-font-rendering") // GTK 4.16+
        .and_then(|p| glib::EnumClass::with_type(p.value_type())?.to_value_by_nick("manual"));
    if let Some(manual) = manual {
        settings.set_property_from_value("gtk-font-rendering", &manual);
    }
    settings.set_gtk_xft_antialias(1);
    settings.set_gtk_xft_hinting(1);
    settings.set_gtk_xft_hintstyle(Some("hintslight"));
    settings.set_gtk_hint_font_metrics(true);
    let css = format!("{}\n{}\n{}", palette.colors(), include_str!("fluent.css"), include_str!("fluent-lists.css"));
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

/// Segoe Fluent Icons (Windows 11) or Segoe MDL2 Assets (Windows 10) is there to draw icons.
fn have_icon_font() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        let Some(map) = gtk::Label::new(None).pango_context().font_map() else { return false };
        map.list_families().iter().any(|f| matches!(f.name().as_str(), "Segoe Fluent Icons" | "Segoe MDL2 Assets"))
    })
}

/// The Segoe Fluent icon standing in for a GTK icon name.
pub fn glyph(icon: &str) -> Option<char> {
    let code = match icon.trim_end_matches("-symbolic") {
        "list-add" => 0xE710,
        "media-playback-start" => 0xE768,
        "media-playback-pause" => 0xE769,
        "media-seek-forward" => 0xE893,
        "media-playback-stop" => 0xE71A,
        "user-trash" => 0xE74D,
        "edit-clear-all" => 0xEA99,
        "folder-open" => 0xE838,
        "emblem-system" => 0xE713,
        "open-menu" => 0xE700,
        "folder-download" => 0xE896,
        "object-select" => 0xE73E,
        "video-x-generic" => 0xE714,
        "audio-x-generic" => 0xEC4F,
        "x-office-document" => 0xE8A5,
        "package-x-generic" => 0xF012,
        "application-x-executable" => 0xECAA,
        "image-x-generic" => 0xEB9F,
        "text-x-generic" => 0xE7C3,
        _ => return None,
    };
    char::from_u32(code)
}

/// An icon at `size` pixels: a Fluent glyph with the Windows look, else the icon theme's.
pub fn icon(name: &str, size: i32) -> gtk::Widget {
    match glyph(name).filter(|_| windows_look() && have_icon_font()) {
        Some(c) => glyph_label(c, size).upcast(),
        None => gtk::Image::builder().icon_name(name).pixel_size(size).build().upcast(),
    }
}

/// Icons drawn with the Fluent font (else `icon()` gives images).
pub fn glyph_icons() -> bool {
    windows_look() && have_icon_font()
}

pub fn glyph_label(c: char, size: i32) -> gtk::Label {
    let label = gtk::Label::builder().label(c.to_string()).css_classes(["fluent-icon"]).build();
    let attrs = gtk::pango::AttrList::new();
    attrs.insert(gtk::pango::AttrSize::new_size_absolute(size * gtk::pango::SCALE));
    label.set_attributes(Some(&attrs));
    label
}

/// The accent colour (r, g, b in 0..1) for drawing, e.g. the connections bar.
pub fn accent() -> Option<(f64, f64, f64)> {
    windows_look().then(|| {
        let (r, g, b) = palette().accent();
        (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    })
}
