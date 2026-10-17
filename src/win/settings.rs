//! Options, as IDM's: tabs (General, Save to, Connection), each a few short groups with a
//! heading, what each setting is on the left and its switch, number or folder on the right;
//! OK and Cancel at the bottom, as in Windows' own dialogs.

use super::theme::{self, Font};
use super::wnd::labels::Kind;
use super::wnd::tabs::TABS;
use super::wnd::{self, enable, is_checked, place, rect, set_text, set_visible, text, Window, FIELD, LINE};
use super::{browsers, helpers, shell, App};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::config::MAX_CONNECTIONS;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The window's width; the names' column; around the pages; a row (96-DPI pixels).
const WIDTH: i32 = 600;
const NAMES: i32 = 330;
const PAD: i32 = 22;
const ROW: i32 = 38;
const CONTROL_X: i32 = PAD + NAMES + 16;
const CONTROL: i32 = WIDTH - PAD - CONTROL_X; // the controls' column

/// A page's line: a group's heading, or a setting (its name and its control, `width` wide).
enum Line {
    Heading(&'static str),
    Setting(&'static str, HWND, i32),
}

pub fn open(app: &Rc<App>) {
    let s = app.manager.settings();
    let window = Window::dialog("Options", Some(app.main.window.hwnd()), (WIDTH, 480), false);

    // Save to: the folder, category folders, the folders remembered for kinds of files
    let folder = Rc::new(RefCell::new(s.download_dir.clone()));
    let (f, w) = (folder.clone(), window.hwnd());
    let button = Rc::new(Cell::new(HWND::default()));
    let b = button.clone();
    let folder_button = window.button(&s.download_dir.display().to_string(), move || {
        if let Some(dir) = shell::choose_folder(w, "Download folder", &f.borrow().clone()) {
            set_text(b.get(), &dir.display().to_string());
            *f.borrow_mut() = dir;
        }
    });
    button.set(folder_button);
    let forget_all = Rc::new(Cell::new(false));
    let remembered = s.folders.len();
    let (fa, button) = (forget_all.clone(), Rc::new(Cell::new(HWND::default())));
    let b = button.clone();
    let forget = window.button(&format!("Forget ({remembered})"), move || {
        fa.set(true);
        set_text(b.get(), "Forgotten");
        enable(b.get(), false);
    });
    button.set(forget);
    enable(forget, remembered > 0);
    let categories = window.switch(s.use_categories, |_| {});

    // Connection: how many, picked from a list (easier than typing); the other numbers typed
    let pick = |values: &[usize], now: usize| {
        let mut values: Vec<usize> = values.to_vec();
        if !values.contains(&now) {
            values.push(now); // (a number set before: kept)
            values.sort_unstable();
        }
        let names: Vec<String> = values.iter().map(|v| v.to_string()).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let chosen = Rc::new(Cell::new(now));
        let c = chosen.clone();
        let list = values.clone();
        let hwnd = window.choice(&names, values.iter().position(|v| *v == now).unwrap_or(0),
                                 move |i| c.set(list[i]));
        (hwnd, chosen)
    };
    let (connections, connections_chosen) = pick(&[1, 2, 4, 8, 16, 24, MAX_CONNECTIONS], s.connections);
    let (parallel, parallel_chosen) = pick(&[1, 2, 3, 4, 5, 6, 8, 10, 12, 16], s.max_parallel);
    let spin = |min: i32, max: i32, value: i32| window.spin(min, max, value, |_| {});
    let speed = spin(0, 10_000_000, s.speed_limit_kib as i32);
    let retries = spin(0, 100, s.retries as i32);
    let timeout = spin(5, 300, s.timeout_secs as i32);

    // General: switches
    let switch = |on: bool| window.switch(on, |_| {});
    let (confirm, predownload) = (switch(s.show_add_dialog), switch(s.predownload));
    let (progress, complete, notify) = (switch(s.show_progress_window), switch(s.show_complete_dialog), switch(s.notify_complete));
    let (clipboard, resume, updates) = (switch(s.clipboard_monitor), switch(s.auto_resume), switch(s.check_updates));
    let seed = switch(s.seed);
    let a = app.clone();
    let add_browser = window.button("Add to your browser\u{2026}", move || browsers::open(&a));
    // the helpers videos need (yt-dlp, FFmpeg): here, or got from here (the setup window asks)
    let a = app.clone();
    let helpers_ready = helpers::ready();
    let video_helpers = window.button(if helpers_ready { "Installed" } else { "Get them\u{2026}" },
                                      move || helpers::with_helpers(&a, |_| {}));
    enable(video_helpers, !helpers_ready);
    let a = app.clone();
    let youtube = window.button("Show how\u{2026}", move || super::youtube::open(&a));

    use Line::{Heading, Setting};
    let pages: [Vec<Line>; 3] = [
        vec![Heading("Downloads from the browser"),
             Setting("Ask where to save them (Download file info)", confirm, 44),
             Setting("Start downloading while it's open", predownload, 44),
             Setting("Browser integration", add_browser, 200),
             Heading("Videos"),
             Setting("Helpers for videos (yt-dlp and FFmpeg)", video_helpers, 130),
             Setting("YouTube in Chrome, Edge and Brave", youtube, 130),
             Heading("Windows and notices"),
             Setting("Open a progress window when a download starts", progress, 44),
             Setting("Show the Download complete window", complete, 44),
             Setting("Notify when downloads finish", notify, 44),
             Heading("At start, and meanwhile"),
             Setting("Resume unfinished downloads at start", resume, 44),
             Setting("Catch download links copied to the clipboard", clipboard, 44),
             Setting("Check for updates", updates, 44)],
        vec![Heading("Where downloads go"),
             Setting("Download folder", folder_button, CONTROL),
             Setting("Sort into category folders (Video, Music\u{2026})", categories, 44),
             Setting("Folders remembered for kinds of files", forget, 130)],
        vec![Heading("Each download"),
             Setting("Connections at once (servers may limit them)", connections, 150),
             Setting("Retries per connection", retries, 150),
             Setting("Reconnect after no data for (seconds)", timeout, 150),
             Heading("All downloads"),
             Setting("Downloads at the same time", parallel, 150),
             Setting("Speed limit, KB/s (0 = no limit)", speed, 150),
             Heading("Torrents"),
             Setting("Keep sharing finished torrents", seed, 44)],
    ];

    // lay the pages out over each other, under the tabs
    let mut shown: Vec<Vec<HWND>> = vec![];
    let mut bottom = 0;
    for page in &pages {
        let mut y = TABS + PAD - 6;
        let mut controls = vec![];
        for (i, line) in page.iter().enumerate() {
            match line {
                Heading(name) => {
                    y += if i == 0 { 0 } else { 12 };
                    let heading = window.text_label(name, Font::Strong, Kind::Line);
                    place(heading, PAD, y, WIDTH - 2 * PAD, LINE + 2);
                    controls.push(heading);
                    y += LINE + 10;
                }
                Setting(name, control, width) => {
                    let label = window.label(name);
                    place(label, PAD + 12, y + (FIELD - LINE) / 2, NAMES - 12, LINE);
                    place(*control, CONTROL_X + CONTROL - width, y, *width, FIELD);
                    controls.extend([label, *control]);
                    y += ROW;
                }
            }
        }
        bottom = bottom.max(y);
        shown.push(controls);
    }
    let page_bottom = bottom + PAD - 10;
    window.set_surfaces(vec![(rect(0, TABS, WIDTH, 1), theme::BORDER),
                             (rect(0, TABS + 1, WIDTH, page_bottom - TABS - 1), theme::VIEW),
                             (rect(0, page_bottom, WIDTH, 1), theme::BORDER)]);
    let show_page = move |page: usize| {
        for (i, controls) in shown.iter().enumerate() {
            controls.iter().for_each(|c| set_visible(*c, i == page));
        }
    };
    show_page(0);
    let tabs = window.tabs(&["General", "Save to", "Connection"], show_page);
    place(tabs, 0, 0, WIDTH, TABS);

    let (a, w) = (app.clone(), window.clone());
    let ok = window.default_button("OK", move || {
        let mut s = a.manager.settings();
        s.download_dir = folder.borrow().clone();
        if forget_all.get() {
            s.folders.clear();
        }
        s.use_categories = is_checked(categories);
        let number = |field: HWND| text(field).parse::<i64>().unwrap_or(0).max(0);
        (s.connections, s.max_parallel) = (connections_chosen.get(), parallel_chosen.get());
        (s.speed_limit_kib, s.retries, s.timeout_secs) = (number(speed) as u64, number(retries) as u32, number(timeout) as u64);
        (s.show_add_dialog, s.predownload) = (is_checked(confirm), is_checked(predownload));
        (s.show_progress_window, s.show_complete_dialog, s.notify_complete) =
            (is_checked(progress), is_checked(complete), is_checked(notify));
        (s.clipboard_monitor, s.auto_resume, s.check_updates) = (is_checked(clipboard), is_checked(resume), is_checked(updates));
        s.seed = is_checked(seed);
        a.manager.update_settings(s.clamp());
        w.destroy();
    });
    let w = window.clone();
    let cancel = window.button("Cancel", move || w.destroy());
    let w = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w.destroy());
    let y = page_bottom + 14;
    place(ok, WIDTH - PAD - 96, y, 96, FIELD + 2);
    place(cancel, WIDTH - PAD - 2 * 96 - 8, y, 96, FIELD + 2);
    wnd::set_client_height(window.hwnd(), y + FIELD + 2 + 14);
    wnd::center(window.hwnd());
    window.show();
}
