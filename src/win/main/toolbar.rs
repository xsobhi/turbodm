//! The toolbar, as on Linux (and like IDM's): 3D icons with their names under them, in groups.

use super::super::theme::pictures::{self as pic, Picture};
use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::canvas::{self, Mouse};
use super::super::wnd::{px, Window};
use super::menu::Cmd;
use std::cell::RefCell;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, RECT};

/// (icon, command, name, tooltip); `None` separates groups.
const BUTTONS: [Option<(Picture, Cmd, &str, &str)>; 12] = [
    Some((pic::ADD, Cmd::Add, "Add URL", "Add a download (Ctrl+N)")),
    None,
    Some((pic::RESUME, Cmd::Resume, "Resume", "Resume selected (Ctrl+R)")),
    Some((pic::PAUSE, Cmd::Pause, "Pause", "Pause selected (Ctrl+P)")),
    Some((pic::RESUME_ALL, Cmd::ResumeAll, "Resume all", "Resume every unfinished download (F5)")),
    Some((pic::PAUSE_ALL, Cmd::PauseAll, "Pause all", "Pause every download")),
    None,
    Some((pic::DELETE, Cmd::Remove, "Delete", "Delete selected: off the list, and their files if you say (Delete)")),
    Some((pic::CLEAR, Cmd::ClearDone, "Clear done", "Remove completed downloads from the list")),
    None,
    Some((pic::FOLDER, Cmd::DownloadFolder, "Folder", "Open the download folder")),
    Some((pic::OPTIONS, Cmd::Settings, "Options", "Preferences (Ctrl+,)")),
];

/// Height of the toolbar; a button's width; the room a separator takes (96-DPI pixels).
pub const HEIGHT: i32 = 60;
const WIDTH: i32 = 76;
const SEPARATOR: i32 = 11;

#[derive(Default)]
struct State {
    hover: Option<usize>,
    down: Option<usize>,
    disabled: Vec<Cmd>,
}

pub struct Toolbar {
    pub hwnd: HWND,
    state: Rc<RefCell<State>>,
}

/// Where each button is (canvas pixels), by its index in BUTTONS; and separators' middles.
fn areas() -> (Vec<(usize, RECT)>, Vec<i32>) {
    let (mut buttons, mut separators, mut x) = (Vec::new(), Vec::new(), px(6));
    for (i, button) in BUTTONS.iter().enumerate() {
        if button.is_some() {
            buttons.push((i, RECT { left: x, top: px(4), right: x + px(WIDTH), bottom: px(HEIGHT - 4) }));
            x += px(WIDTH + 2);
        } else {
            separators.push(x + px(SEPARATOR) / 2 - 1);
            x += px(SEPARATOR);
        }
    }
    (buttons, separators)
}

impl Toolbar {
    pub fn new(window: &Window) -> Self {
        let state: Rc<RefCell<State>> = Rc::default();
        let s = state.clone();
        let hwnd = window.canvas(move |p| paint(p, &s.borrow()));
        let s = state.clone();
        canvas::on_mouse(hwnd, move |event| {
            let at = |x: i32, y: i32| areas().0.into_iter()
                .find(|(_, r)| x >= r.left && x < r.right && y >= r.top && y < r.bottom).map(|(i, _)| i);
            let mut state = s.borrow_mut();
            let before = (state.hover, state.down);
            let mut clicked = None;
            match event {
                Mouse::Move(x, y) => state.hover = at(x, y),
                Mouse::Leave => state.hover = None,
                Mouse::Down(x, y) | Mouse::Double(x, y) => state.down = at(x, y),
                Mouse::Up(x, y) => {
                    clicked = state.down.take().filter(|&i| at(x, y) == Some(i));
                }
            }
            if (state.hover, state.down) != before {
                canvas::redraw(hwnd);
            }
            let cmd = clicked.and_then(|i| BUTTONS[i]).map(|(_, cmd, ..)| cmd).filter(|cmd| !state.disabled.contains(cmd));
            drop(state);
            if let Some(cmd) = cmd {
                super::super::post(move |app| super::menu::run(app, cmd));
            }
        });
        let tips: Vec<(RECT, String)> = areas().0.into_iter()
            .filter_map(|(i, r)| BUTTONS[i].map(|(_, _, _, tip)| (r, tip.to_string()))).collect();
        canvas::set_tips(hwnd, &tips);
        Toolbar { hwnd, state }
    }

    pub fn enable(&self, cmd: Cmd, on: bool) {
        let mut state = self.state.borrow_mut();
        if state.disabled.contains(&cmd) == on {
            state.disabled.retain(|c| *c != cmd);
            if !on {
                state.disabled.push(cmd);
            }
            canvas::redraw(self.hwnd);
        }
    }
}

fn paint(p: &Painter, state: &State) {
    p.fill(p.rect, theme::CHROME);
    let (buttons, separators) = areas();
    for x in separators {
        p.fill(RECT { left: x, top: px(16), right: x + 1, bottom: px(HEIGHT - 16) }, theme::BORDER);
    }
    for (i, r) in buttons {
        let Some((picture, cmd, name, _)) = BUTTONS[i] else { continue };
        let enabled = !state.disabled.contains(&cmd);
        if enabled && state.hover == Some(i) {
            let pressed = state.down == Some(i);
            let fill = if pressed { theme::pressed(theme::CHROME) } else { theme::hover(theme::CHROME) };
            p.round(r, px(theme::RADIUS), Some(fill), None);
        }
        let color = if enabled { theme::TEXT } else { theme::DISABLED };
        p.picture(RECT { top: r.top + px(4), bottom: r.top + px(32), ..r }, picture, 26, !enabled);
        p.text(RECT { top: r.top + px(30), bottom: r.bottom - px(4), ..r }, name, Font::Caption, color, Align::Center);
    }
}
