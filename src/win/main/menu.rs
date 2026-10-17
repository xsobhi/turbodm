//! The main window's commands: the menu bar's menus (as IDM's), the right-click menu, keyboard
//! shortcuts, and what each does.

use super::super::wnd::popup::{self, At};
use super::super::{add, batch, dialogs, progress, refresh, settings, shell, App};
use super::menubar;
use std::path::Path;
use std::rc::Rc;
use turbodm::engine::Status;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_DELETE, VK_F5};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum Cmd {
    Add = 1000, Resume, Pause, ResumeAll, PauseAll, Remove, DeleteFile, ClearDone, Open, OpenWith,
    OpenFolder, Details, CopyUrl, Refresh, DownloadFolder, Settings, Browsers, CheckUpdates, About,
    Exit, Show, SelectAll, Find,
    BatchAdd, ClipboardLinks, Import, Export, Redownload,
    Speed0, Speed256, Speed512, Speed1M, Speed2M, Speed5M, Speed10M,
    ToggleCategories, ToggleToolbar, ToggleProgress, ToggleComplete,
    SortName, SortSize, SortStatus, SortSpeed, SortLeft, SortAdded,
    GitHub, Report, MenuTasks, MenuFile, MenuDownloads, MenuView, MenuHelp,
    AddTorrent, AddVideo, CompleteWindow, YouTube,
}

pub const ALL: &[Cmd] = &[Cmd::Add, Cmd::Resume, Cmd::Pause, Cmd::ResumeAll, Cmd::PauseAll, Cmd::Remove,
    Cmd::DeleteFile, Cmd::ClearDone, Cmd::Open, Cmd::OpenWith, Cmd::OpenFolder, Cmd::Details, Cmd::CopyUrl,
    Cmd::Refresh, Cmd::DownloadFolder, Cmd::Settings, Cmd::Browsers, Cmd::CheckUpdates, Cmd::About,
    Cmd::Exit, Cmd::Show, Cmd::SelectAll, Cmd::Find,
    Cmd::BatchAdd, Cmd::ClipboardLinks, Cmd::Import, Cmd::Export, Cmd::Redownload,
    Cmd::Speed0, Cmd::Speed256, Cmd::Speed512, Cmd::Speed1M, Cmd::Speed2M, Cmd::Speed5M, Cmd::Speed10M,
    Cmd::ToggleCategories, Cmd::ToggleToolbar, Cmd::ToggleProgress, Cmd::ToggleComplete,
    Cmd::SortName, Cmd::SortSize, Cmd::SortStatus, Cmd::SortSpeed, Cmd::SortLeft, Cmd::SortAdded,
    Cmd::GitHub, Cmd::Report, Cmd::MenuTasks, Cmd::MenuFile, Cmd::MenuDownloads, Cmd::MenuView, Cmd::MenuHelp,
    Cmd::AddTorrent, Cmd::AddVideo, Cmd::CompleteWindow, Cmd::YouTube];

/// The speed limiter's choices: (command, KiB/s, its name).
const SPEEDS: [(Cmd, u64, &str); 7] = [(Cmd::Speed0, 0, "&No limit"), (Cmd::Speed256, 256, "256 KB/s"),
    (Cmd::Speed512, 512, "512 KB/s"), (Cmd::Speed1M, 1024, "1 MB/s"), (Cmd::Speed2M, 2048, "2 MB/s"),
    (Cmd::Speed5M, 5120, "5 MB/s"), (Cmd::Speed10M, 10240, "10 MB/s")];

/// "Arrange files": (command, the list's column, its name, ascending at first).
const SORTS: [(Cmd, usize, &str, bool); 6] = [(Cmd::SortName, 0, "By file &name", true),
    (Cmd::SortSize, 1, "By &size", false), (Cmd::SortStatus, 2, "By s&tatus", true),
    (Cmd::SortSpeed, 3, "By transfer &rate", false), (Cmd::SortLeft, 4, "By time &left", true),
    (Cmd::SortAdded, 5, "By &date added", false)];

const GITHUB: &str = "https://github.com/xsobhi/turbodm";

