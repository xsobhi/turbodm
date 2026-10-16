//! The slim bars on real controls (a window shown off the screen, never activated): Windows'
//! bar cut out, ours in its place; ours scrolls the control, and follows it when it scrolls
//! another way.

use super::{slim, SLIM};
use crate::win::wnd::{place, send, Window};
use std::rc::Rc;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetRgnBox, GetWindowRgn, CreateRectRgn, DeleteObject, SIMPLEREGION};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

fn window() -> Rc<Window> {
    // SAFETY: once per thread: the list view's class
    unsafe {
        let classes = INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_LISTVIEW_CLASSES };
        let _ = InitCommonControlsEx(&classes);
    }
    let window = Window::new("scroll bar test", WS_POPUP, WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, None, (400, 300));
    // SAFETY: our window, shown far off the screen without taking the focus
    unsafe { let _ = SetWindowPos(window.hwnd(), None, -30000, -30000, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW); }
    window
}

fn bars(control: HWND) -> [(HWND, bool, RECT); 2] {
    let slim = SLIM.with(|s| s.borrow().get(&(control.0 as isize)).cloned()).expect("slim");
    slim.bars.each_ref().map(|bar| {
        let mut r = RECT::default();
        // SAFETY: plain queries
        unsafe { let _ = GetWindowRect(bar.hwnd, &mut r); }
        (bar.hwnd, unsafe { IsWindowVisible(bar.hwnd) }.as_bool(), r)
    })
}

fn position(control: HWND) -> (i32, i32) {
    let slim = SLIM.with(|s| s.borrow().get(&(control.0 as isize)).cloned()).expect("slim");
    // SAFETY: plain query
    (slim.bars[0].range.get().pos, unsafe { GetScrollPos(control, SB_VERT) })
}

/// Windows' bar is cut out: the control shows only its inside.
fn cut_out(control: HWND) -> bool {
    let mut inside = RECT::default();
    // SAFETY: a region of ours to read the control's into, freed after
    unsafe {
        let _ = GetClientRect(control, &mut inside);
        let region = CreateRectRgn(0, 0, 0, 0);
        let mut r = RECT::default();
        let simple = GetWindowRgn(control, region) == SIMPLEREGION && GetRgnBox(region, &mut r) == SIMPLEREGION;
        let _ = DeleteObject(region.into());
        simple && r.right == inside.right && r.bottom == inside.bottom
    }
}

#[test]
fn on_a_text_field() {
    let window = window();
    let lines: Vec<String> = (1..=100).map(|i| format!("https://example.com/file-{i}.zip")).collect();
    let style = WS_VSCROLL | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL) as u32);
    let (edit, _) = window.control(WC_EDITW, &lines.join("\r\n"), style, WINDOW_EX_STYLE(0));
    place(edit, 10, 10, 300, 200);
    slim(edit);
    assert!(cut_out(edit));
    let [(_, shown, r), (_, across, _)] = bars(edit);
    assert!(shown && !across); // (only up and down)
    let mut at = RECT::default();
    // SAFETY: plain query
    unsafe { let _ = GetWindowRect(edit, &mut at); }
    assert_eq!((r.right, r.top, r.bottom), (at.right, at.top, at.bottom)); // where Windows' bar was
    let slim = SLIM.with(|s| s.borrow().get(&(edit.0 as isize)).cloned()).unwrap();
    slim.bars[0].scroll_to(50); // dragged there
    assert_eq!(position(edit), (50, 50));
    send(edit, EM_LINESCROLL, 0, -20); // scrolled another way: ours follows
    assert_eq!(position(edit), (30, 30));
}

#[test]
fn on_a_list() {
    let window = window();
    let style = WINDOW_STYLE(LVS_REPORT);
    let (list, _) = window.control(WC_LISTVIEWW, "", style, WINDOW_EX_STYLE(0));
    let title = HSTRING::from("Name");
    let column = LVCOLUMNW { mask: LVCF_TEXT | LVCF_WIDTH, cx: 900, pszText: PWSTR(title.as_ptr() as *mut _), ..Default::default() };
    send(list, LVM_INSERTCOLUMNW, 0, &column as *const _ as isize);
    for i in 0..100 {
        let text = HSTRING::from(format!("row {i}"));
        let item = LVITEMW { mask: LVIF_TEXT, iItem: i, pszText: PWSTR(text.as_ptr() as *mut _), ..Default::default() };
        send(list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
    }
    place(list, 10, 10, 300, 200);
    slim(list);
    assert!(cut_out(list));
    let [(_, down, _), (_, across, _)] = bars(list);
    assert!(down && across); // its column is wider than it: both
    let slim = SLIM.with(|s| s.borrow().get(&(list.0 as isize)).cloned()).unwrap();
    assert!(slim.bars[0].start.get() > 0); // below the column headers
    slim.bars[0].scroll_to(40);
    assert_eq!(send(list, LVM_GETTOPINDEX, 0, 0), 40);
    assert_eq!(position(list), (40, 40));
    send(list, LVM_ENSUREVISIBLE, 99, 0); // the last row shown: ours follows to the end
    let (pos, windows) = position(list);
    assert!(pos > 80 && pos == windows, "{pos}");
    slim.bars[1].scroll_to(200);
    // SAFETY: plain query
    assert_eq!(unsafe { GetScrollPos(list, SB_HORZ) }, 200);
}
