//! The toolbar, like IDM's: big icons with their names under them.

use super::super::wnd::{instance, px, send, Window};
use super::menu::Cmd;
use crate::win::icons;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// (icon, command, name); `None` separates groups.
const BUTTONS: [Option<(u16, Cmd, &str)>; 13] = [
    Some((icons::ADD, Cmd::Add, "Add URL")), None,
    Some((icons::RESUME, Cmd::Resume, "Resume")), Some((icons::PAUSE, Cmd::Pause, "Pause")),
    Some((icons::RESUME_ALL, Cmd::ResumeAll, "Resume all")), Some((icons::PAUSE_ALL, Cmd::PauseAll, "Pause all")), None,
    Some((icons::DELETE, Cmd::Remove, "Delete")), Some((icons::CLEAR, Cmd::ClearDone, "Clear done")), None,
    Some((icons::FOLDER, Cmd::DownloadFolder, "Folder")), Some((icons::OPTIONS, Cmd::Settings, "Options")), None,
];

pub const ICON: i32 = 32;

pub fn create(window: &Window) -> HWND {
    let style = WINDOW_STYLE(TBSTYLE_FLAT | TBSTYLE_TOOLTIPS) | WINDOW_STYLE(CCS_NODIVIDER as u32)
        | WINDOW_STYLE(CCS_NORESIZE as u32) | WINDOW_STYLE(CCS_NOPARENTALIGN as u32);
    let (bar, _) = window.control(TOOLBARCLASSNAMEW, "", style, WINDOW_EX_STYLE(0));
    send(bar, TB_BUTTONSTRUCTSIZE, std::mem::size_of::<TBBUTTON>(), 0);
    let size = px(ICON);
    // SAFETY: an image list of our own icon resources, kept by the toolbar for its lifetime
    let images = unsafe { ImageList_Create(size, size, ILC_COLOR32 | ILC_MASK, BUTTONS.len() as i32, 0) };
    let mut buttons = Vec::new();
    let mut names = Vec::new(); // kept alive until TB_ADDBUTTONS has copied them
    for button in BUTTONS {
        let Some((icon, cmd, name)) = button else {
            buttons.push(TBBUTTON { fsStyle: BTNS_SEP as u8, ..Default::default() });
            continue;
        };
        // SAFETY: loads an icon resource of this program at the toolbar's size
        let index = unsafe {
            let handle = LoadImageW(Some(instance()), PCWSTR(icon as usize as *const u16), IMAGE_ICON, size, size,
                                    LR_DEFAULTCOLOR).unwrap_or_default();
            ImageList_ReplaceIcon(images, -1, HICON(handle.0))
        };
        let name = HSTRING::from(name);
        buttons.push(TBBUTTON {
            iBitmap: index,
            idCommand: cmd as i32,
            fsState: TBSTATE_ENABLED as u8,
            fsStyle: (BTNS_BUTTON | BTNS_AUTOSIZE) as u8,
            iString: name.as_ptr() as isize,
            ..Default::default()
        });
        names.push(name);
    }
    send(bar, TB_SETIMAGELIST, 0, images.0 as isize);
    send(bar, TB_SETBUTTONSIZE, 0, ((px(ICON + 36) as isize) << 16) | px(ICON + 44) as isize);
    send(bar, TB_ADDBUTTONSW, buttons.len(), buttons.as_ptr() as isize);
    send(bar, TB_SETMAXTEXTROWS, 1, 0);
    send(bar, TB_AUTOSIZE, 0, 0);
    bar
}

/// The toolbar's height (screen pixels).
pub fn height(bar: HWND) -> i32 {
    let size = send(bar, TB_GETBUTTONSIZE, 0, 0);
    ((size >> 16) & 0xffff) as i32 + px(6)
}

pub fn enable(bar: HWND, cmd: Cmd, on: bool) {
    let state = send(bar, TB_GETSTATE, cmd as usize, 0);
    if (state & TBSTATE_ENABLED as isize != 0) != on {
        send(bar, TB_ENABLEBUTTON, cmd as usize, on as isize);
    }
}
