//! Placing and sizing windows on X11. Open windows in the middle of the screen. GTK 4 can't position windows, so on X11 the window
//! is moved there with Xlib once it's mapped. It's also announced as a dialog, which most window
//! managers centre themselves (Cinnamon only does so for dialogs with a parent). Dialogs can't
//! be minimized, so `minimizable` windows turn back into normal windows once placed. On Wayland
//! the compositor decides; this does nothing.

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::glib;
use gtk::prelude::*;
use std::time::Duration;

/// Run `f` with the window's Xlib display and X window id (on the GTK thread).
fn with_x11(w: &gtk::Window, f: impl FnOnce(&xlib::Xlib, *mut xlib::Display, xlib::Window, &X11Surface)) {
    let surface = w.native().and_then(|n| n.surface()).and_then(|s| s.downcast::<X11Surface>().ok());
    let (Some(surface), Ok(display)) = (surface, WidgetExt::display(w).downcast::<X11Display>()) else { return };
    let Ok(x) = xlib::Xlib::open() else { return };
    // SAFETY: GDK's own Xlib connection, valid while the display is open
    f(&x, unsafe { display.xdisplay() }, surface.xid(), &surface);
}

fn set_type(w: &gtk::Window, kind: &std::ffi::CStr) {
    with_x11(w, |x, dpy, xid, _| unsafe {
        // SAFETY: GDK's own Xlib connection and window, used on the GTK thread
        let prop = (x.XInternAtom)(dpy, c"_NET_WM_WINDOW_TYPE".as_ptr(), 0);
        let value = (x.XInternAtom)(dpy, kind.as_ptr(), 0);
        (x.XChangeProperty)(dpy, xid, prop, xlib::XA_ATOM, 32, xlib::PropModeReplace,
                            (&value as *const xlib::Atom).cast(), 1);
    });
}

/// Move the (mapped) window to the middle of its monitor, if its size isn't `placed` already.
/// Returns the size it was centred for.
fn move_to_center(w: &gtk::Window, placed: (i32, i32)) -> (i32, i32) {
    let mut size = placed;
    with_x11(w, |x, dpy, xid, surface| {
        let Some(monitor) = WidgetExt::display(w).monitor_at_surface(surface) else { return };
        let (area, scale) = (monitor.geometry(), surface.scale_factor());
        let (width, height) = (surface.width(), surface.height());
        if width <= 1 || height <= 1 || (width, height) == placed {
            return; // not sized yet, or already there
        }
        size = (width, height);
        let left = area.x() + (area.width() - width).max(0) / 2;
        let top = area.y() + (area.height() - height).max(0) / 2;
        // SAFETY: as above
        unsafe {
            (x.XMoveWindow)(dpy, xid, left * scale, top * scale);
            (x.XFlush)(dpy);
        }
    });
    size
}

/// Shrink or grow a shown window to the height its content needs at its current width. GTK 4
/// only sizes windows when they first appear; this is what a user's resize would do.
pub fn fit_height(window: &gtk::Window) {
    let content = window.measure(gtk::Orientation::Vertical, window.width()).1;
    with_x11(window, |x, dpy, xid, surface| {
        let margins = surface.height() - window.height(); // client-side shadows
        let scale = surface.scale_factor();
        // SAFETY: GDK's own Xlib connection and window, used on the GTK thread
        unsafe {
            (x.XResizeWindow)(dpy, xid, (surface.width() * scale) as u32, ((content + margins) * scale) as u32);
            (x.XFlush)(dpy);
        }
    });
}

pub fn on_screen(window: &gtk::Window, minimizable: bool) {
    window.connect_realize(|w| set_type(w, c"_NET_WM_WINDOW_TYPE_DIALOG"));
    window.connect_map(move |w| {
        // GTK sizes the window over the first frames after mapping it: follow that for a moment
        let (window, mut ticks, mut placed) = (w.clone(), 0, (0, 0));
        glib::timeout_add_local(Duration::from_millis(10), move || {
            ticks += 1;
            placed = move_to_center(&window, placed);
            if ticks < 30 { glib::ControlFlow::Continue } else { glib::ControlFlow::Break }
        });
        if minimizable {
            let w = w.clone();
            glib::timeout_add_local_once(Duration::from_millis(400), move || {
                set_type(&w, c"_NET_WM_WINDOW_TYPE_NORMAL"); // placed: now allow minimize
            });
        }
    });
}
