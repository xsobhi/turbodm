//! "Download complete": what was downloaded and where it is, and what to do next. The file on
//! white, as Explorer shows files (its big icon, its name, its size and kind); the choices in a
//! grey bar at the bottom, as Windows' and GTK's dialogs have them, Open first among them. A small
//! square at the bottom right, with the "move" pointer in it, drags the file out, as IDM's does:
//! into a folder, a chat, anywhere files go.

use super::theme::{self, Align, Font};
use super::wnd::canvas::{self, Mouse};
use super::wnd::{self, is_checked, place, rect, Rows, Window};
use super::{shell, App};
use std::path::Path;
use std::cell::Cell;
use std::rc::Rc;
use turbodm::engine::Snapshot;
use turbodm::text::finished_line;
use windows::core::{Interface, HSTRING};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{DT_END_ELLIPSIS, DT_NOPREFIX, DT_PATH_ELLIPSIS, DT_SINGLELINE, DT_VCENTER};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows::Win32::UI::Controls::{IImageList, ImageList_Draw, HIMAGELIST, ILD_TRANSPARENT};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE};
use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, LoadCursorW, DI_NORMAL, HICON, IDC_SIZEALL};
use windows::Win32::UI::WindowsAndMessaging::IDCANCEL;

/// Sizes, 96-DPI pixels: the window's width, its white part's height, the file's icon.
const WIDTH: i32 = 560;
const BODY: i32 = 186;
const ICON: i32 = 48;
const DRAG: (i32, i32) = (40, wnd::FIELD + 2); // the drag square, as tall as the buttons

pub fn open(app: &Rc<App>, snap: &Snapshot) {
    let window = Window::dialog("Download complete", None, (WIDTH, BODY + 64), false);
    let big = icon_of(&snap.path, SHIL_EXTRALARGE);
    let (name, line) = (snap.filename.clone(), finished_line(snap));
    let header = window.canvas(move |p| {
        p.fill(p.rect, theme::VIEW);
        if let Some((list, index)) = &big {
            // SAFETY: drawing an icon of Windows' list into our canvas
            unsafe { let _ = ImageList_Draw(HIMAGELIST(list.as_raw() as isize), *index, p.gdi(), 0, 0, ILD_TRANSPARENT); }
        }
        let left = wnd::px(ICON + 16);
        p.text(RECT { left, top: 0, right: p.rect.right, bottom: wnd::px(28) }, &name, Font::Title, theme::TEXT, Align::Left);
        p.text(RECT { left, top: wnd::px(28), right: p.rect.right, bottom: wnd::px(50) }, &line, Font::Body, theme::DIM, Align::Left);
    });
    let drag = drag_box(&window, snap.path.clone());
    // where it is and where it came from, in a light card
    let rows = [("Saved to", snap.path.display().to_string()), ("From", snap.url.clone())];
    let card = window.canvas(move |p| {
        p.fill(p.rect, theme::VIEW);
        p.round(p.rect, wnd::px(theme::RADIUS + 2), Some(theme::mix(theme::VIEW, theme::CHROME, 0.6)), Some(theme::BORDER));
        for (i, (name, value)) in rows.iter().enumerate() {
            let top = wnd::px(9 + 26 * i as i32);
            let bottom = top + wnd::px(26);
            p.text(RECT { left: wnd::px(14), top, right: wnd::px(84), bottom }, name, Font::Body, theme::DIM, Align::Left);
            let cut = if i == 0 { DT_PATH_ELLIPSIS } else { DT_END_ELLIPSIS }; // a path keeps its file name
            let r = RECT { left: wnd::px(90), top, right: p.rect.right - wnd::px(14), bottom };
            p.draw_text(r, value, Font::Body, theme::TEXT, cut | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
        }
    });
    let again = window.checkbox("Don't show again", false, |_| {});
    let (path, w) = (snap.path.clone(), window.clone());
    // (the window goes at once; the other program starts after)
    let with = window.button("Open with…", move || { w.close(); shell::open_with(&path); });
    let (a, id, w) = (app.clone(), snap.id.clone(), window.clone());
    let folder = window.button("Open folder", move || { w.close(); shell::show_download(&a, &id); });
    let (path, w) = (snap.path.clone(), window.clone());
    let open = window.default_button("Open", move || { w.close(); shell::open(&path); });
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.close());
    let a = app.clone();
    window.on_close(move || {
        if is_checked(again) {
            let mut s = a.manager.settings();
            s.show_complete_dialog = false;
            a.manager.update_settings(s);
        }
        true
    });

    place(header, 24, 24, WIDTH - 48, 52);
    place(card, 24, 96, WIDTH - 48, 70);
    window.set_surfaces(vec![(rect(0, 0, WIDTH, BODY), theme::VIEW), (rect(0, BODY, WIDTH, 1), theme::BORDER)]);
    // the buttons, and the drag square right of them, in the window's corner
    place(drag, WIDTH - 20 - DRAG.0, BODY + 16, DRAG.0, DRAG.1);
    canvas::set_tips(drag, &[(RECT { left: 0, top: 0, right: wnd::px(DRAG.0), bottom: wnd::px(DRAG.1) },
                              "Drag the file to a folder, a chat\u{2026} anywhere files go".into())]);
    let mut footer = Rows::new(20, BODY + 16, 0, WIDTH - 40 - DRAG.0 - 8);
    place(again, 20, BODY + 18, 150, 28);
    let bottom = footer.buttons(&[with, folder, open]);
    wnd::set_client_height(window.hwnd(), bottom + 16);
    wnd::center(window.hwnd());
    window.show_in_front();
}

