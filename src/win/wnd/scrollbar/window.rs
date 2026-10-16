//! The slim bar's window: its painting, and the mouse on it (bar.rs says what each does).

use super::bar::BARS;
use crate::win::theme::Painter;
use crate::win::wnd::{instance, send};
use std::sync::Once;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

const REPEAT: usize = 1; // the timer paging again while the track is held

pub const CLASS: PCWSTR = w!("TurboDMScrollBar");

pub fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        // SAFETY: plain class registration with a static name and procedure
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&class);
    });
}

extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(bar) = BARS.with(|b| b.borrow().get(&(hwnd.0 as isize)).cloned()) else {
        // SAFETY: the default handling
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    // SAFETY: standard handling of our own window's messages
    unsafe {
        match msg {
            WM_ERASEBKGND => return LRESULT(1),
            WM_PAINT => Painter::paint(hwnd, |p| bar.paint(p)),
            WM_MOUSEMOVE => {
                let along = bar.mouse_at(lparam);
                if !bar.hover.replace(true) {
                    let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                                      dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut track);
                    bar.redraw();
                }
                if let (Some(grab), Some((_, size))) = (bar.drag.get(), bar.thumb()) {
                    let (start, length) = bar.track();
                    bar.scroll_to(bar.range.get().at(along - start - grab, length, size));
                }
            }
            WM_MOUSELEAVE => {
                bar.hover.set(false);
                bar.redraw();
            }
            WM_LBUTTONDOWN => {
                SetCapture(hwnd);
                let along = bar.mouse_at(lparam);
                match bar.thumb() {
                    Some((at, size)) if (at..at + size).contains(&along) => bar.drag.set(Some(along - at)),
                    Some((at, _)) => {
                        bar.paging.set(if along < at { -1 } else { 1 });
                        bar.page();
                        SetTimer(Some(hwnd), REPEAT, 350, None);
                    }
                    None => {}
                }
                bar.redraw();
            }
            WM_LBUTTONUP | WM_CAPTURECHANGED => {
                if msg == WM_LBUTTONUP {
                    let _ = ReleaseCapture();
                }
                bar.drag.set(None);
                bar.paging.set(0);
                let _ = KillTimer(Some(hwnd), REPEAT);
                bar.redraw();
            }
            WM_TIMER if wparam.0 == REPEAT && bar.paging.get() != 0 => {
                bar.page();
                SetTimer(Some(hwnd), REPEAT, 50, None);
            }
            // the wheel over the bar scrolls the control, as over it
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => return LRESULT(send(bar.control, msg, wparam.0, lparam.0)),
            WM_NCDESTROY => {
                BARS.with(|b| b.borrow_mut().remove(&(hwnd.0 as isize)));
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
        LRESULT(0)
    }
}
