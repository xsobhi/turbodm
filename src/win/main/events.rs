//! The main window's messages: commands, the refresh timer, resizing and the splitter,
//! closing to the notification area; and the program's message loop.

use super::super::wnd::{self, px};
use super::super::{run_posted, App, WM_WAKE};
use super::{clipboard, menu, open_row, refresh};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::LRESULT;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetActiveWindow, ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

const REFRESH_TIMER: usize = 1;

/// After the app exists: refresh timer, commands, splitter, closing to the tray, clipboard.
pub fn setup(app: &Rc<App>) {
    let window = &app.main.window;
    for cmd in menu::ALL {
        window.on_command(cmd as u16, move |_| menu::run(&super::super::app(), cmd));
    }
    window.on_command(IDOK.0 as u16, |_| {
        let app = super::super::app();
        if let Some(id) = app.main.list.selected().into_iter().next() {
            open_row(&app, &id); // Enter on a download
        }
    });
    // close: keep running in the notification area, like IDM (Exit is in the menus)
    let hwnd = window.hwnd();
    window.on_close(move || {
        // SAFETY: hide our own window
        unsafe { let _ = ShowWindow(hwnd, SW_HIDE); }
        false
    });
    let started = Instant::now();
    let split = app.main.split.clone();
    let dragging = Rc::new(Cell::new(false));
    window.hook(move |msg, wparam, lparam| match msg {
        WM_WAKE => {
            run_posted();
            Some(LRESULT(0))
        }
        WM_TIMER if wparam.0 == REFRESH_TIMER => {
            let app = super::super::app();
            refresh(&app);
            quit_when_idle(&app, started);
            Some(LRESULT(0))
        }
        WM_SIZE => {
            super::super::app().main.layout((lparam.0 & 0xffff) as i32, ((lparam.0 >> 16) & 0xffff) as i32);
            Some(LRESULT(0))
        }
        WM_GETMINMAXINFO => {
            // SAFETY: WM_GETMINMAXINFO comes with a MINMAXINFO to fill in
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize.x = px(640);
            info.ptMinTrackSize.y = px(360);
            Some(LRESULT(0))
        }
        // the gap between the categories and the list moves them
        WM_SETCURSOR if wparam.0 as *mut std::ffi::c_void == hwnd.0 && (lparam.0 & 0xffff) as u32 == HTCLIENT => {
            // SAFETY: plain cursor change
            unsafe { SetCursor(LoadCursorW(None, IDC_SIZEWE).ok()); }
            Some(LRESULT(1))
        }
        WM_LBUTTONDOWN => {
            dragging.set(true);
            // SAFETY: follow the mouse until the button is released
            unsafe { SetCapture(hwnd); }
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE if dragging.get() => {
            let x = (lparam.0 & 0xffff) as i16 as i32 * 96 / px(96);
            split.set(x.clamp(120, 500));
            let (w, h) = wnd::client_size(hwnd);
            super::super::app().main.layout(px(w), px(h));
            Some(LRESULT(0))
        }
        WM_LBUTTONUP if dragging.get() => {
            dragging.set(false);
            // SAFETY: as above
            unsafe { let _ = ReleaseCapture(); }
            Some(LRESULT(0))
        }
        _ => clipboard::handle(msg, wparam, lparam),
    });
    // SAFETY: a timer on our window
    unsafe { SetTimer(Some(hwnd), REFRESH_TIMER, 100, None) };
    clipboard::start(hwnd);
    let (w, h) = wnd::client_size(hwnd);
    app.main.layout(px(w), px(h));
}

/// Started by the browser and nothing is shown or downloading: quit after a few seconds.
fn quit_when_idle(app: &App, started: Instant) {
    if app.background && started.elapsed() > Duration::from_secs(5) && !app.manager.busy()
        && !wnd::any_window_shown() {
        app.quit();
    }
}

/// Windows' message loop, with the main window's shortcuts and dialogs' keyboard handling.
pub fn message_loop(app: &Rc<App>) {
    let shortcuts = menu::shortcuts();
    let main = app.main.window.hwnd();
    let mut msg = MSG::default();
    // SAFETY: the standard loop on this thread's messages
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let active = GetActiveWindow();
            if active == main && TranslateAcceleratorW(main, shortcuts, &msg) != 0 {
                continue;
            }
            if !active.is_invalid() && IsDialogMessageW(active, &msg).as_bool() {
                continue; // Tab, Enter and Escape in dialogs
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
