//! "Add TurboDM to your browser", like IDM's guide: shown on the first start and from the menu.
//! Firefox is handed the signed extension and asks to add it (one click). Chrome, Edge and
//! Brave only take extensions from their stores, or unpacked by hand: their extensions page
//! is opened, with the steps and the folder to pick.

mod find;

use super::launch::{launch, Launch};
use super::Ctx;
use gtk::prelude::*;
use std::path::Path;
use std::rc::Rc;

/// The first time TurboDM starts (until the extension is seen working).
pub fn offer(ctx: &Rc<Ctx>) {
    if !ctx.manager.settings().browsers_offered {
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

fn text(label: &str) -> gtk::Label {
    gtk::Label::builder().label(label).xalign(0.0).wrap(true).max_width_chars(58).build()
}

pub fn open(ctx: &Rc<Ctx>) {
    let dir = find::extension_dir();
    let browsers = find::installed();
    let content = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(12)
        .margin_top(20).margin_bottom(20).margin_start(22).margin_end(22).build();
    content.append(&gtk::Label::builder().label("Catch downloads from your browser").xalign(0.0)
        .css_classes(["title-3"]).build());
    content.append(&text("Add the TurboDM extension to your browser: files you download there then go to TurboDM."));
    let list = gtk::Box::builder().orientation(gtk::Orientation::Vertical).spacing(6).margin_top(4).build();
    content.append(&list);
    let guide = text("");
    guide.set_visible(false);
    content.append(&guide);
    let copy = gtk::Button::builder().label("Copy folder path").halign(gtk::Align::Start).visible(false).build();
    content.append(&copy);
    if browsers.is_empty() || dir.is_none() {
        list.append(&text(if dir.is_none() { "The extension isn't installed with this copy of TurboDM." }
                          else { "No browser was found. The extension is in the folder below." }));
    }
    let window = gtk::Window::builder().title("Add TurboDM to your browser").default_width(520)
        .resizable(false).transient_for(&ctx.win.window).build();
    for browser in browsers.into_iter().filter(|_| dir.is_some()) {
        let row = gtk::Box::builder().spacing(12).build();
        row.append(&gtk::Label::builder().label(browser.name).xalign(0.0).hexpand(true).build());
        let add = gtk::Button::with_label("Add");
        row.append(&add);
        list.append(&row);
        let (ctx, dir, guide, copy) = (ctx.clone(), dir.clone().unwrap_or_default(), guide.clone(), copy.clone());
        let window = window.clone();
        add.connect_clicked(move |_| {
            guide.set_text(&add_to(&ctx, &browser, &dir));
            guide.set_visible(true);
            copy.set_visible(browser.extensions_page.is_some());
            super::center::fit_height(&window);
        });
    }
    let buttons = gtk::Box::builder().spacing(8).margin_top(8).build();
    let folder = gtk::Button::with_label("Open extension folder");
    folder.set_sensitive(dir.is_some());
    let done = gtk::Button::builder().label("Done").css_classes(["suggested-action"]).hexpand(true)
        .halign(gtk::Align::End).build();
    buttons.append(&folder);
    buttons.append(&done);
    content.append(&buttons);
    let c = ctx.clone();
    copy.connect_clicked(move |_| if let Some(dir) = find::extension_dir() {
        c.win.window.clipboard().set_text(&dir.join("chrome").to_string_lossy());
    });
    let c = ctx.clone();
    folder.connect_clicked(move |_| if let Some(dir) = find::extension_dir() { launch(&c, &dir, Launch::Open) });
    let w = window.clone();
    done.connect_clicked(move |_| w.close());
    window.set_child(Some(&content));
    window.set_default_widget(Some(&done));
    window.present();
    done.grab_focus(); // Enter closes it, rather than adding to the first browser
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
    // browsers don't open their own pages for other programs: the user pastes its address
    ctx.win.window.clipboard().set_text(page);
    if !run(&browser.program, None) {
        return format!("Couldn't open {}.", browser.name);
    }
    format!("{name} only adds extensions from its store on its own, so this \
             takes a few steps in {name}:\n\n\
             1.  Go to {page} (it's copied: paste it in the address bar with Ctrl+V).\n\
             2.  Turn on Developer mode.\n\
             3.  Click \"Load unpacked\" and choose this folder:\n      {folder}",
            name = browser.name, folder = dir.join("chrome").display())
}

fn run(program: &Path, arg: Option<&std::ffi::OsStr>) -> bool {
    std::process::Command::new(program).args(arg).stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().is_ok()
}
