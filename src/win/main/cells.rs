//! What the downloads list shows for a download: its texts, Windows' icon for its file type,
//! and dates in the user's own format.

use std::collections::HashMap;
use std::cell::RefCell;
use turbodm::categories::extension;
use turbodm::engine::{Snapshot, Status};
use turbodm::text::{eta_text, rate_text, status_text};
use turbodm::util::human_size;
use windows::core::HSTRING;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::core::w;
use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, ENUM_DATE_FORMATS_FLAGS, TIME_NOSECONDS};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::Controls::HIMAGELIST;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_FLAGS, SHGFI_SMALLICON, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES};

/// (title, width) as on Linux; the name takes the room the others leave.
pub const COLUMNS: [(&str, i32); 6] = [
    ("File name", 320), ("Size", 90), ("Status", 170), ("Speed", 100), ("Time left", 80), ("Added", 130),
];
pub const STATUS: i32 = 2; // the column drawn as a progress bar

#[derive(Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub cells: [String; 6],
    pub icon: i32,
    pub size: u64,   // for sorting
    pub added: u64,
    pub progress: f64,
    pub eta: f64,
    pub speed: u64,
}

impl Row {
    pub fn new(s: &Snapshot) -> Self {
        Row {
            id: s.id.clone(),
            cells: [s.filename.clone(), human_size(s.size), status_text(s), rate_text(s), eta_text(s),
                    local_time(s.added)],
            icon: file_icon(s),
            size: s.size.unwrap_or(0),
            added: s.added,
            progress: s.progress.unwrap_or(0.0),
            eta: s.eta.unwrap_or(f64::MAX),
            speed: s.speed as u64,
        }
    }

    pub fn compare(&self, other: &Row, column: usize) -> std::cmp::Ordering {
        match column {
            1 => self.size.cmp(&other.size),
            2 => self.progress.total_cmp(&other.progress),
            3 => self.speed.cmp(&other.speed),
            4 => self.eta.total_cmp(&other.eta),
            5 => self.added.cmp(&other.added),
            _ => self.cells[0].to_lowercase().cmp(&other.cells[0].to_lowercase()),
        }
    }
}

/// Windows' small icon list for file types (shared, owned by the system).
pub fn system_icons() -> HIMAGELIST {
    let mut info = SHFILEINFOW::default();
    // SAFETY: asks for the system image list; nothing is created
    let list = unsafe {
        SHGetFileInfoW(&HSTRING::from(".txt"), FILE_ATTRIBUTE_NORMAL, Some(&mut info),
                       std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_USEFILEATTRIBUTES | SHGFI_SYSICONINDEX | SHGFI_SMALLICON)
    };
    HIMAGELIST(list as isize)
}

/// Kinds of files with an icon of their own (programs and installers show theirs, like IDM).
const OWN_ICONS: [&str; 4] = ["exe", "msi", "ico", "lnk"];

/// The file's icon in Windows' list, as Explorer shows it: a program's own once it's downloaded,
/// otherwise its type's.
fn file_icon(s: &Snapshot) -> i32 {
    thread_local! {
        static CACHE: RefCell<HashMap<String, i32>> = RefCell::default();
    }
    let ext = extension(&s.filename);
    let own = s.status == Status::Completed && OWN_ICONS.contains(&ext.as_str());
    let key = if own { s.path.display().to_string() } else { format!(".{ext}") };
    if let Some(icon) = CACHE.with(|cache| cache.borrow().get(&key).copied()) {
        return icon;
    }
    let by_type = if own && s.path.exists() { SHGFI_FLAGS(0) } else { SHGFI_USEFILEATTRIBUTES };
    let ask = if by_type == SHGFI_USEFILEATTRIBUTES { format!(".{ext}") } else { key.clone() };
    let mut info = SHFILEINFOW::default();
    // SAFETY: looks up an icon into our struct: from the file itself, or by its extension only
    // (the file needn't exist)
    unsafe {
        SHGetFileInfoW(&HSTRING::from(ask.as_str()), FILE_ATTRIBUTE_NORMAL, Some(&mut info),
                       std::mem::size_of::<SHFILEINFOW>() as u32, by_type | SHGFI_SYSICONINDEX | SHGFI_SMALLICON);
    }
    CACHE.with(|cache| cache.borrow_mut().insert(key, info.iIcon));
    info.iIcon
}

/// A Unix time as a short date, as on Linux, and the time as the user has it ("Oct 8, 15:42").
pub fn local_time(secs: u64) -> String {
    let ticks = (secs + 11_644_473_600) * 10_000_000; // since 1601, in 100 ns
    let file = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    let (mut date, mut time) = ([0u16; 64], [0u16; 64]);
    // SAFETY: conversions into our buffers, with their sizes
    unsafe {
        if FileTimeToSystemTime(&file, &mut utc).is_err()
            || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return String::new();
        }
        let d = GetDateFormatEx(None, ENUM_DATE_FORMATS_FLAGS(0), Some(&local), w!("MMM d"), Some(&mut date), None);
        let t = GetTimeFormatEx(None, TIME_NOSECONDS, Some(&local), None, Some(&mut time));
        format!("{}, {}", String::from_utf16_lossy(&date[..(d.max(1) - 1) as usize]),
                String::from_utf16_lossy(&time[..(t.max(1) - 1) as usize]))
    }
}
