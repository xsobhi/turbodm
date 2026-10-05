//! Every window opens in the middle of the screen, and resizing a shown window to fit its
//! content. GTK 4 can't position windows and window managers mostly put new ones in a corner,
//! so it's done for the platform: Xlib on X11, Win32 on Windows. On Wayland the compositor
//! decides (nothing happens here).

use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
use x11 as platform;
#[cfg(windows)]
mod win32;
#[cfg(windows)]
use win32 as platform;
#[cfg(not(any(target_os = "linux", windows)))]
mod platform {
    pub fn realized(_: &gtk::Window) {}
    pub fn placed(_: &gtk::Window) {}
    pub fn center(_: &gtk::Window, placed: (i32, i32)) -> (i32, i32) { placed }
    pub fn set_height(_: &gtk::Window, _: i32) {}
}

/// Center each window of the app the first time it's shown (call once at start).
pub fn all_windows() {
    let windows = gtk::Window::toplevels();
    windows.connect_items_changed(|list, position, _removed, added| {
        for i in position..position + added {
            if let Some(window) = list.item(i).and_downcast::<gtk::Window>() {
                watch(&window);
            }
        }
    });
    for i in 0..windows.n_items() {
        if let Some(window) = windows.item(i).and_downcast::<gtk::Window>() {
            watch(&window);
        }
    }
}

fn watch(window: &gtk::Window) {
    window.connect_realize(platform::realized);
    let handler = Rc::new(RefCell::new(None));
    let h = handler.clone();
    *handler.borrow_mut() = Some(window.connect_map(move |w| {
        if let Some(id) = h.borrow_mut().take() {
            w.disconnect(id); // only the first time: later the user decides where it goes
        }
        // GTK sizes the window over the first frames after mapping it: follow that a moment
        let (window, mut ticks, mut placed) = (w.clone(), 0, (0, 0));
        glib::timeout_add_local(Duration::from_millis(10), move || {
            ticks += 1;
            placed = platform::center(&window, placed);
            if ticks < 30 {
                return glib::ControlFlow::Continue;
            }
            platform::placed(&window);
            glib::ControlFlow::Break
        });
    }));
}

/// Shrink or grow a shown window to the height its content needs now (GTK keeps a window's
/// size once it's shown).
pub fn fit_height(window: &gtk::Window) {
    let height = window.measure(gtk::Orientation::Vertical, window.width()).1;
    window.set_default_size(window.width(), height); // honoured on X11
    // Windows: resize once GTK has laid the window out again, or the old minimum height
    // (still in force until then) keeps it from shrinking
    let window = window.clone();
    glib::timeout_add_local_once(Duration::from_millis(120), move || {
        let height = window.measure(gtk::Orientation::Vertical, window.width()).1;
        platform::set_height(&window, height);
    });
}
