//! The main window's commands: menu bar, right-click menu, keyboard shortcuts, and what each does.

use super::super::{add, dialogs, progress, settings, shell, App};
use std::rc::Rc;
use turbodm::engine::{AddRequest, Status};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DELETE, VK_F5};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum Cmd {
    Add = 1000, Resume, Pause, ResumeAll, PauseAll, Remove, DeleteFile, ClearDone, Open, OpenWith,
    OpenFolder, Details, CopyUrl, Refresh, DownloadFolder, Settings, Browsers, CheckUpdates, About,
    Exit, Show, SelectAll, Find,
}

pub const ALL: [Cmd; 23] = [Cmd::Add, Cmd::Resume, Cmd::Pause, Cmd::ResumeAll, Cmd::PauseAll, Cmd::Remove,
    Cmd::DeleteFile, Cmd::ClearDone, Cmd::Open, Cmd::OpenWith, Cmd::OpenFolder, Cmd::Details, Cmd::CopyUrl,
    Cmd::Refresh, Cmd::DownloadFolder, Cmd::Settings, Cmd::Browsers, Cmd::CheckUpdates, Cmd::About,
    Cmd::Exit, Cmd::Show, Cmd::SelectAll, Cmd::Find];

/// (command, text) for a menu; `None` is a separator.
type Items<'a> = &'a [Option<(Cmd, &'a str)>];

fn popup(items: Items) -> HMENU {
    // SAFETY: a new menu filled with our items; Windows frees it with its window or after use
    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        for item in items {
            let _ = match item {
                Some((cmd, text)) => AppendMenuW(menu, MF_STRING, *cmd as usize, &HSTRING::from(*text)),
                None => AppendMenuW(menu, MF_SEPARATOR, 0, None),
            };
        }
        menu
    }
}

/// The main menu (the button at the end of the toolbar), as on Linux.
pub fn app_menu(app: &Rc<App>) {
    let items: Items = &[Some((Cmd::Add, "&Add URL...	Ctrl+N")), Some((Cmd::ResumeAll, "&Resume all	F5")),
        Some((Cmd::PauseAll, "&Pause all")), Some((Cmd::ClearDone, "Remove &completed from list")),
        Some((Cmd::DownloadFolder, "Open download &folder")), None,
        Some((Cmd::Settings, "Pr&eferences	Ctrl+,")), Some((Cmd::Browsers, "Add to your &browser...")),
        Some((Cmd::CheckUpdates, "Check for &updates")), Some((Cmd::About, "Abou&t TurboDM")), None,
        Some((Cmd::Exit, "&Quit	Ctrl+Q"))];
    let mut button = RECT::default();
    // SAFETY: plain query; the menu opens under the button, its right edges lined up
    unsafe { let _ = GetWindowRect(app.main.menu_button(), &mut button); }
    show(app.main.window.hwnd(), popup(items), POINT { x: button.right, y: button.bottom }, TPM_RIGHTALIGN);
}

/// The downloads list's right-click menu, at `point` (screen pixels).
pub fn context(owner: HWND, point: POINT) {
    let items: Items = &[Some((Cmd::Open, "&Open")), Some((Cmd::OpenWith, "Open &with...")),
        Some((Cmd::OpenFolder, "Open f&older")), Some((Cmd::Details, "Progress &details")), None,
        Some((Cmd::Resume, "&Resume")), Some((Cmd::Pause, "&Pause")),
        Some((Cmd::Refresh, "Refresh download &address...")), Some((Cmd::CopyUrl, "&Copy address")), None,
        Some((Cmd::Remove, "Remove from &list")), Some((Cmd::DeleteFile, "Delete with &file..."))];
    show(owner, popup(items), point, TRACK_POPUP_MENU_FLAGS(0));
}

fn show(owner: HWND, menu: HMENU, point: POINT, align: TRACK_POPUP_MENU_FLAGS) {
    // SAFETY: shows our menu; its choice arrives as WM_COMMAND
    unsafe {
        let _ = SetForegroundWindow(owner);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON | align, point.x, point.y, None, owner, None);
        let _ = DestroyMenu(menu);
    }
}

