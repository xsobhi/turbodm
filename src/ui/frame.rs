//! TurboDM's dialogs, made as on Windows (src/win/wnd): the title bar with only Close, the
//! content 16 to 18 pixels from the edges, fields under names in a column, buttons at the
//! bottom on the right (the default one in the accent colour: Enter presses it, Escape closes).

use super::style;
use gtk::prelude::*;
use gtk::glib;

/// A field's, a button's height; the narrowest button.
pub const FIELD: i32 = 30;
pub const BUTTON_WIDTH: i32 = 96;
/// Room between a heading and the text under it.
pub const LINE_GAP: i32 = 8;

pub struct Dialog {
    pub window: gtk::Window,
    pub body: gtk::Box,
    pub title: gtk::Label,
}

/// A dialog `width` wide, over `owner` (if it's showing), its height fitting what's in it.
pub fn dialog(title: &str, owner: Option<&gtk::Window>, width: i32) -> Dialog {
    dialog_with(title, owner, width, ":close", (16, 18))
}

/// …with these title bar buttons, (top, sides) margins.
pub fn dialog_with(title: &str, owner: Option<&gtk::Window>, width: i32, buttons: &str, margins: (i32, i32)) -> Dialog {
    let body = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(0)
        .margin_top(margins.0).margin_bottom(18).margin_start(margins.1).margin_end(margins.1)
        .width_request(width - 2 * margins.1).build();
    let window = gtk::Window::builder().title(title).resizable(false).child(&body).icon_name("turbodm").build();
    let (header, label) = style::title_bar(title, buttons);
    window.set_titlebar(Some(&header));
    if let Some(owner) = owner.filter(|o| o.is_visible()) {
        window.set_transient_for(Some(owner));
    }
    close_on_escape(&window);
    Dialog { window, body, title: label }
}

/// Escape closes `window`.
pub fn close_on_escape(window: &gtk::Window) {
    let keys = gtk::EventControllerKey::new();
    let w = window.downgrade();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            if let Some(w) = w.upgrade() {
                w.close();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(keys);
}

/// A heading (19 pixels, bold).
pub fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).wrap(true).max_width_chars(10).hexpand(true)
        .css_classes(["title-3"]).build()
}

/// A paragraph, wrapped to the dialog's width.
pub fn note(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).yalign(0.0).wrap(true).max_width_chars(10).hexpand(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar).build()
}

/// One line, ellipsized.
pub fn label(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).hexpand(true)
        .max_width_chars(10).build()
}

/// A grid for names and fields: 10 between rows, 12 between a name and its field.
pub fn grid() -> gtk::Grid {
    gtk::Grid::builder().row_spacing(10).column_spacing(12).build()
}

/// A name in a column `width` wide, on its right.
pub fn name_sized(text: &str, width: i32) -> gtk::Label {
    let label = name(text);
    label.set_width_request(width);
    label
}

/// A name in the names' column, on the right.
pub fn name(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(1.0).build()
}

pub fn button(text: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(text).height_request(FIELD + 2).build();
    button.set_size_request(BUTTON_WIDTH, FIELD + 2);
    button
}

/// The default button: in the accent colour, Enter presses it, focused when the window shows.
pub fn default_button(window: &gtk::Window, text: &str) -> gtk::Button {
    let button = button(text);
    button.add_css_class("suggested-action");
    window.set_default_widget(Some(&button));
    let b = button.downgrade();
    window.connect_map(move |_| {
        if let Some(b) = b.upgrade() {
            b.grab_focus();
        }
    });
    button
}

/// Buttons in a row on the right, 8 apart (the last on the right), `gap` under what's above.
pub fn buttons(body: &gtk::Box, buttons: &[&gtk::Button], gap: i32) -> gtk::Box {
    let row = gtk::Box::builder().spacing(8).halign(gtk::Align::End).margin_top(gap).build();
    for button in buttons {
        row.append(*button);
    }
    body.append(&row);
    row
}

/// Names on the left (`width` wide, theirs on the right) and fields beside them, 10 apart.
pub fn form(width: i32) -> (gtk::Grid, impl Fn(&gtk::Grid, &str, &gtk::Widget)) {
    let grid = grid();
    let row = std::cell::Cell::new(0);
    let add = move |grid: &gtk::Grid, text: &str, field: &gtk::Widget| {
        let name = name(text);
        name.set_width_request(width - 12);
        let r = row.get();
        grid.attach(&name, 0, r, 1, 1);
        field.set_hexpand(true);
        grid.attach(field, 1, r, 1, 1);
        row.set(r + 1);
    };
    (grid, add)
}

/// A text field (30 tall).
pub fn entry(text: &str) -> gtk::Entry {
    gtk::Entry::builder().text(text).height_request(FIELD).activates_default(true).hexpand(true).build()
}

/// A text field with a small icon button inside it on the right.
pub fn entry_with_button(text: &str, icon: &str, tip: &str) -> (gtk::Entry, gtk::EntryIconPosition) {
    let entry = entry(text);
    entry.set_secondary_icon_name(Some(&style::icon_name(icon)));
    entry.set_secondary_icon_tooltip_text(Some(tip));
    entry.set_secondary_icon_activatable(true);
    (entry, gtk::EntryIconPosition::Secondary)
}

/// A drop-down of `items`, `selected` picked.
pub fn choice(items: &[&str], selected: u32) -> gtk::DropDown {
    let choice = gtk::DropDown::from_strings(items);
    choice.set_selected(selected.min(items.len().saturating_sub(1) as u32));
    choice.set_height_request(FIELD);
    choice
}

/// A message box, as Windows' (its icon, the text, OK).
pub fn message(owner: Option<&gtk::Window>, title: &str, body: &str, error: bool) {
    let d = dialog(title, owner, 420);
    d.body.append(&icon_and_text(if error { "dialog-error" } else { "dialog-information" }, body));
    let ok = default_button(&d.window, "OK");
    let w = d.window.clone();
    ok.connect_clicked(move |_| w.close());
    buttons(&d.body, &[&ok], 18);
    d.window.present();
}

/// A question box, as Windows' (Yes, and No the default); `yes` runs on Yes.
pub fn ask(owner: Option<&gtk::Window>, title: &str, body: &str, yes: impl Fn() + 'static) {
    let d = dialog(title, owner, 440);
    d.body.append(&icon_and_text("dialog-question", body));
    let go = button("Yes");
    let no = default_button(&d.window, "No");
    let w = d.window.clone();
    go.connect_clicked(move |_| {
        w.close();
        yes();
    });
    let w = d.window.clone();
    no.connect_clicked(move |_| w.close());
    buttons(&d.body, &[&go, &no], 18);
    d.window.present();
}

fn icon_and_text(icon: &str, body: &str) -> gtk::Box {
    let row = gtk::Box::builder().spacing(14).build();
    let image = gtk::Image::builder().icon_name(icon).pixel_size(32).valign(gtk::Align::Start).build();
    row.append(&image);
    let text = note(body);
    text.set_selectable(true);
    text.set_focusable(false);
    row.append(&text);
    row
}
