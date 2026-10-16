//! The notification-area icon, like IDM's: click to show TurboDM, right-click for a menu, and
//! notifications for finished downloads (a click opens the file).

use super::main::menu::Cmd;
use super::wnd::popup::{self, At, Item};
use super::wnd::{instance, APP_ICON};
use super::{shell, App};
use std::cell::RefCell;
use std::rc::Rc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LRESULT, POINT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 2;
const NIN_BALLOONUSERCLICK: u32 = WM_USER + 5;

thread_local! {
    static LAST: RefCell<Option<String>> = const { RefCell::new(None) }; // download of the last notification
}

/// TurboDM's icon among the notification area's (the hidden downloads' is another: queue.rs).
const ICON: u32 = 1;

pub(super) fn copy(target: &mut [u16], text: &str) {
    let wide: Vec<u16> = text.encode_utf16().take(target.len() - 1).collect();
    target[..wide.len()].copy_from_slice(&wide);
    target[wide.len()] = 0;
}

/// Icon `id` of our window, to fill in for Shell_NotifyIcon.
pub(super) fn data(hwnd: HWND, id: u32) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: id, ..Default::default() }
}

fn add(hwnd: HWND) {
    let mut icon = data(hwnd, ICON);
    icon.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    icon.uCallbackMessage = WM_TRAY;
    // SAFETY: the program's own icon, at the small size; the notification area copies it
    unsafe {
        icon.hIcon = LoadImageW(Some(instance()), PCWSTR(APP_ICON as usize as *const u16), IMAGE_ICON,
                                GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_DEFAULTCOLOR)
            .map(|h| HICON(h.0)).unwrap_or_default();
    }
    copy(&mut icon.szTip, "TurboDM");
    // SAFETY: registers our icon with the notification area
    unsafe { let _ = Shell_NotifyIconW(NIM_ADD, &icon); }
}

pub fn start(app: &Rc<App>) {
    let hwnd = app.main.window.hwnd();
    add(hwnd);
    // SAFETY: plain registration; Explorer sends it when the taskbar comes back after a crash
    let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
    app.main.window.hook(move |msg, _, lparam| {
        if msg == taskbar_created {
            add(hwnd);
            return None; // (the hidden downloads' icon comes back too: queue.rs)
        }
        if msg != WM_TRAY {
            return None;
        }
        let app = super::app();
        match (lparam.0 & 0xffff) as u32 {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => app.show(),
            WM_RBUTTONUP | WM_CONTEXTMENU => tray_menu(hwnd),
            NIN_BALLOONUSERCLICK => {
                if let Some(snap) = LAST.with(|l| l.borrow().clone()).and_then(|id| app.manager.get(&id)) {
                    shell::open(&snap.path);
                }
            }
            _ => {}
        }
        Some(LRESULT(0))
    });
}

fn tray_menu(hwnd: HWND) {
    let items = [Item::Bold(Cmd::Show as u16, "&Show TurboDM".into()), Item::Do(Cmd::Add as u16, "&Add new download...".into(), true),
                 Item::Do(Cmd::ResumeAll as u16, "&Resume all".into(), true), Item::Do(Cmd::PauseAll as u16, "&Pause all".into(), true),
                 Item::Line, Item::Do(Cmd::Exit as u16, "E&xit".into(), true)];
    let mut point = POINT::default();
    // SAFETY: plain query
    unsafe { let _ = GetCursorPos(&mut point); }
    popup::command(hwnd, &items, At::Point(point)); // (its choice comes to the main window as WM_COMMAND)
}

/// A notification from the icon; clicking it opens the download `id`'s file.
pub fn notify(app: &App, title: &str, body: &str, id: Option<String>) {
    LAST.with(|l| *l.borrow_mut() = id);
    let mut icon = data(app.main.window.hwnd(), ICON);
    icon.uFlags = NIF_INFO;
    icon.dwInfoFlags = NIIF_INFO;
    copy(&mut icon.szInfoTitle, title);
    copy(&mut icon.szInfo, body);
    // SAFETY: updates our icon
    unsafe { let _ = Shell_NotifyIconW(NIM_MODIFY, &icon); }
}

pub fn remove(app: &App) {
    // SAFETY: removes our icon
    unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &data(app.main.window.hwnd(), ICON)); }
}
