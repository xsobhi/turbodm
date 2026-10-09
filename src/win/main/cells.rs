//! What the downloads list shows for a download: its texts, Windows' icon for its file type,
//! and dates in the user's own format.

use std::collections::HashMap;
use std::cell::RefCell;
use turbodm::categories::extension;
use turbodm::engine::Snapshot;
use turbodm::text::status_text;
use turbodm::util::{human_eta, human_size, human_speed};
use windows::core::HSTRING;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx, DATE_SHORTDATE, TIME_NOSECONDS};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use windows::Win32::UI::Controls::HIMAGELIST;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_SMALLICON, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES};

pub const COLUMNS: [(&str, i32, bool); 6] = [ // (title, width, right-aligned)
    ("File name", 250, false), ("Size", 80, true), ("Status", 140, false), ("Time left", 80, true),
    ("Transfer rate", 95, true), ("Added", 125, false),
];

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
            cells: [s.filename.clone(), human_size(s.size), status_text(s), human_eta(s.eta),
                    human_speed(s.speed), local_time(s.added)],
            icon: file_icon(&s.filename),
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
            3 => self.eta.total_cmp(&other.eta),
            4 => self.speed.cmp(&other.speed),
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

/// The file type's icon in Windows' list (as Explorer shows it), by extension.
fn file_icon(filename: &str) -> i32 {
    thread_local! {
        static CACHE: RefCell<HashMap<String, i32>> = RefCell::default();
    }
    let ext = format!(".{}", extension(filename));
    CACHE.with(|cache| {
        *cache.borrow_mut().entry(ext.clone()).or_insert_with(|| {
            let mut info = SHFILEINFOW::default();
            // SAFETY: looks up an icon by extension only (the file needn't exist)
            unsafe {
                SHGetFileInfoW(&HSTRING::from(ext.as_str()), FILE_ATTRIBUTE_NORMAL, Some(&mut info),
                               std::mem::size_of::<SHFILEINFOW>() as u32,
                               SHGFI_USEFILEATTRIBUTES | SHGFI_SYSICONINDEX | SHGFI_SMALLICON);
            }
            info.iIcon
        })
    })
}

/// A Unix time as the user's short date and time ("08/10/2026 15:42", or as set in Windows).
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
        let d = GetDateFormatEx(None, DATE_SHORTDATE, Some(&local), None, Some(&mut date), None);
        let t = GetTimeFormatEx(None, TIME_NOSECONDS, Some(&local), None, Some(&mut time));
        format!("{} {}", String::from_utf16_lossy(&date[..(d.max(1) - 1) as usize]),
                String::from_utf16_lossy(&time[..(t.max(1) - 1) as usize]))
    }
}
