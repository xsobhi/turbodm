//! The notification-area icon, like IDM's: click to show TurboDM, right-click for a menu, and
//! notifications for finished downloads (a click opens the file).

use super::main::menu::Cmd;
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

fn copy(target: &mut [u16], text: &str) {
    let wide: Vec<u16> = text.encode_utf16().take(target.len() - 1).collect();
    target[..wide.len()].copy_from_slice(&wide);
    target[wide.len()] = 0;
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

fn add(hwnd: HWND) {
    let mut icon = data(hwnd);
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
            return Some(LRESULT(0));
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
    let items = [(Cmd::Show, "&Show TurboDM"), (Cmd::Add, "&Add new download..."), (Cmd::ResumeAll, "&Resume all"),
                 (Cmd::PauseAll, "&Pause all"), (Cmd::Exit, "E&xit")];
    // SAFETY: a menu shown at the mouse; its choice arrives as WM_COMMAND on the main window
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        for (i, (cmd, text)) in items.iter().enumerate() {
            if i == items.len() - 1 {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            }
            let _ = AppendMenuW(menu, MF_STRING, *cmd as usize, &windows::core::HSTRING::from(*text));
        }
        let _ = SetMenuDefaultItem(menu, Cmd::Show as u32, 0);
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(hwnd); // so the menu closes when clicking elsewhere
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
    }
}

/// A notification from the icon; clicking it opens the download `id`'s file.
pub fn notify(app: &App, title: &str, body: &str, id: Option<String>) {
    LAST.with(|l| *l.borrow_mut() = id);
    let mut icon = data(app.main.window.hwnd());
    icon.uFlags = NIF_INFO;
    icon.dwInfoFlags = NIIF_INFO;
    copy(&mut icon.szInfoTitle, title);
    copy(&mut icon.szInfo, body);
    // SAFETY: updates our icon
    unsafe { let _ = Shell_NotifyIconW(NIM_MODIFY, &icon); }
}

pub fn remove(app: &App) {
    // SAFETY: removes our icon
    unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &data(app.main.window.hwnd())); }
}