/// Keyboard shortcuts of the main window.
pub fn shortcuts() -> HACCEL {
    let ctrl = FVIRTKEY | FCONTROL;
    let key = |flags: ACCEL_VIRT_FLAGS, key: u16, cmd: Cmd| ACCEL { fVirt: flags, key, cmd: cmd as u16 };
    let table = [key(ctrl, b'N' as u16, Cmd::Add), key(ctrl, b'R' as u16, Cmd::Resume), key(ctrl, b'P' as u16, Cmd::Pause),
        key(ctrl, b'I' as u16, Cmd::Details), key(ctrl, b'Q' as u16, Cmd::Exit), key(ctrl, b'A' as u16, Cmd::SelectAll),
        key(ctrl, 0xBC, Cmd::Settings), // Ctrl+, (VK_OEM_COMMA)
        key(ctrl, b'F' as u16, Cmd::Find),
        key(FVIRTKEY, VK_DELETE.0, Cmd::Remove), key(FVIRTKEY, VK_F5.0, Cmd::ResumeAll)];
    // SAFETY: a table built from our array
    unsafe { CreateAcceleratorTableW(&table).unwrap_or_default() }
}

/// Do `cmd` (from the menus, the toolbar, a shortcut or the tray).
pub fn run(app: &Rc<App>, cmd: Cmd) {
    let selected = || app.main.list.selected();
    let first = || selected().into_iter().next();
    match cmd {
        Cmd::Add => add::open(app, AddRequest::default()),
        Cmd::Resume => selected().iter().for_each(|id| {
            app.manager.resume(id);
            if app.manager.settings().show_progress_window {
                progress::open(app, id);
            }
        }),
        Cmd::Pause => selected().iter().for_each(|id| app.manager.pause(id)),
        Cmd::ResumeAll => app.manager.resume_all(),
        Cmd::PauseAll => app.manager.pause_all(),
        Cmd::Remove => selected().iter().for_each(|id| app.manager.remove(id, false)),
        Cmd::DeleteFile => dialogs::confirm_delete(app, selected()),
        Cmd::ClearDone => app.manager.snapshot().iter().filter(|s| s.status == Status::Completed)
            .for_each(|s| app.manager.remove(&s.id, false)), // from the list only: the files stay
        Cmd::Open => if let Some(id) = first() { shell::open_download(app, &id) },
        Cmd::OpenWith => if let Some(s) = first().and_then(|id| app.manager.get(&id)) { shell::open_with(&s.path) },
        Cmd::OpenFolder => if let Some(id) = first() { shell::show_download(app, &id) },
        Cmd::Details => if let Some(id) = first() { progress::open(app, &id) },
        Cmd::CopyUrl => if let Some(s) = first().and_then(|id| app.manager.get(&id)) { shell::copy_text(&s.url) },
        Cmd::Refresh => if let Some(id) = first() { dialogs::refresh_address(app, &id) },
        Cmd::DownloadFolder => {
            let dir = app.manager.settings().download_dir;
            let _ = std::fs::create_dir_all(&dir);
            shell::open(&dir);
        }
        Cmd::Settings => settings::open(app),
        Cmd::Browsers => super::super::browsers::open(app),
        Cmd::CheckUpdates => super::super::update::check(app, true),
        Cmd::About => dialogs::about(app),
        Cmd::Exit => app.quit(),
        Cmd::Show => app.show(),
        Cmd::SelectAll => app.main.list.select_all(),
        Cmd::Find => {
            // SAFETY: plain focus change in our window
            unsafe { let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(app.main.search)); }
            super::super::wnd::send(app.main.search, 0x00B1, 0, -1); // EM_SETSEL: all
        }
    }
}
