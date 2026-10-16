//! The sidebar, as on Linux Mint (and like IDM's category tree): downloads by state and by kind
//! of file, with counts; the free space where downloads go at the bottom. In a short window (or
//! on a screen scaled up) the rows stop above the free space and scroll: by the mouse wheel, or
//! by Mint's slim scroll bar at the right (dragged, or clicked above or below its thumb).

use super::super::theme::pictures::{self as pic, Picture};
use super::super::theme::{self, Align, Font, Painter};
use super::super::wnd::canvas::{self, Mouse};
use super::super::wnd::{px, Window};
mod scroll;

use scroll::{content, overflow, rows_bottom, scroll_bar, thumb};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use turbodm::engine::{Snapshot, Status};
use turbodm::util::human_size;
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// What the downloads list shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    All,
    Active,     // downloading or waiting in the queue
    Unfinished, // everything not complete
    Completed,
    Category(&'static str),
}

impl View {
    pub fn matches(self, s: &Snapshot) -> bool {
        match self {
            View::All => true,
            View::Active => s.status.is_active() || s.status == Status::Queued,
            View::Unfinished => s.status != Status::Completed,
            View::Completed => s.status == Status::Completed,
            View::Category(c) => s.category == c,
        }
    }
}

enum Row {
    Heading(&'static str),
    Entry(View, &'static str, Picture),
}

/// Categories come from `turbodm::categories`; "General" is the rest.
const ROWS: [Row; 13] = [
    Row::Heading("Downloads"),
    Row::Entry(View::All, "All downloads", pic::ALL),
    Row::Entry(View::Active, "Downloading", pic::DOWNLOADING),
    Row::Entry(View::Unfinished, "Unfinished", pic::UNFINISHED),
    Row::Entry(View::Completed, "Completed", pic::COMPLETED),
    Row::Heading("Categories"),
    Row::Entry(View::Category("Video"), "Video", pic::VIDEO),
    Row::Entry(View::Category("Music"), "Music", pic::MUSIC),
    Row::Entry(View::Category("Documents"), "Documents", pic::DOCUMENTS),
    Row::Entry(View::Category("Compressed"), "Compressed", pic::COMPRESSED),
    Row::Entry(View::Category("Programs"), "Programs", pic::PROGRAMS),
    Row::Entry(View::Category("Images"), "Images", pic::IMAGES),
    Row::Entry(View::Category("General"), "Other", pic::OTHER),
];

const HEADING: i32 = 32; // heights, 96-DPI pixels
const ENTRY: i32 = 38;
const FOOTER: i32 = 58;

#[derive(Default)]
struct State {
    hover: Option<usize>,
    counts: Vec<usize>,
    disk: Option<(u64, u64)>, // free, total
    checked: Option<Instant>,
    scroll: i32, // how far the rows are scrolled up (canvas pixels)
    drag: Option<(i32, i32)>, // the thumb held: where the mouse and the scroll were then
    bar_hot: bool, // the mouse over the scroll bar
}

pub struct Sidebar {
    pub hwnd: HWND,
    pub view: Rc<Cell<View>>, // the one picked
    state: Rc<RefCell<State>>,
}

/// Each row's index in ROWS, top and bottom (canvas pixels), scrolled up by `scroll`.
fn rows(scroll: i32) -> impl Iterator<Item = (usize, i32, i32)> {
    let mut y = px(4) - scroll;
    ROWS.iter().enumerate().map(move |(i, row)| {
        let height = px(if matches!(row, Row::Heading(_)) { HEADING } else { ENTRY });
        y += height;
        (i, y - height, y)
    })
}

impl Sidebar {
    /// Picking a view sets `view` and runs `on_change`.
    pub fn new(window: &Window, on_change: impl Fn() + 'static) -> Self {
        let view = Rc::new(Cell::new(View::All));
        let state: Rc<RefCell<State>> = Rc::default();
        let (s, v) = (state.clone(), view.clone());
        let hwnd = window.canvas(move |p| paint(p, &s.borrow(), v.get()));
        let (s, v) = (state.clone(), view.clone());
        let s2 = state.clone();
        canvas::on_wheel(hwnd, move |delta| {
            let mut state = s2.borrow_mut();
            let scroll = (state.scroll - delta * px(ENTRY) / 120).clamp(0, overflow(hwnd, &state));
            if scroll != state.scroll {
                state.scroll = scroll;
                canvas::redraw(hwnd);
            }
        });
        canvas::on_mouse(hwnd, move |event| {
            if scroll_bar(hwnd, &mut s.borrow_mut(), &event) {
                return; // (the scroll bar's)
            }
            let (scroll, rows_end) = (s.borrow().scroll.min(overflow(hwnd, &s.borrow())), rows_bottom(hwnd, &s.borrow()));
            let entry = |y: i32| rows(scroll).find(|&(i, top, bottom)| y >= top && y < bottom && y < rows_end
                                                    && matches!(ROWS[i], Row::Entry(..)));
            let hover = match event {
                Mouse::Move(_, y) | Mouse::Down(_, y) | Mouse::Up(_, y) | Mouse::Double(_, y) => entry(y).map(|e| e.0),
                Mouse::Leave => None,
            };
            if s.borrow().hover != hover {
                s.borrow_mut().hover = hover;
                canvas::redraw(hwnd);
            }
            if let (Mouse::Down(..), Some(Row::Entry(picked, ..))) = (event, hover.map(|i| &ROWS[i]))
                && v.get() != *picked {
                v.set(*picked);
                canvas::redraw(hwnd);
                on_change();
            }
        });
        Sidebar { hwnd, view, state }
    }

    /// New counts, and the free space where downloads are saved.
    pub fn update(&self, snapshots: &[Snapshot], download_dir: &Path) {
        let counts: Vec<usize> = ROWS.iter().map(|row| match row {
            Row::Entry(view, ..) => snapshots.iter().filter(|s| view.matches(s)).count(),
            Row::Heading(_) => 0,
        }).collect();
        let mut state = self.state.borrow_mut();
        let due = state.checked.is_none_or(|t| t.elapsed() > Duration::from_secs(2));
        let disk = if due { free_space(download_dir) } else { state.disk };
        if due {
            state.checked = Some(Instant::now());
        }
        if state.counts != counts || state.disk != disk {
            (state.counts, state.disk) = (counts, disk);
            canvas::redraw(self.hwnd);
        }
    }
}

/// (free, total) bytes on the disk of `dir` (or of the nearest folder above it that exists).
fn free_space(dir: &Path) -> Option<(u64, u64)> {
    let dir = dir.ancestors().find(|d| d.exists())?;
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: plain query into our two numbers
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut free), Some(&mut total), None) }.ok()?;
    Some((free, total))
}

