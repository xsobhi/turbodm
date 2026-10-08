//! How TurboDM looks. On Linux it's GTK's own style, at home on the desktop. On Windows it
//! looks like a Windows 11 app: Windows' title bars, its colours, accent colour and light or
//! dark mode, the Segoe UI font and Fluent icons.
//! `TURBODM_LOOK=windows` (or `windows-dark`) shows the Windows look elsewhere, for testing.

mod icons;
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
    icons::install(&display);
    let css = format!("{}\n{}\n{}", palette.colors(), include_str!("fluent.css"), include_str!("fluent-lists.css"));
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

/// The icon to use for a GTK icon name: Windows 11's with the Windows look.
pub fn icon_name(name: &str) -> String {
    windows_look().then(|| icons::fluent(name)).flatten().unwrap_or_else(|| name.to_string())
}

/// An icon at `size` pixels.
pub fn icon(name: &str, size: i32) -> gtk::Image {
    gtk::Image::builder().icon_name(icon_name(name)).pixel_size(size).build()
}

/// The accent colour (r, g, b in 0..1) for drawing, e.g. the connections bar.
pub fn accent() -> Option<(f64, f64, f64)> {
    windows_look().then(|| {
        let (r, g, b) = palette().accent();
        (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
    })
}
