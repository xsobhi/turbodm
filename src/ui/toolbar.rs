//! The main window's toolbar, menus, and the note shown when the list is empty.

use gtk::gio;
use gtk::prelude::*;

/// A toolbar button like IDM's: icon over a label.
fn tool_button(icon: &str, label: &str, tooltip: &str, action: &str) -> gtk::Button {
    let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(3).build();
    content.append(&gtk::Image::builder().icon_name(icon).pixel_size(20).build());
    content.append(&gtk::Label::builder().label(label).css_classes(["caption"]).build());
    gtk::Button::builder().child(&content).tooltip_text(tooltip).action_name(action)
        .css_classes(["flat"]).width_request(76).build()
}

pub fn toolbar() -> gtk::Box {
    let bar = gtk::Box::builder().spacing(2).margin_start(6).margin_end(6).margin_top(4).margin_bottom(4).build();
    let groups: [&[(&str, &str, &str, &str)]; 4] = [
        &[("list-add-symbolic", "Add URL", "Add a download (Ctrl+N)", "win.add")],
        &[("media-playback-start-symbolic", "Resume", "Resume selected (Ctrl+R)", "win.resume"),
          ("media-playback-pause-symbolic", "Pause", "Pause selected (Ctrl+P)", "win.pause"),
          ("media-seek-forward-symbolic", "Resume all", "Resume every unfinished download", "win.resume-all"),
          ("media-playback-stop-symbolic", "Pause all", "Pause every download", "win.pause-all")],
        &[("user-trash-symbolic", "Delete", "Remove selected from the list (Delete)", "win.remove"),
          ("edit-clear-all-symbolic", "Clear done", "Remove completed downloads from the list", "win.delete-completed")],
        &[("folder-open-symbolic", "Folder", "Open the download folder", "win.open-download-folder"),
          ("emblem-system-symbolic", "Options", "Preferences (Ctrl+,)", "win.settings")],
    ];
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            bar.append(&gtk::Separator::builder().orientation(gtk::Orientation::Vertical)
                .margin_start(4).margin_end(4).margin_top(6).margin_bottom(6).build());
        }
        for (icon, label, tip, action) in group.iter() {
            bar.append(&tool_button(icon, label, tip, action));
        }
    }
    bar
}

pub fn empty_page() -> (gtk::Box, gtk::Label) {
    let text = gtk::Label::builder().css_classes(["dim-label"]).justify(gtk::Justification::Center).build();
    let page = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
        .valign(gtk::Align::Center).halign(gtk::Align::Center).build();
    page.append(&gtk::Image::builder().icon_name("folder-download-symbolic").pixel_size(64)
        .css_classes(["dim-label"]).build());
    page.append(&text);
    (page, text)
}

/// The header bar's menu.
pub fn app_menu() -> gio::Menu {
    let app_menu = gio::Menu::new();
    let section = gio::Menu::new();
    section.append(Some("Add URL…"), Some("win.add"));
    section.append(Some("Resume all"), Some("win.resume-all"));
    section.append(Some("Pause all"), Some("win.pause-all"));
    section.append(Some("Remove completed from list"), Some("win.delete-completed"));
    section.append(Some("Open download folder"), Some("win.open-download-folder"));
    app_menu.append_section(None, &section);
    let section = gio::Menu::new();
    section.append(Some("Preferences"), Some("win.settings"));
    section.append(Some("About TurboDM"), Some("win.about"));
    section.append(Some("Quit"), Some("app.quit"));
    app_menu.append_section(None, &section);
    app_menu
}

pub fn context_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let open = gio::Menu::new();
    open.append(Some("Open"), Some("win.open"));
    open.append(Some("Open with…"), Some("win.open-with"));
    open.append(Some("Open folder"), Some("win.open-folder"));
    open.append(Some("Progress details"), Some("win.details"));
    menu.append_section(None, &open);
    let control = gio::Menu::new();
    control.append(Some("Resume"), Some("win.resume"));
    control.append(Some("Pause"), Some("win.pause"));
    control.append(Some("Refresh download address…"), Some("win.refresh"));
    control.append(Some("Copy address"), Some("win.copy-url"));
    menu.append_section(None, &control);
    let remove = gio::Menu::new();
    remove.append(Some("Remove from list"), Some("win.remove"));
    remove.append(Some("Delete with file"), Some("win.delete-file"));
    menu.append_section(None, &remove);
    menu
}
