//! "TurboDM downloads": the download windows hidden or minimized go together under one taskbar
//! button of their own, beside TurboDM's (with the downloads' arrow). Clicking it shows the
//! downloads in a small menu, with how far each is, and a click there brings that one back.

use super::theme::pictures;
use super::wnd::Window;
use super::{progress, App};
use std::rc::Rc;
use turbodm::engine::Status;
use turbodm::util::human_speed;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, LRESULT, POINT};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Its own taskbar identity: a button of its own even where Windows groups a program's windows.
const APP_ID: &str = "xsobhi.TurboDM.Downloads";

/// Show the button while a download window is tucked away, hide it when none is (and say how many).
pub fn update(app: &Rc<App>) {
    // SAFETY: plain queries on our own windows
    let hidden: Vec<String> = app.progress.borrow().iter().filter(|(_, p)| unsafe { !IsWindowVisible(p.window.hwnd()).as_bool() })
        .map(|(id, _)| id.clone()).collect();
    if hidden.is_empty() {
        if let Some(window) = app.queue.borrow().as_ref() {
            // SAFETY: plain call on our own window
            unsafe { let _ = ShowWindow(window.hwnd(), SW_HIDE); }
        }
        return;
    }
    let window = app.queue.borrow_mut().get_or_insert_with(|| make(app)).clone();
    let title = match hidden.len() {
        1 => "TurboDM: 1 download".to_string(),
        n => format!("TurboDM: {n} downloads"),
    };
    window.set_title(&title);
    // SAFETY: plain calls on our own window: on the taskbar, minimized, without taking the focus
    unsafe {
        if !IsWindowVisible(window.hwnd()).as_bool() {
            let _ = ShowWindow(window.hwnd(), SW_SHOWMINNOACTIVE);
        }
    }
}

fn make(app: &Rc<App>) -> Rc<Window> {
    let window = Window::new("TurboDM downloads", WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
                             WS_EX_APPWINDOW, None, (320, 120));
    set_app_id(window.hwnd(), APP_ID);
    if let Some(icon) = pictures::hicon(pictures::DOWNLOADING) {
        for which in [ICON_BIG, ICON_SMALL] {
            super::wnd::send(window.hwnd(), WM_SETICON, which as usize, icon.0 as isize);
        }
    }
    // a click on its button would bring it back: the menu of downloads shows instead
    window.hook(move |msg, wparam, _| match msg {
        WM_SYSCOMMAND if matches!(wparam.0 as u32 & 0xFFF0, SC_RESTORE | SC_MAXIMIZE) => {
            super::post(choose);
            Some(LRESULT(0))
        }
        _ => None,
    });
    // closed from the taskbar: the download windows come back
    let a = Rc::downgrade(app);
    window.on_close(move || {
        if let Some(app) = a.upgrade() {
            show_all(&app);
        }
        false
    });
    window
}

/// The menu of downloads, where the mouse is (by the taskbar button).
fn choose(app: &Rc<App>) {
    let Some(window) = app.queue.borrow().clone() else { return };
    // the ones with windows, then the others still under way
    let mut ids: Vec<String> = app.progress.borrow().keys().cloned().collect();
    ids.sort();
    let others = app.manager.snapshot_where(|id, status| status.is_active() && !ids.iter().any(|i| i == id));
    ids.extend(others.into_iter().map(|s| s.id));
    let lines: Vec<(String, String)> = ids.iter().filter_map(|id| app.manager.get(id)).map(|s| {
        let how = match (s.status, s.progress) {
            (Status::Completed, _) => "Done".to_string(),
            (status, Some(p)) if status.is_active() => format!("{:.0}%  {}", p * 100.0, human_speed(s.speed)),
            (status, Some(p)) => format!("{:.0}%  {}", p * 100.0, status.label()),
            (status, None) => status.label().to_string(),
        };
        (s.id.clone(), format!("{}\t{how}", s.filename))
    }).collect();
    // SAFETY: our menu, shown and destroyed here; TPM_RETURNCMD gives the choice back
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        for (i, (_, text)) in lines.iter().enumerate() {
            let _ = AppendMenuW(menu, MF_STRING, i + 1, &HSTRING::from(text.as_str()));
        }
        if lines.is_empty() {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, &HSTRING::from("No downloads under way"));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let (all, main) = (lines.len() + 1, lines.len() + 2);
        let _ = AppendMenuW(menu, MF_STRING, all, &HSTRING::from("Show all download windows"));
        let _ = AppendMenuW(menu, MF_STRING, main, &HSTRING::from("Open TurboDM"));
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(window.hwnd()); // (so the menu closes when clicking elsewhere)
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, point.x, point.y, None,
                                    window.hwnd(), None).0 as usize;
        let _ = DestroyMenu(menu);
        match picked {
            0 => {}
            n if n == all => show_all(app),
            n if n == main => app.show(),
            n => progress::open(app, &lines[n - 1].0),
        }
    }
}

/// Every download window back on screen.
fn show_all(app: &Rc<App>) {
    let ids: Vec<String> = app.progress.borrow().keys().cloned().collect();
    for id in ids {
        progress::open(app, &id);
    }
}

/// Give a window its own taskbar identity (AppUserModelID).
fn set_app_id(hwnd: HWND, id: &str) {
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    use windows::Win32::System::Com::CoTaskMemAlloc;
    use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: a string value in COM's memory, as Windows' InitPropVariantFromString makes it (the
    // store copies it; PropVariantClear frees ours); the window's property store
    unsafe {
        let text = CoTaskMemAlloc(wide.len() * 2) as *mut u16;
        if text.is_null() {
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), text, wide.len());
        let mut value = PROPVARIANT { Anonymous: PROPVARIANT_0 { Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
            vt: VT_LPWSTR, wReserved1: 0, wReserved2: 0, wReserved3: 0,
            Anonymous: PROPVARIANT_0_0_0 { pwszVal: PWSTR(text) },
        }) } };
        if let Ok(store) = SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) {
            let _ = store.SetValue(&PKEY_AppUserModel_ID, &value);
            let _ = store.Commit();
        }
        let _ = PropVariantClear(&mut value);
    }
}

