//! Small dialogs: Windows' message boxes, "Refresh download address", About.

use super::wnd::{self, text, Rows, Window, FIELD};
use super::App;
use std::rc::Rc;
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Windows' message box, with an information or error icon.
pub fn message(owner: HWND, title: &str, body: &str, error: bool) {
    let icon = if error { MB_ICONERROR } else { MB_ICONINFORMATION };
    // SAFETY: a modal message box with our texts
    unsafe { MessageBoxW(Some(owner), &HSTRING::from(body), &HSTRING::from(title), MB_OK | icon); }
}

pub fn confirm_delete(app: &Rc<App>, ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    let body = format!("Delete {} download(s) and their files?\n\nDownloaded files will be removed from disk.", ids.len());
    // SAFETY: a modal question box
    let answer = unsafe {
        MessageBoxW(Some(app.main.window.hwnd()), &HSTRING::from(body), &HSTRING::from("TurboDM"),
                    MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2)
    };
    if answer == IDYES {
        ids.iter().for_each(|id| app.manager.remove(id, true));
    }
}

/// IDM's "Refresh download address": paste a fresh link for an expired one.
pub fn refresh_address(app: &Rc<App>, id: &str) {
    let Some(snap) = app.manager.get(id) else { return };
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
    let window = Window::new("Refresh download address", style, WINDOW_EX_STYLE(0), Some(app.main.window.hwnd()), (520, 150));
    let hint = window.note(&format!("New link for \u{201c}{}\u{201d}. Progress is kept if the server returns the same file.",
                                    snap.filename));
    let field = window.edit(&snap.url, || {});
    let (a, w, id) = (app.clone(), window.clone(), id.to_string());
    let ok = window.default_button("Refresh", move || {
        let url = text(field).trim().to_string();
        if url.starts_with("http") {
            a.manager.refresh_address(&id, url);
        }
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let mut rows = Rows::new(14, 14, 0, 492);
    rows.full(hint, 32);
    rows.full(field, FIELD);
    rows.gap(4);
    let bottom = rows.buttons(&[ok, cancel]);
    wnd::set_client_height(window.hwnd(), bottom + 14);
    wnd::center(window.hwnd());
    window.show();
}

pub fn about(app: &Rc<App>) {
    let body = format!("TurboDM {}\n\nFast multi-connection download manager, with browser integration.\n\n\
                        https://github.com/xsobhi/turbodm\nMIT license.", turbodm::config::VERSION);
    message(app.main.window.hwnd(), "About TurboDM", &body, false);
}
