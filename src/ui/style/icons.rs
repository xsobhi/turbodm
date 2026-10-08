//! Windows 11's icons, from Microsoft's Fluent UI System Icons (MIT, see icons/LICENSE), standing
//! in for the GTK icon theme's with the Windows look. They're symbolic: drawn in the text colour.

use gtk::gdk;
use std::path::Path;

/// (GTK icon name, Fluent icon, its SVG)
const ICONS: [(&str, &str, &str); 19] = [
    ("list-add", "add", include_str!("icons/add.svg")),
    ("media-playback-start", "play", include_str!("icons/play.svg")),
    ("media-playback-pause", "pause", include_str!("icons/pause.svg")),
    ("media-seek-forward", "next", include_str!("icons/next.svg")),
    ("media-playback-stop", "stop", include_str!("icons/stop.svg")),
    ("user-trash", "delete", include_str!("icons/delete.svg")),
    ("edit-clear-all", "broom", include_str!("icons/broom.svg")),
    ("folder-open", "folder-open", include_str!("icons/folder_open.svg")),
    ("emblem-system", "settings", include_str!("icons/settings.svg")),
    ("open-menu", "navigation", include_str!("icons/navigation.svg")),
    ("folder-download", "arrow-download", include_str!("icons/arrow_download.svg")),
    ("object-select", "checkmark", include_str!("icons/checkmark.svg")),
    ("video-x-generic", "video", include_str!("icons/video.svg")),
    ("audio-x-generic", "music", include_str!("icons/music_note_2.svg")),
    ("x-office-document", "document-text", include_str!("icons/document_text.svg")),
    ("package-x-generic", "folder-zip", include_str!("icons/folder_zip.svg")),
    ("application-x-executable", "apps", include_str!("icons/apps.svg")),
    ("image-x-generic", "image", include_str!("icons/image.svg")),
    ("text-x-generic", "document", include_str!("icons/document.svg")),
];

fn file_name(fluent: &str) -> String {
    format!("fluent-{fluent}-symbolic")
}

/// The Fluent icon's name for a GTK icon name (with or without "-symbolic").
pub fn fluent(icon: &str) -> Option<String> {
    let icon = icon.trim_end_matches("-symbolic");
    ICONS.iter().find(|(gtk, ..)| *gtk == icon).map(|(_, fluent, _)| file_name(fluent))
}

/// Put the icons where GTK finds them (TurboDM's data folder, rewritten when they change).
pub fn install(display: &gdk::Display) {
    let dir = turbodm::config::data_dir().join("icons");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (_, fluent, svg) in ICONS {
        let file = dir.join(format!("{}.svg", file_name(fluent)));
        if std::fs::read_to_string(&file).ok().as_deref() != Some(svg) {
            let _ = std::fs::write(&file, svg);
        }
    }
    gtk::IconTheme::for_display(display).add_search_path(Path::new(&dir));
}
