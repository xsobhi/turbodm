//! Open windows in the middle of the screen. GTK 4 can't position windows, and X11 window
//! managers (Cinnamon, GNOME, …) place ordinary windows in a corner but centre dialogs, so the
//! window is announced as a dialog. Dialogs can't be minimized, so `minimizable` windows turn
//! back into normal windows once placed. On Wayland the compositor decides; this does nothing.

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::glib;
use gtk::prelude::*;
use std::time::Duration;

fn set_type(w: &gtk::Window, kind: &std::ffi::CStr) {
    let surface = w.native().and_then(|n| n.surface()).and_then(|s| s.downcast::<X11Surface>().ok());
    let (Some(surface), Ok(display)) = (surface, WidgetExt::display(w).downcast::<X11Display>()) else { return };
    let Ok(x) = xlib::Xlib::open() else { return };
    // SAFETY: GDK's own Xlib connection and window, used on the GTK thread
    unsafe {
        let dpy = display.xdisplay();
        let prop = (x.XInternAtom)(dpy, c"_NET_WM_WINDOW_TYPE".as_ptr(), 0);
        let value = (x.XInternAtom)(dpy, kind.as_ptr(), 0);
        (x.XChangeProperty)(dpy, surface.xid(), prop, xlib::XA_ATOM, 32, xlib::PropModeReplace,
                            (&value as *const xlib::Atom).cast(), 1);
    }
}

pub fn on_screen(window: &gtk::Window, minimizable: bool) {
    window.connect_realize(|w| set_type(w, c"_NET_WM_WINDOW_TYPE_DIALOG"));
    if minimizable {
        window.connect_map(|w| {
            let w = w.clone();
            glib::timeout_add_local_once(Duration::from_millis(400), move || {
                set_type(&w, c"_NET_WM_WINDOW_TYPE_NORMAL"); // placed: now allow minimize
            });
        });
    }
}
