//! Per-download window, like IDM's: opens when a download starts or resumes. Tabs for the
//! download status (with the connections), its speed limiter and what to do when it's done.

mod details;
mod options;
mod refresh;

pub use refresh::{close, refresh_all};

use super::wnd::{self, place, send, set_text, Window, FIELD, LINE};
use super::App;
use details::Details;
use std::cell::Cell;
use std::rc::Rc;
use turbodm::engine::Status;
use windows::core::{HSTRING, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LRESULT};
use windows::Win32::Graphics::Gdi::{GetSysColor, GetSysColorBrush, SetBkColor, COLOR_WINDOW, HDC};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct ProgressWindow {
    id: String,
    pub window: Rc<Window>,
    values: Vec<HWND>,
    bar: HWND,
    percent: HWND,
    details: Rc<Details>,
    toggle: HWND,
}

const FIELDS: [&str; 8] = ["Address", "Saved to", "Status", "File size", "Downloaded", "Transfer rate",
                           "Time left", "Resume capability"];
const WIDTH: i32 = 540; // the window's inside, 96-DPI pixels
const PAGE_X: i32 = 22; // where the tabs' contents start
const PAGE_Y: i32 = 42;

/// Where things go with or without the connections shown; returns the window's inside height.
fn arrange(tabs: HWND, details: &Details, buttons: [HWND; 3], shown: bool) -> i32 {
    let below_bar = PAGE_Y + FIELDS.len() as i32 * 22 + 40;
    let tabs_bottom = if shown { details.place(PAGE_X, below_bar, WIDTH - 2 * PAGE_X) + 12 } else { below_bar };
    details.show(shown);
    place(tabs, 10, 10, WIDTH - 20, tabs_bottom - 10);
    let y = tabs_bottom + 10;
    place(buttons[0], 10, y, 110, FIELD + 2);
    place(buttons[1], WIDTH - 10 - 2 * 92 - 8, y, 92, FIELD + 2);
    place(buttons[2], WIDTH - 10 - 92, y, 92, FIELD + 2);
    y + FIELD + 2 + 10
}

pub fn open(app: &Rc<App>, id: &str) {
    if let Some(existing) = app.progress.borrow().get(id) {
        existing.window.show();
        return;
    }
    let Some(snap) = app.manager.get(id) else { return };
    app.on_done.borrow_mut().entry(id.to_string()).or_default(); // being watched: say when it's done
    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    let window = Window::new(&snap.filename, style, WINDOW_EX_STYLE(0), None, (WIDTH, 300));
    let (tabs, tabs_id) = window.control(WC_TABCONTROLW, "", WS_CLIPSIBLINGS | WS_TABSTOP, WINDOW_EX_STYLE(0));
    for (i, name) in ["Download status", "Speed limiter", "Options on completion"].iter().enumerate() {
        let text = HSTRING::from(*name);
        let item = TCITEMW { mask: TCIF_TEXT, pszText: PWSTR(text.as_ptr() as *mut _), ..Default::default() };
        send(tabs, TCM_INSERTITEMW, i, &item as *const _ as isize);
    }
    let mut status_page = Vec::new();
    let mut values = Vec::new();
    for (i, name) in FIELDS.iter().enumerate() {
        let y = PAGE_Y + i as i32 * 22;
        let (label, value) = (window.label(name), window.label(""));
        place(label, PAGE_X, y, 120, LINE);
        place(value, PAGE_X + 125, y, WIDTH - 2 * PAGE_X - 125, LINE);
        status_page.extend([label, value]);
        values.push(value);
    }
    let bar_y = PAGE_Y + FIELDS.len() as i32 * 22 + 6;
    let bar = window.progress_bar();
    let percent = window.label("");
    place(bar, PAGE_X, bar_y, WIDTH - 2 * PAGE_X - 60, 18);
    place(percent, WIDTH - PAGE_X - 52, bar_y + 1, 52, LINE);
    status_page.extend([bar, percent]);
    let details = Rc::new(Details::new(&window));
    let limiter = options::speed_limiter(&window, app, &snap, PAGE_X, PAGE_Y + 4);
    let completion = options::on_completion(&window, app, id, PAGE_X, PAGE_Y + 4);

    let shown = Rc::new(Cell::new(true));
    let buttons = Rc::new(Cell::new([HWND::default(); 3]));
    let (d, s, b, w) = (details.clone(), shown.clone(), buttons.clone(), window.hwnd());
    let more = window.button("&Hide details", move || {
        s.set(!s.get());
        set_text(b.get()[0], if s.get() { "&Hide details" } else { "&Show details" });
        let height = arrange(tabs, &d, b.get(), s.get());
        wnd::set_client_height(w, height);
    });
    let (a, i) = (app.clone(), id.to_string());
    let toggle = window.default_button("Pause", move || match a.manager.get(&i).map(|s| s.status) {
        Some(s) if s.is_active() || s == Status::Queued => a.manager.pause(&i),
        Some(_) => a.manager.resume(&i),
        None => {}
    });
    let (a, i, w) = (app.clone(), id.to_string(), window.hwnd());
    let cancel = window.button("Cancel", move || {
        a.manager.pause(&i); // IDM's Cancel: stop, keep what's downloaded
        a.on_done.borrow_mut().remove(&i);
        // SAFETY: closes our own window
        unsafe { let _ = DestroyWindow(w); }
    });
    buttons.set([more, toggle, cancel]);
    let height = arrange(tabs, &details, buttons.get(), true);
    wnd::set_client_height(window.hwnd(), height);

    let pages = [status_page, limiter, completion];
    let details_shown = details.clone();
    let show_page = move |page: usize| {
        for (i, controls) in pages.iter().enumerate() {
            controls.iter().for_each(|c| wnd::set_visible(*c, i == page));
        }
        details_shown.show(page == 0 && shown.get());
        wnd::set_visible(more, page == 0); // details belong to the status tab
    };
    show_page(0);
    window.on_notify(tabs_id, move |header, _| {
        if header.code == TCN_SELCHANGE {
            show_page(send(tabs, TCM_GETCURSEL, 0, 0).max(0) as usize);
        }
        None
    });
    // labels and check boxes sit on the tabs' page, which is white
    window.hook(|msg, wparam, _| (msg == WM_CTLCOLORSTATIC).then(|| unsafe {
        // SAFETY: the DC Windows passes for drawing the control
        SetBkColor(HDC(wparam.0 as *mut _), COLORREF(GetSysColor(COLOR_WINDOW)));
        LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
    }));
    let (a, i) = (Rc::downgrade(app), id.to_string());
    window.hook(move |msg, _, _| {
        if let (WM_DESTROY, Some(app)) = (msg, a.upgrade()) {
            app.progress.borrow_mut().remove(&i);
        }
        None
    });
    wnd::center(window.hwnd());
    let pw = ProgressWindow { id: id.to_string(), window, values, bar, percent, details, toggle };
    pw.refresh(&app.manager);
    pw.window.show();
    app.progress.borrow_mut().insert(id.to_string(), pw);
}
