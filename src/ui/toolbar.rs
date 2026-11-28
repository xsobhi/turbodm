//! The toolbar, as on Windows (and like IDM's): 3D icons with their names under them, in groups;
//! or, smaller, the icons alone (View → Toolbar text), their names in their tooltips.

use super::style;
use gtk::prelude::*;

/// (picture, action, name, tooltip); `None` separates groups.
const BUTTONS: [Option<(&str, &str, &str, &str)>; 12] = [
    Some(("add", "win.add", "Add URL", "Add a download (Ctrl+N)")),
    None,
    Some(("resume", "win.resume", "Resume", "Resume selected (Ctrl+R; Space pauses or resumes)")),
    Some(("pause", "win.pause", "Pause", "Pause selected (Ctrl+P; Space pauses or resumes)")),
    Some(("resume-all", "win.resume-all", "Resume all", "Resume every unfinished download (F5)")),
    Some(("pause-all", "win.pause-all", "Pause all", "Pause every download")),
    None,
    Some(("delete", "win.remove", "Delete", "Delete selected: off the list, and their files if you say (Delete)")),
    Some(("clear", "win.delete-completed", "Clear done", "Remove completed downloads from the list")),
    None,
    Some(("folder", "win.open-download-folder", "Folder", "Open the download folder")),
    Some(("options", "win.settings", "Options", "Preferences (Ctrl+,)")),
];

/// The toolbar's height and a button's width, with the names under the icons and without.
const SIZES: [(i32, i32); 2] = [(60, 76), (40, 44)];

pub struct Toolbar {
    pub widget: gtk::Box,
}

impl Toolbar {
    pub fn new(text: bool) -> Self {
        let widget = gtk::Box::builder().spacing(2).margin_start(6).css_classes(["toolbar"]).build();
        let toolbar = Toolbar { widget };
        toolbar.show_text(text);
        toolbar
    }

    /// The names under the icons, or not (smaller).
    pub fn show_text(&self, text: bool) {
        while let Some(child) = self.widget.first_child() {
            self.widget.remove(&child);
        }
        let (height, width) = SIZES[usize::from(!text)];
        self.widget.set_height_request(height);
        let inset = if text { 16 } else { 10 };
        for button in BUTTONS {
            let Some((picture, action, name, tip)) = button else {
                self.widget.append(&gtk::Separator::builder().orientation(gtk::Orientation::Vertical)
                    .margin_start(4).margin_end(4).margin_top(inset).margin_bottom(inset).build());
                continue;
            };
            let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(0)
                .valign(gtk::Align::Center).build();
            content.append(&style::picture(picture, 26));
            let tip = if text {
                content.append(&gtk::Label::builder().label(name).css_classes(["caption"]).margin_top(2).build());
                tip.to_string()
            } else {
                format!("{name}: {tip}")
            };
            let button = gtk::Button::builder().child(&content).tooltip_text(tip).action_name(action)
                .css_classes(["tool"]).width_request(width).margin_top(4).margin_bottom(4)
                .focus_on_click(false).build();
            self.widget.append(&button);
        }
    }
}

/// The note shown when the list is empty, in the middle of the list.
pub fn empty_page() -> (gtk::Box, gtk::Label) {
    let text = gtk::Label::builder().css_classes(["dim-label"]).justify(gtk::Justification::Center).build();
    let page = gtk::Box::builder().orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center).halign(gtk::Align::Center).build();
    page.append(&text);
    (page, text)
}
