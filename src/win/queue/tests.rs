//! The hidden downloads' icon, with a stand-in for Windows (Shell_NotifyIcon): added while a
//! download window is hidden, its text kept up to date, taken away, and added again when
//! Explorer restarts or wasn't there yet.

use super::*;

thread_local! {
    static CALLS: RefCell<Vec<(NOTIFY_ICON_MESSAGE, u32, String)>> = RefCell::default();
    static WORKS: Cell<bool> = const { Cell::new(true) };
}

/// Instead of Windows: what was asked (the icon's id, its text), and whether it worked.
fn record(what: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
    let end = data.szTip.iter().position(|&c| c == 0).unwrap_or(0);
    if what != NIM_DELETE {
        assert_eq!(data.uCallbackMessage, WM_QUEUE);
        assert!(data.uFlags.contains(NIF_ICON | NIF_MESSAGE | NIF_TIP));
    }
    CALLS.with(|c| c.borrow_mut().push((what, data.uID, String::from_utf16_lossy(&data.szTip[..end]))));
    WORKS.with(Cell::get)
}

fn calls() -> Vec<(NOTIFY_ICON_MESSAGE, u32, String)> {
    CALLS.with(|c| c.borrow_mut().drain(..).collect())
}

#[test]
fn comes_and_goes_with_the_hidden_windows() {
    NOTIFY.with(|n| n.set(record));
    let hwnd = HWND::default();
    apply(hwnd, 0);
    assert_eq!(calls(), vec![]); // nothing hidden: no icon
    apply(hwnd, 1);
    assert_eq!(calls(), vec![(NIM_ADD, ID, "TurboDM: 1 download".to_string())]);
    apply(hwnd, 1);
    assert_eq!(calls(), vec![]); // (as it is)
    apply(hwnd, 3);
    assert_eq!(calls(), vec![(NIM_MODIFY, ID, "TurboDM: 3 downloads".to_string())]);
    taskbar_back(hwnd, 3); // Explorer restarted: added again
    assert_eq!(calls(), vec![(NIM_ADD, ID, "TurboDM: 3 downloads".to_string())]);
    apply(hwnd, 0);
    assert_eq!(calls(), vec![(NIM_DELETE, ID, String::new())]);
    apply(hwnd, 0);
    assert_eq!(calls(), vec![]);
}

#[test]
fn still_there_when_the_taskbar_says_it_is_new() {
    NOTIFY.with(|n| n.set(|what, data| record(what, data) && what != NIM_ADD)); // (Windows: it's there already)
    let hwnd = HWND::default();
    taskbar_back(hwnd, 1);
    assert_eq!(calls(), vec![(NIM_ADD, ID, "TurboDM: 1 download".to_string()), (NIM_MODIFY, ID, "TurboDM: 1 download".to_string())]);
    apply(hwnd, 0);
    assert_eq!(calls(), vec![(NIM_DELETE, ID, String::new())]); // (known to be there)
}

#[test]
fn added_later_when_the_taskbar_is_not_there() {
    NOTIFY.with(|n| n.set(record));
    WORKS.with(|w| w.set(false));
    let hwnd = HWND::default();
    apply(hwnd, 2);
    apply(hwnd, 2);
    assert_eq!(calls().iter().filter(|c| c.0 == NIM_ADD).count(), 2); // tried again (each with a NIM_MODIFY)
    WORKS.with(|w| w.set(true));
    taskbar_back(hwnd, 2);
    apply(hwnd, 2);
    assert_eq!(calls(), vec![(NIM_ADD, ID, "TurboDM: 2 downloads".to_string())]);
}
