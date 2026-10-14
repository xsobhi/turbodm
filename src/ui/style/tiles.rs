//! TurboDM's 3D icons for the toolbar and the sidebar (tools/icons.py, with Fluent UI symbols,
//! MIT: data/icons/symbols/LICENSE): the same pictures as on Windows, part of the program.

use gtk::{gdk, glib};

macro_rules! tile {
    ($name:literal) => {
        ($name, include_bytes!(concat!("../../../data/icons/", $name, ".png")) as &[u8])
    };
}

const TILES: [(&str, &[u8]); 20] = [
    tile!("add"), tile!("resume"), tile!("pause"), tile!("resume-all"), tile!("pause-all"),
    tile!("delete"), tile!("clear"), tile!("folder"), tile!("options"), tile!("all"),
    tile!("downloading"), tile!("unfinished"), tile!("completed"), tile!("video"), tile!("music"),
    tile!("documents"), tile!("compressed"), tile!("programs"), tile!("images"), tile!("other"),
];

/// The picture called `name` (data/icons/<name>.png), `size` pixels square.
pub fn picture(name: &str, size: i32) -> gtk::Image {
    let image = gtk::Image::builder().pixel_size(size).build();
    let texture = TILES.iter().find(|(n, _)| *n == name)
        .and_then(|&(_, png)| gdk::Texture::from_bytes(&glib::Bytes::from_static(png)).ok());
    if let Some(texture) = texture {
        image.set_paintable(Some(&texture));
    }
    image
}
