//! "Add TurboDM to your browser", as on Windows (src/win/browsers.rs) and like IDM's guide: shown
//! on the first start and from the menu. Firefox is handed the signed extension and asks to add it
//! (one click); on the first start it's asked straight away when it's the default browser.
//! Chrome, Edge and Brave add it from their store once it's listed there; else the window says
//! how to load it by hand.

use super::launch::{launch, Launch};
use super::{frame, Ctx};
use gtk::prelude::*;
use std::path::Path;
use std::rc::Rc;
use turbodm::browsers as find;

/// The first time TurboDM starts (until the extension is seen working): Firefox, when it's the
/// default browser, asks to add it; the guide opens unless every browser has it already.
pub fn offer(ctx: &Rc<Ctx>) {
    if !ctx.manager.settings().browsers_offered {
        let missing = find::installed().into_iter().filter(|b| !b.has_extension()).collect::<Vec<_>>();
        if missing.is_empty() {
            return mark_offered(ctx);
        }
        if let (Some(firefox), Some(xpi)) = (missing.iter().find(|b| b.is_firefox()),
                                            find::extension_dir().and_then(|dir| find::firefox_extension(&dir))) {
            // (not from a test copy: it would open the real Firefox)
            if turbodm::config::sandbox().is_none() && firefox_is_default() {
                run(&firefox.program, Some(xpi.as_os_str()));
            }
        }
        open(ctx);
        mark_offered(ctx);
    }
}

/// A download came from the browser: the extension works, no need to offer it.
pub fn mark_offered(ctx: &Ctx) {
    let mut settings = ctx.manager.settings();
    if !settings.browsers_offered {
        settings.browsers_offered = true;
        ctx.manager.update_settings(settings);
    }
}

pub fn open(ctx: &Rc<Ctx>) {
    let dir = find::extension_dir();
    let browsers: Vec<_> = find::installed().into_iter().filter(|_| dir.is_some()).collect();
    let d = frame::dialog_with("Add TurboDM to your browser", None, 500, ":close", (16, 18));
    d.body.set_margin_bottom(16);
    d.body.append(&frame::heading("Catch downloads from your browser"));
    let from_store = turbodm::config::CHROME_STORE_ID.is_some();
    let intro = frame::note(if from_store {
        "Add the TurboDM extension to your browser: files you download there then go to TurboDM. \
         Chrome, Edge and Brave also ask by themselves: click \"Enable extension\" when they do."
    } else {
        "Add the TurboDM extension to your browser: files you download there then go to TurboDM."
    });
    intro.set_margin_top(frame::LINE_GAP);
    intro.set_margin_bottom(8);
    d.body.append(&intro);
    let guide = frame::note("");
    guide.set_visible(false);
    guide.set_margin_top(4);
    let copy = frame::button("Copy folder path");
    copy.set_halign(gtk::Align::Start);
    copy.set_visible(false);
    copy.set_margin_top(8);
    let c = ctx.clone();
    copy.connect_clicked(move |_| if let Some(dir) = find::extension_dir() {
        c.win.window.clipboard().set_text(&dir.join("chrome").to_string_lossy());
    });
    let none = browsers.is_empty() || dir.is_none();
    for browser in browsers {
        let added = browser.has_extension();
        let row = gtk::Box::builder().margin_bottom(6).build();
        let name = frame::label(&if added { format!("{}: added", browser.name) } else { browser.name.to_string() });
        let add = frame::button(if added { "Add again" } else { "Add" });
        add.set_width_request(88);
        add.set_margin_end(0);
        row.append(&name);
        row.append(&add);
        d.body.append(&row);
        let (c, dir, guide, copy) = (ctx.clone(), dir.clone().unwrap_or_default(), guide.clone(), copy.clone());
        let by_hand = browser.extensions_page.is_some() && browser.store_page().is_none();
        add.connect_clicked(move |_| {
            guide.set_text(&add_to(&c, &browser, &dir));
            guide.set_visible(true);
            copy.set_visible(by_hand);
        });
    }
    if none {
        d.body.append(&frame::note(if dir.is_none() { "The extension isn't installed with this copy of TurboDM." }
                                   else { "No browser was found. The extension is in the folder below." }));
    }
    // YouTube: the stores' copy leaves it out; the full extension, added by hand, has it
    if from_store && find::installed().iter().any(|b| !b.is_firefox()) {
        let row = gtk::Box::builder().spacing(16).margin_top(4).build();
        row.append(&frame::note("YouTube videos: the extension Chrome, Edge and Brave add from their store leaves them out."));
        let button = frame::button("Add YouTube\u{2026}");
        button.set_valign(gtk::Align::Center);
        let c = ctx.clone();
        button.connect_clicked(move |_| super::youtube::open(&c));
        row.append(&button);
        d.body.append(&row);
    }
    d.body.append(&guide);
    d.body.append(&copy);
    let folder = frame::button("Open extension folder");
    folder.set_sensitive(dir.is_some());
    let c = ctx.clone();
    folder.connect_clicked(move |_| if let Some(dir) = find::extension_dir() { launch(&c, &dir, Launch::Open) });
    let done = frame::default_button(&d.window, "Done");
    let w = d.window.clone();
    done.connect_clicked(move |_| w.close());
    let bottom = gtk::Box::builder().margin_top(14).build();
    folder.set_hexpand(true);
    folder.set_halign(gtk::Align::Start);
    bottom.append(&folder);
    bottom.append(&done);
    d.body.append(&bottom);
    d.window.present();
}

/// Start adding the extension to `browser`; returns what the user does next.
fn add_to(ctx: &Rc<Ctx>, browser: &find::Browser, dir: &Path) -> String {
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
    ctx.win.window.clipboard().set_text(page);
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
    std::process::Command::new("xdg-settings").args(["get", "default-web-browser"]).output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).to_lowercase().contains("firefox"))
}

pub fn run(program: &Path, arg: Option<&std::ffi::OsStr>) -> bool {
    std::process::Command::new(program).args(arg).stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_ok()
}
