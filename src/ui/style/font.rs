//! Mint's font, Ubuntu (TurboDM's own copy, data/fonts: Ubuntu Font Licence), for the looks that
//! use it, whether the desktop has it or not: handed to fontconfig for TurboDM alone.

use gtk::prelude::*;
use std::ffi::{c_char, c_void, CString};

#[link(name = "fontconfig")]
unsafe extern "C" {
    fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> i32;
}

const FONTS: [(&str, &[u8]); 2] = [
    ("Ubuntu-Regular.ttf", include_bytes!("../../../data/fonts/Ubuntu-Regular.ttf")),
    ("Ubuntu-Bold.ttf", include_bytes!("../../../data/fonts/Ubuntu-Bold.ttf")),
];

pub fn install() {
    let dir = turbodm::config::data_dir().join("fonts");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (name, bytes) in FONTS {
        let file = dir.join(name);
        if std::fs::metadata(&file).map(|m| m.len()).ok() != Some(bytes.len() as u64) {
            let _ = std::fs::write(&file, bytes);
        }
        let Ok(path) = CString::new(file.to_string_lossy().as_bytes()) else { continue };
        // SAFETY: a NUL-terminated path; a null config is fontconfig's current one
        unsafe { FcConfigAppFontAddFile(std::ptr::null_mut(), path.as_ptr()) };
    }
    gtk::pango::FontMap::changed(&pangocairo_map());
}

fn pangocairo_map() -> gtk::pango::FontMap {
    gtk::Label::new(None).pango_context().font_map().expect("font map")
}
