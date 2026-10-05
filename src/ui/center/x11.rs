//! X11: windows are moved with Xlib once mapped. They're first announced as dialogs, which
//! window managers centre themselves (no jump from a corner); normal windows turn back into
//! normal windows once placed, so they can be minimized.

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::prelude::*;

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

/// The window manager's frame around the window: (left, right, top, bottom), X pixels.
fn frame(x: &xlib::Xlib, dpy: *mut xlib::Display, xid: xlib::Window) -> [i32; 4] {
    let mut extents = [0; 4];
    // SAFETY: GDK's own Xlib connection; the reply is checked and freed
    unsafe {
        let prop = (x.XInternAtom)(dpy, c"_NET_FRAME_EXTENTS".as_ptr(), 0);
        let (mut kind, mut format, mut count, mut after) = (0, 0, 0, 0);
        let mut data: *mut u8 = std::ptr::null_mut();
        let ok = (x.XGetWindowProperty)(dpy, xid, prop, 0, 4, 0, xlib::XA_CARDINAL, &mut kind, &mut format,
                                        &mut count, &mut after, &mut data) == 0;
        if ok && !data.is_null() && format == 32 && count == 4 {
            let values = std::slice::from_raw_parts(data as *const std::ffi::c_ulong, 4);
            for (extent, value) in extents.iter_mut().zip(values) {
                *extent = *value as i32;
            }
        }
        if !data.is_null() {
            (x.XFree)(data.cast());
        }
    }
    extents
}

/// Move the (mapped) window, frame included, to the middle of its monitor, unless it was
/// already centred at this size. Returns the outer size it was centred for.
fn move_to_center(w: &gtk::Window, placed: (i32, i32)) -> (i32, i32) {
    let mut size = placed;
    with_x11(w, |x, dpy, xid, surface| {
        let Some(monitor) = WidgetExt::display(w).monitor_at_surface(surface) else { return };
        let (area, scale) = (monitor.geometry(), surface.scale_factor());
        if surface.width() <= 1 || surface.height() <= 1 {
            return; // not sized yet
        }
        let [left, right, top, bottom] = frame(x, dpy, xid);
        let outer = (surface.width() * scale + left + right, surface.height() * scale + top + bottom);
        if outer == placed {
            return;
        }
        size = outer;
        // the requested position is the frame's top-left corner
        let x_pos = area.x() * scale + (area.width() * scale - outer.0).max(0) / 2;
        let y_pos = area.y() * scale + (area.height() * scale - outer.1).max(0) / 2;
        // SAFETY: as above
        unsafe {
            (x.XMoveWindow)(dpy, xid, x_pos, y_pos);
            (x.XFlush)(dpy);
        }
    });
    size
}

pub fn realized(window: &gtk::Window) {
    set_type(window, c"_NET_WM_WINDOW_TYPE_DIALOG");
}

/// Placed: windows that aren't dialogs of another window can be minimized again.
pub fn placed(window: &gtk::Window) {
    if window.transient_for().is_none() && !window.is_modal() {
        set_type(window, c"_NET_WM_WINDOW_TYPE_NORMAL");
    }
}

pub fn center(window: &gtk::Window, placed: (i32, i32)) -> (i32, i32) {
    move_to_center(window, placed)
}

/// Window managers resize the frame to the new default size by themselves.
pub fn set_height(_window: &gtk::Window, _height: i32) {}