/// A menu's line.
enum Item {
    Do(Cmd, &'static str, bool),    // a command (false: greyed)
    Tick(Cmd, &'static str, bool),  // with a tick when it's on
    Sub(&'static str, Vec<Item>),
    Line,
}

use Item::{Do, Line, Sub, Tick};

/// The menus' lines, as TurboDM's menus have them (wnd/popup).
fn build(items: &[Item]) -> Vec<popup::Item> {
    items.iter().map(|item| match item {
        Do(cmd, text, on) => popup::Item::Do(*cmd as u16, text.to_string(), *on),
        Tick(cmd, text, on) => popup::Item::Tick(*cmd as u16, text.to_string(), *on),
        Sub(text, items) => popup::Item::Sub(text.to_string(), build(items)),
        Line => popup::Item::Line,
    }).collect()
}

/// The menu bar's menu `index` (as `menubar::MENUS`), for what's selected now.
pub fn bar_menu(app: &Rc<App>, index: usize) -> Vec<popup::Item> {
    let s = app.manager.settings();
    let selected: Vec<_> = app.main.list.selected().iter().filter_map(|id| app.manager.get(id)).collect();
    let any = !selected.is_empty();
    let done = selected.first().is_some_and(|s| s.status == Status::Completed);
    let torrent = selected.first().is_some_and(|s| s.torrent.is_some()); // (no address to refresh)
    let items = match index {
        0 => vec![Do(Cmd::Add, "&Add new download...\tCtrl+N", true),
                  Do(Cmd::AddVideo, "Download &video...", true),
                  Do(Cmd::AddTorrent, "Add &torrent file...", true), Line,
                  Do(Cmd::BatchAdd, "Add &batch download...", true),
                  Do(Cmd::ClipboardLinks, "Add batch download from &clipboard", true), Line,
                  Sub("&Import", vec![Do(Cmd::Import, "From &text file (one address a line)...", true)]),
                  Sub("&Export", vec![Do(Cmd::Export, "To &text file...", true)]), Line,
                  Do(Cmd::Exit, "E&xit\tCtrl+Q", true)],
        1 => vec![Do(Cmd::Open, "&Open", done), Do(Cmd::OpenWith, "Open &with...", done),
                  Do(Cmd::OpenFolder, "Open f&older", any),
                  Do(Cmd::CompleteWindow, "Download comple&te window...", done), Line,
                  Do(Cmd::Redownload, "Re&download...", any),
                  Do(Cmd::Refresh, "Refresh download &address...", any && !torrent),
                  Do(Cmd::CopyUrl, "&Copy address", any), Line,
                  Do(Cmd::Details, "&Properties (progress details)\tCtrl+I", any), Line,
                  Do(Cmd::Remove, "D&elete...\tDel", any), Do(Cmd::DeleteFile, "Delete with &file...", any)],
        2 => vec![Do(Cmd::Resume, "&Resume\tCtrl+R", any), Do(Cmd::Pause, "&Pause\tCtrl+P", any), Line,
                  Do(Cmd::ResumeAll, "Resume a&ll\tF5", true), Do(Cmd::PauseAll, "Pa&use all", true),
                  Do(Cmd::ClearDone, "Delete all &completed (from the list)", true), Line,
                  Do(Cmd::Find, "&Find...\tCtrl+F", true),
                  Sub("&Speed limiter", SPEEDS.iter().map(|&(cmd, kib, name)| Tick(cmd, name, s.speed_limit_kib == kib)).collect()),
                  Line, Do(Cmd::DownloadFolder, "Open download f&older", true),
                  Do(Cmd::Settings, "&Options...\tCtrl+,", true)],
        3 => {
            let (column, _) = app.main.list.sorting();
            vec![Tick(Cmd::ToggleCategories, "&Categories", s.show_categories),
                 Tick(Cmd::ToggleToolbar, "&Toolbar", s.show_toolbar), Line,
                 Sub("&Arrange files", SORTS.iter().map(|&(cmd, c, name, _)| Tick(cmd, name, column == c)).collect()),
                 Line, Tick(Cmd::ToggleProgress, "Show download &progress window", s.show_progress_window),
                 Tick(Cmd::ToggleComplete, "Show download comp&lete window", s.show_complete_dialog)]
        }
        _ => vec![Do(Cmd::GitHub, "TurboDM on &GitHub", true), Do(Cmd::Report, "&Report a problem...", true), Line,
                  Do(Cmd::Browsers, "Add to your &browser...", true), Do(Cmd::YouTube, "&YouTube in your browser...", true),
                  Do(Cmd::CheckUpdates, "Check for &updates...", true),
                  Line, Do(Cmd::About, "&About TurboDM", true)],
    };
    build(&items)
}

/// The downloads list's right-click menu, at `point` (screen pixels).
pub fn context(owner: HWND, point: POINT) {
    let items = [Do(Cmd::Open, "&Open", true), Do(Cmd::OpenWith, "Open &with...", true),
        Do(Cmd::OpenFolder, "Open f&older", true), Do(Cmd::CompleteWindow, "Download comple&te window...", true),
        Do(Cmd::Details, "Progress &details", true), Line,
        Do(Cmd::Resume, "&Resume", true), Do(Cmd::Pause, "&Pause", true), Do(Cmd::Redownload, "Re&download...", true),
        Do(Cmd::Refresh, "Refresh download &address...", true), Do(Cmd::CopyUrl, "&Copy address", true), Line,
        Do(Cmd::Remove, "De&lete...", true), Do(Cmd::DeleteFile, "Delete with &file...", true)];
    popup::command(owner, &build(&items), At::Point(point)); // (its choice comes as WM_COMMAND)
}

/// Keyboard shortcuts of the main window (Alt and a menu's letter opens it).
pub fn shortcuts() -> HACCEL {
    let ctrl = FVIRTKEY | FCONTROL;
    let alt = FVIRTKEY | FALT;
    let key = |flags: ACCEL_VIRT_FLAGS, key: u16, cmd: Cmd| ACCEL { fVirt: flags, key, cmd: cmd as u16 };
    let table = [key(ctrl, b'N' as u16, Cmd::Add), key(ctrl, b'R' as u16, Cmd::Resume), key(ctrl, b'P' as u16, Cmd::Pause),
        key(ctrl, b'I' as u16, Cmd::Details), key(ctrl, b'Q' as u16, Cmd::Exit), key(ctrl, b'A' as u16, Cmd::SelectAll),
        key(ctrl, 0xBC, Cmd::Settings), // Ctrl+, (VK_OEM_COMMA)
        key(ctrl, b'F' as u16, Cmd::Find),
        key(FVIRTKEY, VK_DELETE.0, Cmd::Remove), key(FVIRTKEY, VK_F5.0, Cmd::ResumeAll),
        key(alt, b'T' as u16, Cmd::MenuTasks), key(alt, b'F' as u16, Cmd::MenuFile), key(alt, b'D' as u16, Cmd::MenuDownloads),
        key(alt, b'V' as u16, Cmd::MenuView), key(alt, b'H' as u16, Cmd::MenuHelp)];
    // SAFETY: a table built from our array
    unsafe { CreateAcceleratorTableW(&table).unwrap_or_default() }
}

/// Do `cmd` (from the menus, the toolbar, a shortcut or the tray).
pub fn run(app: &Rc<App>, cmd: Cmd) {
    let selected = || app.main.list.selected();
    let first = || selected().into_iter().next();
    let settings = |change: &dyn Fn(&mut turbodm::config::Settings)| {
        let mut s = app.manager.settings();
        change(&mut s);
        app.manager.update_settings(s);
    };
    match cmd {
        Cmd::Add => add::add_url(app),
        Cmd::AddVideo => add::add_video(app, String::new()), // (a link on the clipboard is filled in)
        Cmd::AddTorrent => add::torrent::add_torrent_file(app),
        Cmd::Resume => selected().iter().for_each(|id| {
            app.manager.resume(id);
            if app.manager.settings().show_progress_window {
                progress::open(app, id);
            }
        }),
        Cmd::Pause => selected().iter().for_each(|id| app.manager.pause(id)),
        Cmd::ResumeAll => app.manager.resume_all(),
        Cmd::PauseAll => app.manager.pause_all(),
        Cmd::Remove => dialogs::confirm_delete(app, selected(), false), // (asks, with "Also delete the files")
        Cmd::DeleteFile => dialogs::confirm_delete(app, selected(), true),
        Cmd::ClearDone => app.manager.snapshot().iter().filter(|s| s.status == Status::Completed)
            .for_each(|s| app.manager.remove(&s.id, false)), // from the list only: the files stay
        Cmd::Open => if let Some(id) = first() { shell::open_download(app, &id) },
        // (its window again, for one closed by mistake)
        Cmd::CompleteWindow => if let Some(s) = first().and_then(|id| app.manager.get(&id))
            .filter(|s| s.status == Status::Completed) { super::super::complete::open(app, &s) },
        Cmd::OpenWith => if let Some(s) = first().and_then(|id| app.manager.get(&id)) { shell::open_with(&s.path) },
        Cmd::OpenFolder => if let Some(id) = first() { shell::show_download(app, &id) },
        Cmd::Details => if let Some(id) = first() { progress::open(app, &id) },
        Cmd::CopyUrl => if let Some(s) = first().and_then(|id| app.manager.get(&id)) { shell::copy_text(&s.url) },
        Cmd::Refresh => if let Some(s) = first().and_then(|id| app.manager.get(&id)).filter(|s| s.torrent.is_none()) {
            refresh::start(app, &s.id)
        },
        Cmd::Redownload => dialogs::confirm_redownload(app, selected()),
        Cmd::DownloadFolder => {
            let dir = app.manager.settings().download_dir;
            let _ = std::fs::create_dir_all(&dir);
            shell::open(&dir);
        }
        Cmd::Settings => settings::open(app),
        Cmd::Browsers => super::super::browsers::open(app),
        Cmd::YouTube => super::super::youtube::open(app),
        Cmd::CheckUpdates => super::super::update::check(app, true),
        Cmd::About => dialogs::about(app),
        Cmd::Exit => app.quit(),
        Cmd::Show => app.show(),
        Cmd::SelectAll => app.main.list.select_all(),
        Cmd::Find => {
            // SAFETY: plain focus change in our window
            unsafe { let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(app.main.search)); }
            super::super::wnd::send(app.main.search, 0x00B1, 0, -1); // EM_SETSEL: all
        }
        Cmd::BatchAdd => batch::pattern(app),
        Cmd::ClipboardLinks => batch::from_clipboard(app),
        Cmd::Import => batch::import(app),
        Cmd::Export => batch::export(app),
        Cmd::Speed0 | Cmd::Speed256 | Cmd::Speed512 | Cmd::Speed1M | Cmd::Speed2M | Cmd::Speed5M | Cmd::Speed10M => {
            if let Some(&(_, kib, _)) = SPEEDS.iter().find(|(c, _, _)| *c == cmd) {
                settings(&|s| s.speed_limit_kib = kib);
            }
        }
        Cmd::ToggleCategories | Cmd::ToggleToolbar => {
            settings(&|s| if cmd == Cmd::ToggleCategories { s.show_categories ^= true } else { s.show_toolbar ^= true });
            let s = app.manager.settings();
            app.main.show_parts(s.show_categories, s.show_toolbar);
        }
        Cmd::ToggleProgress => settings(&|s| s.show_progress_window ^= true),
        Cmd::ToggleComplete => settings(&|s| s.show_complete_dialog ^= true),
        Cmd::SortName | Cmd::SortSize | Cmd::SortStatus | Cmd::SortSpeed | Cmd::SortLeft | Cmd::SortAdded => {
            if let Some(&(_, column, _, ascending)) = SORTS.iter().find(|(c, ..)| *c == cmd) {
                // (the same again turns it around, as clicking a column's header does)
                let (now, up) = app.main.list.sorting();
                app.main.list.sort_by(column, if now == column { !up } else { ascending });
                super::refresh(app);
            }
        }
        Cmd::GitHub => shell::open(Path::new(GITHUB)),
        Cmd::Report => shell::open(Path::new(&format!("{GITHUB}/issues/new"))),
        Cmd::MenuTasks => menubar::open(app, 0, true),
        Cmd::MenuFile => menubar::open(app, 1, true),
        Cmd::MenuDownloads => menubar::open(app, 2, true),
        Cmd::MenuView => menubar::open(app, 3, true),
        Cmd::MenuHelp => menubar::open(app, 4, true),
    }
}
