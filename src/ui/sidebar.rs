//! The main window's sidebar, like IDM's category tree: downloads by state and by file type,
//! with counts, and the free space where downloads are saved.

use gtk::prelude::*;
use gtk::gio;
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use turbodm::engine::{Snapshot, Status};
use turbodm::util::human_size;

/// What the downloads list shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    All,
    Active,     // downloading or waiting in the queue
    Unfinished, // everything not complete
    Completed,
    Category(&'static str),
}

/// A download's place in the state views (kept on each row for filtering).
pub fn state_of(status: Status) -> &'static str {
    match status {
        Status::Completed => "completed",
        s if s.is_active() || s == Status::Queued => "active",
        _ => "unfinished",
    }
}

impl View {
    pub fn matches(self, state: &str, category: &str) -> bool {
        match self {
            View::All => true,
            View::Active => state == "active",
            View::Unfinished => state != "completed",
            View::Completed => state == "completed",
            View::Category(c) => category == c,
        }
    }
}

const STATES: [(View, &str, &str); 4] = [
    (View::All, "All downloads", "folder-download-symbolic"),
    (View::Active, "Downloading", "media-playback-start-symbolic"),
    (View::Unfinished, "Unfinished", "media-playback-pause-symbolic"),
    (View::Completed, "Completed", "object-select-symbolic"),
];

/// (category, label, icon); categories come from `turbodm::categories`, "General" is the rest.
const CATEGORIES: [(&str, &str, &str); 7] = [
    ("Video", "Video", "video-x-generic-symbolic"),
    ("Music", "Music", "audio-x-generic-symbolic"),
    ("Documents", "Documents", "x-office-document-symbolic"),
    ("Compressed", "Compressed", "package-x-generic-symbolic"),
    ("Programs", "Programs", "application-x-executable-symbolic"),
    ("Images", "Images", "image-x-generic-symbolic"),
    ("General", "Other", "text-x-generic-symbolic"),
];

pub struct Sidebar {
    pub widget: gtk::Box,
    counts: Vec<(View, gtk::Label)>,
    disk: gtk::Label,
    disk_bar: gtk::LevelBar,
}

fn heading(text: &str) -> gtk::ListBoxRow {
    let label = gtk::Label::builder().label(text).xalign(0.0).margin_start(6).margin_top(10)
        .css_classes(["dim-label", "caption-heading"]).build();
    gtk::ListBoxRow::builder().child(&label).selectable(false).activatable(false).build()
}

fn entry(name: &str, icon: &str) -> (gtk::ListBoxRow, gtk::Label) {
    let count = gtk::Label::builder().css_classes(["dim-label", "numeric"]).build();
    let row = gtk::Box::builder().spacing(10).margin_start(4).margin_end(4).margin_top(3).margin_bottom(3).build();
    row.append(&gtk::Image::from_icon_name(icon));
    row.append(&gtk::Label::builder().label(name).xalign(0.0).hexpand(true).build());
    row.append(&count);
    (gtk::ListBoxRow::builder().child(&row).build(), count)
}

impl Sidebar {
    /// Picking a view sets `view` and runs `on_change`.
    pub fn new(view: Rc<Cell<View>>, on_change: impl Fn() + 'static) -> Self {
        let list = gtk::ListBox::builder().css_classes(["navigation-sidebar"]).build();
        let (mut views, mut counts) = (Vec::new(), Vec::new());
        let mut add = |row: gtk::ListBoxRow, view: Option<(View, gtk::Label)>| {
            list.append(&row);
            views.push(view.as_ref().map(|(v, _)| *v));
            counts.extend(view);
        };
        add(heading("Downloads"), None);
        for (view, name, icon) in STATES {
            let (row, count) = entry(name, icon);
            add(row, Some((view, count)));
        }
        add(heading("Categories"), None);
        for (category, name, icon) in CATEGORIES {
            let (row, count) = entry(name, icon);
            add(row, Some((View::Category(category), count)));
        }
        list.select_row(list.row_at_index(1).as_ref());
        let v = view.clone();
        list.connect_row_selected(move |_, row| {
            if let Some(picked) = row.and_then(|r| views.get(r.index() as usize).copied().flatten()) {
                v.set(picked);
                on_change();
            }
        });

        let disk = gtk::Label::builder().xalign(0.0).css_classes(["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End).build();
        let disk_bar = gtk::LevelBar::builder().min_value(0.0).max_value(1.0).build();
        disk_bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW)); // a full disk is the "bad" end
        disk_bar.add_offset_value("used", 0.9);
        let footer = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(4)
            .margin_start(12).margin_end(12).margin_top(8).margin_bottom(10).build();
        footer.append(&disk);
        footer.append(&disk_bar);
        let scroller = gtk::ScrolledWindow::builder().child(&list).vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never).build();
        let widget = gtk::Box::builder().orientation(gtk::Orientation::Vertical).width_request(200).build();
        widget.append(&scroller);
        widget.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        widget.append(&footer);
        Sidebar { widget, counts, disk, disk_bar }
    }

    /// New counts, and the free space in the download folder.
    pub fn update(&self, snapshots: &[Snapshot], download_dir: &Path) {
        for (view, label) in &self.counts {
            let n = snapshots.iter().filter(|s| view.matches(state_of(s.status), s.category)).count();
            let text = if n == 0 { String::new() } else { n.to_string() };
            if label.text() != text {
                label.set_text(&text);
            }
        }
        // the folder may not exist yet: ask about the nearest one that does
        let Some(dir) = download_dir.ancestors().find(|d| d.exists()) else { return };
        let info = gio::File::for_path(dir).query_filesystem_info("filesystem::*", gio::Cancellable::NONE);
        if let Ok(info) = info {
            let free = info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE);
            let size = info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_SIZE);
            let text = format!("{} free of {}", human_size(Some(free)), human_size(Some(size)));
            if self.disk.text() != text {
                self.disk.set_text(&text);
                self.disk.set_tooltip_text(Some(&format!("Download folder: {}", download_dir.display())));
                self.disk_bar.set_value(if size > 0 { 1.0 - free as f64 / size as f64 } else { 0.0 });
            }
        }
    }
}
