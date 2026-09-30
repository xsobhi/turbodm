//! Open parentless windows in the middle of the screen. GTK 4 can't position windows, and X11
//! window managers (Cinnamon, GNOME, …) place ordinary windows in a corner but centre dialogs,
//! so windows shown without a visible parent (e.g. when the browser starts us) are marked as
//! dialogs. On Wayland the compositor decides and this does nothing.

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::prelude::*;

pub fn as_dialog(window: &gtk::Window) {
    window.connect_realize(|w| {
        let surface = w.native().and_then(|n| n.surface()).and_then(|s| s.downcast::<X11Surface>().ok());
        let (Some(surface), Ok(display)) = (surface, WidgetExt::display(w).downcast::<X11Display>()) else { return };
        let Ok(x) = xlib::Xlib::open() else { return };
        // SAFETY: GDK's own Xlib connection and window, used on the GTK thread
        unsafe {
            let dpy = display.xdisplay();
            let prop = (x.XInternAtom)(dpy, c"_NET_WM_WINDOW_TYPE".as_ptr(), 0);
            let dialog = (x.XInternAtom)(dpy, c"_NET_WM_WINDOW_TYPE_DIALOG".as_ptr(), 0);
            (x.XChangeProperty)(dpy, surface.xid(), prop, xlib::XA_ATOM, 32, xlib::PropModeReplace,
                                (&dialog as *const xlib::Atom).cast(), 1);
        }
    });
}
