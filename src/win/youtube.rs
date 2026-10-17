//! "YouTube in your browser": how to add TurboDM's full extension to Chrome, Edge and Brave by
//! hand. The copy they add from the Chrome Web Store leaves YouTube out (the store doesn't allow
//! extensions that download YouTube's videos); the full one has it, and where both are on, the
//! store's copy rests (extension/common/background.js). Step by step, with screenshots.

use super::theme::pictures::{self, Picture};
use super::theme::{self};
use super::wnd::{self, button_width, place, set_text, Rows, Window, FIELD, LINE};
use super::{shell, App};
use std::rc::Rc;
use turbodm::browsers as find;
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::*;

const WIDTH: i32 = 540;
const LEFT: i32 = 18;
const STEP: i32 = 40; // where a step's pictures and buttons start, under its words

pub fn open(_app: &Rc<App>) {
    let window = Window::dialog("YouTube in your browser", None, (WIDTH, 600), false);
    let heading = window.heading("YouTube videos in Chrome, Edge and Brave");
    let intro = window.note(
        "Google doesn't let extensions from the Chrome Web Store download YouTube videos, so the TurboDM \
         extension these browsers add by themselves leaves YouTube out; everything else works with it. \
         For YouTube too, add TurboDM's full extension by hand. It takes a minute, once:");
    let browsers: Vec<_> = find::installed().into_iter().filter(|b| !b.is_firefox()).collect();
    let added: Vec<_> = browsers.iter().filter(|b| b.has_full_extension()).map(|b| b.name).collect();

    // 1. the extensions page (browsers don't open their own pages for other programs: it's pasted)
    let step1 = window.label(if browsers.is_empty() {
        "1.  Open your browser's extensions page (chrome://extensions, edge://extensions\u{2026})."
    } else {
        "1.  Open your browser's extensions page:"
    });
    let told = window.note("Click yours: TurboDM opens it, with the page's address copied for its address bar.");
    let opens: Vec<_> = browsers.into_iter().map(|browser| {
        let text = if added.contains(&browser.name) { format!("{} (added)", browser.name) } else { browser.name.to_string() };
        window.button(&text, move || {
            let page = browser.extensions_page.unwrap_or("chrome://extensions");
            shell::copy_text(page);
            let opened = std::process::Command::new(&browser.program).spawn().is_ok();
            set_text(told, &if opened {
                format!("{page} is copied: paste it in {}'s address bar (Ctrl+V), and press Enter.", browser.name)
            } else {
                format!("Couldn't open {}: open it, and go to {page}.", browser.name)
            });
        })
    }).collect();

    // 2. Developer mode, and 3. Load unpacked, with the folder
    let step2 = window.label("2.  Turn on Developer mode, at the top right (in Edge, it's on the left):");
    let developer_mode = shot(&window, pictures::DEVELOPER_MODE);
    let step3 = window.label("3.  Click \"Load unpacked\", and choose TurboDM's extension folder:");
    let load_unpacked = shot(&window, pictures::LOAD_UNPACKED);
    let dir = find::extension_dir().map(|dir| dir.join("chrome"));
    let folder = window.label(&dir.as_ref().map_or("(not installed with this copy of TurboDM)".into(),
                                                     |dir| dir.display().to_string()));
    let d = dir.clone();
    let copy = window.button("Copy folder path", move || {
        if let Some(dir) = &d {
            shell::copy_text(&dir.to_string_lossy());
        }
    });
    let d = dir.clone();
    let show = window.button("Open folder", move || {
        if let Some(dir) = &d {
            shell::open(dir);
        }
    });
    wnd::enable(copy, dir.is_some());
    wnd::enable(show, dir.is_some());
    let done_note = window.note(
        "That's all: the video button shows on YouTube too, and the extension the browser added by \
         itself steps aside. If the browser offers to turn off developer-mode extensions, close that \
         message instead.");

    let w2 = window.clone();
    let done = window.default_button("Done", move || w2.destroy());
    let w2 = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w2.destroy());

    let mut rows = Rows::new(LEFT, 16, 0, WIDTH - 2 * LEFT);
    rows.full(heading, 24);
    rows.full(intro, 4 * LINE);
    rows.gap(2);
    rows.full(step1, LINE);
    if !opens.is_empty() {
        let mut x = STEP;
        for button in &opens {
            let width = button_width(*button);
            place(*button, x, rows.y, width, FIELD);
            x += width + 8;
        }
        rows.gap(FIELD + 6);
        place(told, STEP, rows.y, WIDTH - LEFT - STEP, 2 * LINE);
        rows.gap(2 * LINE + 4);
    }
    rows.full(step2, LINE);
    let (w, h) = pictures::shot_size(pictures::DEVELOPER_MODE);
    place(developer_mode, STEP, rows.y, w + 2, h + 2);
    rows.gap(h + 14);
    rows.full(step3, LINE);
    let (w, h) = pictures::shot_size(pictures::LOAD_UNPACKED);
    place(load_unpacked, STEP, rows.y, w + 2, h + 2);
    rows.gap(h + 12);
    place(folder, STEP, rows.y, WIDTH - LEFT - STEP, LINE);
    rows.gap(LINE + 6);
    place(copy, STEP, rows.y, button_width(copy), FIELD);
    place(show, STEP + button_width(copy) + 8, rows.y, button_width(show), FIELD);
    rows.gap(FIELD + 12);
    rows.full(done_note, 3 * LINE);
    rows.gap(4);
    let bottom = rows.buttons(&[done]);
    wnd::set_client_height(window.hwnd(), bottom + 16);
    wnd::center(window.hwnd());
    window.show();
}

/// A screenshot, at its size (`pictures::shot_size`), in a thin frame.
fn shot(window: &Window, png: Picture) -> windows::Win32::Foundation::HWND {
    window.canvas(move |p| {
        p.fill(p.rect, theme::VIEW);
        let one = wnd::px(1);
        p.shot(RECT { left: one, top: one, right: p.rect.right - one, bottom: p.rect.bottom - one }, png);
        p.round(p.rect, 0, None, Some(theme::BORDER));
    })
}
