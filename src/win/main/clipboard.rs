//! Clipboard monitoring (opt-in): offer to download links to files you copy. Windows tells the
//! main window about every copy, so nothing is polled.

use super::super::shell;
use std::cell::RefCell;
use turbodm::engine::AddRequest;
use turbodm::text::looks_like_file_link;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::UI::WindowsAndMessaging::WM_CLIPBOARDUPDATE;

pub fn start(hwnd: HWND) {
    // SAFETY: plain registration of our window
    unsafe { let _ = AddClipboardFormatListener(hwnd); }
}

pub fn handle(msg: u32, _wparam: WPARAM, _lparam: LPARAM) -> Option<LRESULT> {
    if msg != WM_CLIPBOARDUPDATE {
        return None;
    }
    thread_local! {
        static LAST: RefCell<String> = RefCell::default();
    }
    let app = super::super::app();
    if app.manager.settings().clipboard_monitor {
        let text = shell::clipboard_text().unwrap_or_default().trim().to_string();
        let new = LAST.with(|last| last.replace(text.clone()) != text);
        if new && looks_like_file_link(&text) {
            app.handle_download(AddRequest { url: text, ..Default::default() }, false);
        }
    }
    Some(LRESULT(0))
}