/// The drag square: Windows' own "move" pointer in it (what dragging looks like), as a button
/// would look; pressed and moved, the file goes along with the mouse, to drop anywhere.
fn drag_box(window: &Window, path: std::path::PathBuf) -> windows::Win32::Foundation::HWND {
    let hover = Rc::new(Cell::new(false));
    let h = hover.clone();
    let hwnd = window.canvas(move |p| {
        p.fill(p.rect, theme::CHROME);
        let line = if h.get() { theme::ACCENT } else { theme::BORDER };
        let fill = if h.get() { theme::mix(theme::VIEW, theme::ACCENT, 0.12) } else { theme::VIEW };
        p.round(p.rect, wnd::px(theme::RADIUS), Some(fill), Some(line));
        let size = wnd::px(26); // (the pointer's picture has room around its arrows)
        // SAFETY: Windows' shared pointer, drawn into our canvas, centred
        unsafe {
            if let Ok(pointer) = LoadCursorW(None, IDC_SIZEALL) {
                let _ = DrawIconEx(p.gdi(), (p.rect.right - size) / 2, (p.rect.bottom - size) / 2, HICON(pointer.0),
                                   size, size, 0, None, DI_NORMAL);
            }
        }
    });
    let owner = window.hwnd();
    canvas::on_mouse(hwnd, move |event| match event {
        Mouse::Move(..) if !hover.replace(true) => canvas::redraw(hwnd),
        Mouse::Leave => {
            hover.set(false);
            canvas::redraw(hwnd);
        }
        Mouse::Down(x, y) => {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::Input::KeyboardAndMouse::DragDetect;
            let mut point = POINT { x, y };
            // SAFETY: plain calls on our own window; DragDetect waits for a drag or a release
            let dragging = unsafe {
                let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut point);
                DragDetect(hwnd, point).as_bool()
            };
            if dragging {
                shell::drag_files(owner, std::slice::from_ref(&path));
            }
            // SAFETY: the canvas took the mouse when it was pressed
            unsafe { let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture(); }
        }
        _ => {}
    });
    hwnd
}

/// The file's icon as Explorer shows it (the program's own, for a program): Windows' list of
/// icons of that size (SHIL_EXTRALARGE, SHIL_SMALL…) and its place in it.
fn icon_of(path: &Path, size: u32) -> Option<(IImageList, i32)> {
    let mut info = SHFILEINFOW::default();
    // SAFETY: queries into our struct; the image list is Windows' own
    unsafe {
        SHGetFileInfoW(&HSTRING::from(path.as_os_str()), FILE_ATTRIBUTE_NORMAL, Some(&mut info),
                       std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_SYSICONINDEX);
        Some((SHGetImageList::<IImageList>(size as i32).ok()?, info.iIcon))
    }
}
