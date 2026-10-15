//! "Add TurboDM to your browser", like IDM's guide: shown on the first start and from the menu.
//! Firefox is handed the signed extension and asks to add it (one click); on the first start it's
//! asked straight away, as IDM does. Chrome, Edge and Brave add it from their store by themselves
//! once it's listed there (register.rs), asking first; else the window says how to load it.

use super::wnd::{self, place, set_text, set_visible, Rows, Window, FIELD, LINE};
use super::{shell, App};
use std::path::Path;
use std::rc::Rc;
use turbodm::browsers as find;
use windows::Win32::UI::WindowsAndMessaging::*;

/// The first time TurboDM starts (until the extension is seen working): Firefox, when it's the
/// default browser, asks to add it; the guide opens unless every browser has it already.
pub fn offer(app: &Rc<App>) {
    if !app.manager.settings().browsers_offered {
        let missing = find::installed().into_iter().filter(|b| !b.has_extension()).collect::<Vec<_>>();
        if missing.is_empty() {
            return mark_offered(app);
        }
        if let (Some(firefox), Some(xpi)) = (missing.iter().find(|b| b.is_firefox()),
                                            find::extension_dir().and_then(|dir| find::firefox_extension(&dir))) {
            // (not from a test copy: it would open the real Firefox)
            if !wnd::moves::offscreen() && turbodm::config::sandbox().is_none() && firefox_is_default() {
                run(&firefox.program, Some(xpi.as_os_str()));
            }
        }
        open(app);
        mark_offered(app);
    }
}

/// A download came from the browser: the extension works, no need to offer it.
pub fn mark_offered(app: &App) {
    let mut settings = app.manager.settings();
    if !settings.browsers_offered {
        settings.browsers_offered = true;
        app.manager.update_settings(settings);
    }
}

pub fn open(_app: &Rc<App>) {
    let dir = find::extension_dir();
    let browsers: Vec<_> = find::installed().into_iter().filter(|_| dir.is_some()).collect();
    let window = Window::dialog("Add TurboDM to your browser", None, (500, 300), false);
    let heading = window.heading("Catch downloads from your browser");
    let from_store = turbodm::config::CHROME_STORE_ID.is_some();
    let intro = window.note(if from_store {
        "Add the TurboDM extension to your browser: files you download there then go to TurboDM. \
         Chrome, Edge and Brave also ask by themselves: click \"Enable extension\" when they do."
    } else {
        "Add the TurboDM extension to your browser: files you download there then go to TurboDM."
    });
    let guide = window.note("");
    set_visible(guide, false);
    let copy = window.button("Copy folder path", || {
        if let Some(dir) = find::extension_dir() {
            shell::copy_text(&dir.join("chrome").to_string_lossy());
        }
    });
    set_visible(copy, false);
    let missing = (browsers.is_empty() || dir.is_none()).then(|| window.note(if dir.is_none() {
        "The extension isn't installed with this copy of TurboDM."
    } else {
        "No browser was found. The extension is in the folder below."
    }));
    let mut rows = Rows::new(18, 16, 0, 464);
    rows.full(heading, 24);
    rows.full(intro, if from_store { 48 } else { 32 });
    for browser in browsers {
        let added = browser.has_extension();
        let name = window.label(&if added { format!("{}: added", browser.name) } else { browser.name.to_string() });
        let dir = dir.clone().unwrap_or_default();
        let by_hand = browser.extensions_page.is_some() && browser.store_page().is_none();
        let add = window.button(if added { "Add again" } else { "Add" }, move || {
            set_text(guide, &add_to(&browser, &dir).replace('\n', "\r\n"));
            set_visible(guide, true);
            set_visible(copy, by_hand);
        });
        place(name, 18, rows.y + 4, 300, LINE);
        place(add, 394, rows.y, 88, FIELD);
        rows.gap(FIELD + 6);
    }
    if let Some(missing) = missing {
        rows.full(missing, 32);
    }
    rows.gap(4);
    rows.full(guide, 7 * LINE);
    place(copy, 18, rows.y, 130, FIELD);
    rows.gap(FIELD + 14);
    let folder = window.button("Open extension folder", || {
        if let Some(dir) = find::extension_dir() {
            shell::open(&dir);
        }
    });
    wnd::enable(folder, dir.is_some());
    let w2 = window.clone();
    let done = window.default_button("Done", move || w2.destroy());
    let w2 = window.clone();
    window.on_command(IDCANCEL.0 as u16, move |_| w2.destroy());
    place(folder, 18, rows.y, 150, FIELD + 2);
    let bottom = rows.buttons(&[done]);
    wnd::set_client_height(window.hwnd(), bottom + 16);
    wnd::center(window.hwnd());
    window.show();
}

/// Start adding the extension to `browser`; returns what the user does next.
fn add_to(browser: &find::Browser, dir: &Path) -> String {
    let Some(page) = browser.extensions_page else {
        return match find::firefox_extension(dir) {
            Some(xpi) if run(&browser.program, Some(xpi.as_os_str())) =>
                "Firefox asks whether to add TurboDM: click Add, then Okay.".into(),
            _ => "Couldn't open Firefox.".into(),
        };
    };
    // in the store: its page opens in that browser, with the store's own Add button
    if let Some(store) = browser.store_page() {
        if !run(&browser.program, Some(std::ffi::OsStr::new(&store))) {
            return format!("Couldn't open {}.", browser.name);
        }
        return match (browser.name == "Microsoft Edge", store.contains("chromewebstore")) {
            (true, false) => "Edge opens TurboDM in Edge Add-ons: click \"Get\", then \"Add extension\".".into(),
            (true, true) => "Edge opens TurboDM in the Chrome Web Store: click \"Allow extensions from other \
                             stores\", then \"Add to Chrome\", then \"Add extension\".".into(),
            _ => format!("{} opens TurboDM in the Chrome Web Store: click \"Add to\u{2026}\", then \"Add extension\".",
                         browser.name),
        };
    }
    // browsers don't open their own pages for other programs: the user pastes its address
    shell::copy_text(page);
    if !run(&browser.program, None) {
        return format!("Couldn't open {}.", browser.name);
    }
    format!("{name} only adds extensions from its store on its own, so this takes a few steps in {name}:\n\n\
             1.  Go to {page} (it's copied: paste it in the address bar with Ctrl+V).\n\
             2.  Turn on Developer mode.\n\
             3.  Click \"Load unpacked\" and choose this folder:\n      {folder}",
            name = browser.name, folder = dir.join("chrome").display())
}

/// Does the user browse with Firefox? (Opening it by itself is only welcome then.)
fn firefox_is_default() -> bool {
    let key = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice";
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(key)
        .and_then(|k| k.get_value::<String, _>("ProgId"))
        .is_ok_and(|id| id.starts_with("Firefox"))
}

fn run(program: &Path, arg: Option<&std::ffi::OsStr>) -> bool {
    std::process::Command::new(program).args(arg).spawn().is_ok()
}
