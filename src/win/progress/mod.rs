//! Per-download window, as on Linux and like IDM's: opens when a download starts or resumes.
//! Tabs for the download status (with its connections), its speed limiter and what to do when
//! it's done.

mod details;
mod info;
mod options;
mod refresh;

pub use refresh::{close, refresh_all};

use super::theme;
use super::wnd::{self, rect, set_text, tabs::TABS, Window, FIELD};
use super::App;
use details::Details;
use info::{Bar, Info};
use std::cell::Cell;
use std::rc::Rc;
use turbodm::engine::Status;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct ProgressWindow {
    id: String,
    pub window: Rc<Window>,
    info: Info,
    bar: Bar,
    details: Rc<Details>,
    toggle: HWND,
}

const WIDTH: i32 = 640; // the window's inside, 96-DPI pixels
const PAD: i32 = 16; // around the pages' contents
const TOP: i32 = TABS + PAD; // where the pages' contents start
const BAR_Y: i32 = TOP + info::FIELDS.len() as i32 * info::ROW + 8;
const BAR: i32 = 26;

/// Where things go with or without the connections shown; returns the window's inside height.
fn arrange(window: &Window, details: &Details, buttons: [HWND; 3], shown: bool) -> i32 {
    let below_bar = BAR_Y + BAR;
    let page_bottom = if shown { details.place(PAD, below_bar + 16, WIDTH - 2 * PAD) } else { below_bar } + PAD;
    details.show(shown);
    // the pages are a lighter layer under the tabs, as on Linux
    window.set_surfaces(vec![(rect(0, TABS, WIDTH, 1), theme::BORDER),
                             (rect(0, TABS + 1, WIDTH, page_bottom - TABS - 1), theme::VIEW)]);
    let y = page_bottom + 14;
    wnd::place(buttons[0], PAD, y, 130, FIELD + 2);
    wnd::place(buttons[1], WIDTH - PAD - 2 * 96 - 8, y, 96, FIELD + 2);
    wnd::place(buttons[2], WIDTH - PAD - 96, y, 96, FIELD + 2);
    y + FIELD + 2 + 14
}

pub fn open(app: &Rc<App>, id: &str) {
    if let Some(existing) = app.progress.borrow().get(id) {
        existing.window.show();
        return;
    }
    let Some(snap) = app.manager.get(id) else { return };
    app.on_done.borrow_mut().entry(id.to_string()).or_default(); // being watched: say when it's done
    let window = Window::dialog(&snap.filename, None, (WIDTH, 400), true);
    let torrent = snap.torrent.is_some();
    let info = Info::new(&window, if torrent { &info::TORRENT_FIELDS } else { &info::FIELDS });
    wnd::place(info.hwnd, PAD, TOP, WIDTH - 2 * PAD, info::FIELDS.len() as i32 * info::ROW);
    let bar = Bar::new(&window);
    wnd::place(bar.hwnd, PAD, BAR_Y, WIDTH - 2 * PAD, BAR);
    let details = Rc::new(Details::new(&window, torrent));
    let limiter = options::speed_limiter(&window, app, &snap, PAD, TOP);
    let completion = options::on_completion(&window, app, id, PAD, TOP);

    let shown = Rc::new(Cell::new(true));
    let buttons = Rc::new(Cell::new([HWND::default(); 3]));
    let (d, s, b, w) = (details.clone(), shown.clone(), buttons.clone(), window.clone());
    let more = window.button("&Hide details", move || {
        s.set(!s.get());
        set_text(b.get()[0], if s.get() { "&Hide details" } else { "&Show details" });
        let height = arrange(&w, &d, b.get(), s.get());
        wnd::set_client_height(w.hwnd(), height);
    });
    let (a, i) = (app.clone(), id.to_string());
    let toggle = window.button("Pause", move || match a.manager.get(&i).map(|s| s.status) {
        Some(s) if s.is_active() || s == Status::Queued => a.manager.pause(&i),
        Some(_) => a.manager.resume(&i),
        None => {}
    });
    // SAFETY: plain query; Enter presses Pause / Resume, as in IDM
    window.set_default(unsafe { GetDlgCtrlID(toggle) } as u16);
    let (a, i, w) = (app.clone(), id.to_string(), window.hwnd());
    let cancel = window.button("Cancel", move || {
        a.manager.pause(&i); // IDM's Cancel: stop, keep what's downloaded
        a.on_done.borrow_mut().remove(&i);
        // SAFETY: closes our own window
        unsafe { let _ = DestroyWindow(w); }
    });
    buttons.set([more, toggle, cancel]);
    let height = arrange(&window, &details, buttons.get(), true);
    wnd::set_client_height(window.hwnd(), height);

    let pages = [vec![info.hwnd, bar.hwnd], limiter, completion];
    let (d, s) = (details.clone(), shown.clone());
    let show_page = move |page: usize| {
        for (i, controls) in pages.iter().enumerate() {
            controls.iter().for_each(|c| wnd::set_visible(*c, i == page));
        }
        d.show(page == 0 && s.get());
        wnd::set_visible(more, page == 0); // details belong to the status tab
    };
    show_page(0);
    let tabs = window.tabs(&["Download status", "Speed limiter", "Options on completion"], show_page);
    wnd::place(tabs, 0, 0, WIDTH, TABS);
    let (a, i) = (Rc::downgrade(app), id.to_string());
    window.hook(move |msg, _, _| {
        if let (WM_DESTROY, Some(app)) = (msg, a.upgrade()) {
            app.progress.borrow_mut().remove(&i);
        }
        None
    });
    wnd::center(window.hwnd());
    let pw = ProgressWindow { id: id.to_string(), window, info, bar, details, toggle };
    pw.refresh(&app.manager);
    pw.window.show();
    app.progress.borrow_mut().insert(id.to_string(), pw);
}