fn paint(p: &Painter, state: &State, view: View) {
    p.fill(p.rect, theme::VIEW);
    let right = p.rect.right;
    // (scrolled no further than there is to see: the window may have grown since)
    let footer = if state.disk.is_some() { px(FOOTER) } else { 0 };
    let room = p.rect.bottom - footer;
    let scroll = state.scroll.min((content() - room).max(0));
    for (i, top, bottom) in rows(scroll) {
        match ROWS[i] {
            Row::Heading(text) => {
                let r = RECT { left: px(14), top: top + px(10), right: right - px(12), bottom };
                p.text(r, text, Font::CaptionStrong, theme::DIM, Align::Left);
            }
            Row::Entry(entry, name, picture) => {
                // the chosen one a full-width row in Mint's green, as GTK draws Mint's sidebars
                let r = RECT { left: 0, top, right, bottom };
                let picked = entry == view;
                if picked {
                    p.fill(r, theme::ACCENT);
                } else if state.hover == Some(i) {
                    p.fill(r, theme::hover(theme::VIEW));
                }
                let (color, dim) = if picked { (theme::WHITE, theme::WHITE) } else { (theme::TEXT, theme::DIM) };
                p.picture(RECT { left: px(10), right: px(32), ..r }, picture, 20, false);
                p.text(RECT { left: px(40), right: right - px(48), ..r }, name, Font::Body, color, Align::Left);
                if let Some(n) = state.counts.get(i).filter(|&&n| n > 0) {
                    p.text(RECT { left: right - px(52), right: right - px(14), ..r }, &n.to_string(), Font::Body, dim, Align::Right);
                }
            }
        }
    }
    if let Some(t) = thumb(right, room, scroll, state.bar_hot) {
        let color = if state.bar_hot { theme::DIM } else { theme::mix(theme::VIEW, theme::DIM, 0.45) };
        p.round(t, (t.right - t.left) / 2, Some(color), None);
    }
    let Some((free, total)) = state.disk else { return };
    let top = p.rect.bottom - px(FOOTER);
    p.fill(RECT { left: 0, top, right, bottom: p.rect.bottom }, theme::VIEW); // (over rows that go on below)
    p.fill(RECT { left: 0, top, right, bottom: top + 1 }, theme::BORDER);
    let line = format!("{} free of {}", human_size(Some(free)), human_size(Some(total)));
    p.text(RECT { left: px(14), top: top + px(10), right: right - px(12), bottom: top + px(30) }, &line,
           Font::Caption, theme::DIM, Align::Left);
    let bar = RECT { left: px(14), top: top + px(38), right: right - px(14), bottom: top + px(42) };
    let used = 1.0 - free as f64 / total.max(1) as f64;
    p.round(bar, px(2), Some(theme::TRACK), None);
    let fill = RECT { right: bar.left + ((bar.right - bar.left) as f64 * used) as i32, ..bar };
    p.round(fill, px(2), Some(if used > 0.9 { theme::DANGER } else { theme::ACCENT }), None);
}
