//! "YouTube in your browser", as on Windows (src/win/youtube.rs): how to add TurboDM's full
//! extension to Chrome, Edge and Brave by hand. The copy they add from the Chrome Web Store leaves
//! YouTube out (the store doesn't allow extensions that download YouTube's videos); the full one
//! has it, and where both are on, the store's copy rests. Step by step, with screenshots.

use super::launch::{launch, Launch};
use super::{browsers, frame, Ctx};
use gtk::prelude::*;
use gtk::{gdk, glib};
use std::rc::Rc;
use turbodm::browsers as find;

const STEP: i32 = 22; // where a step's pictures and buttons start, under its words (past the margin)

fn shot(png: &'static [u8]) -> gtk::Widget {
    let picture = gtk::Picture::new();
    if let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from_static(png)) {
        // at half its pixels, as on Windows (they're taken at 200%)
        picture.set_size_request(texture.width() / 2, texture.height() / 2);
        picture.set_paintable(Some(&texture));
    }
    picture.set_can_shrink(true);
    let framed = gtk::Box::builder().css_classes(["guide-picture"]).halign(gtk::Align::Start).margin_start(STEP)
        .margin_top(6).build();
    framed.append(&picture);
    framed.upcast()
}

fn step(text: &str) -> gtk::Label {
    let label = frame::label(text);
    label.set_margin_top(10);
    label
}

pub fn open(ctx: &Rc<Ctx>) {
    let d = frame::dialog_with("YouTube in your browser", None, 540, ":close", (16, 18));
    d.body.set_margin_bottom(16);
    d.body.append(&frame::heading("YouTube videos in Chrome, Edge and Brave"));
    let intro = frame::note(
        "Google doesn't let extensions from the Chrome Web Store download YouTube videos, so the TurboDM \
         extension these browsers add by themselves leaves YouTube out; everything else works with it. \
         For YouTube too, add TurboDM's full extension by hand. It takes a minute, once:");
    intro.set_margin_top(frame::LINE_GAP);
    d.body.append(&intro);
    let browsers: Vec<_> = find::installed().into_iter().filter(|b| !b.is_firefox()).collect();
    let added: Vec<_> = browsers.iter().filter(|b| b.has_full_extension()).map(|b| b.name).collect();

    // 1. the extensions page (browsers don't open their own pages for other programs: it's pasted)
    d.body.append(&step(if browsers.is_empty() {
        "1.  Open your browser's extensions page (chrome://extensions, edge://extensions\u{2026})."
    } else {
        "1.  Open your browser's extensions page:"
    }));
    if !browsers.is_empty() {
        let told = frame::note("Click yours: TurboDM opens it, with the page's address copied for its address bar.");
        told.set_margin_start(STEP);
        told.set_margin_top(6);
        let row = gtk::Box::builder().spacing(8).margin_start(STEP).margin_top(6).build();
        for browser in browsers {
            let text = if added.contains(&browser.name) { format!("{} (added)", browser.name) } else { browser.name.to_string() };
            let button = gtk::Button::builder().label(text).height_request(frame::FIELD).build();
            let (c, told) = (ctx.clone(), told.clone());
            button.connect_clicked(move |_| {
                let page = browser.extensions_page.unwrap_or("chrome://extensions");
                c.win.window.clipboard().set_text(page);
                let opened = browsers::run(&browser.program, None);
                told.set_text(&if opened {
                    format!("{page} is copied: paste it in {}'s address bar (Ctrl+V), and press Enter.", browser.name)
                } else {
                    format!("Couldn't open {}: open it, and go to {page}.", browser.name)
                });
            });
            row.append(&button);
        }
        d.body.append(&row);
        d.body.append(&told);
    }

    // 2. Developer mode, and 3. Load unpacked, with the folder
    d.body.append(&step("2.  Turn on Developer mode, at the top right (in Edge, it's on the left):"));
    d.body.append(&shot(include_bytes!("../../data/guide/developer-mode.png")));
    d.body.append(&step("3.  Click \"Load unpacked\", and choose TurboDM's extension folder:"));
    d.body.append(&shot(include_bytes!("../../data/guide/load-unpacked.png")));
    let dir = find::extension_dir().map(|dir| dir.join("chrome"));
    let folder = frame::label(&dir.as_ref().map_or("(not installed with this copy of TurboDM)".into(),
                                                     |dir| dir.display().to_string()));
    folder.set_margin_start(STEP);
    folder.set_margin_top(10);
    folder.set_selectable(true);
    d.body.append(&folder);
    let row = gtk::Box::builder().spacing(8).margin_start(STEP).margin_top(6).build();
    let copy = gtk::Button::builder().label("Copy folder path").height_request(frame::FIELD).sensitive(dir.is_some()).build();
    let show = gtk::Button::builder().label("Open folder").height_request(frame::FIELD).sensitive(dir.is_some()).build();
    let (c, d2) = (ctx.clone(), dir.clone());
    copy.connect_clicked(move |_| if let Some(dir) = &d2 { c.win.window.clipboard().set_text(&dir.to_string_lossy()) });
    let (c, d2) = (ctx.clone(), dir.clone());
    show.connect_clicked(move |_| if let Some(dir) = &d2 { launch(&c, dir, Launch::Open) });
    row.append(&copy);
    row.append(&show);
    d.body.append(&row);
    let done_note = frame::note(
        "That's all: the video button shows on YouTube too, and the extension the browser added by \
         itself steps aside. If the browser offers to turn off developer-mode extensions, close that \
         message instead.");
    done_note.set_margin_top(12);
    d.body.append(&done_note);
    let done = frame::default_button(&d.window, "Done");
    let w = d.window.clone();
    done.connect_clicked(move |_| w.close());
    frame::buttons(&d.body, &[&done], 8);
    d.window.present();
}
