//! The menu bar's menus, as on Windows (and IDM's): Tasks, File, Downloads, View, Help; and the
//! downloads list's right-click menu. What each does is in actions.rs.

use gtk::gio;
use gtk::prelude::*;

/// The speed limiter's choices: (KiB/s, its name).
pub const SPEEDS: [(i64, &str); 7] = [(0, "_No limit"), (256, "256 KB/s"), (512, "512 KB/s"), (1024, "1 MB/s"),
    (2048, "2 MB/s"), (5120, "5 MB/s"), (10240, "10 MB/s")];

/// "Arrange files": (the list's column, its name).
pub const SORTS: [(i32, &str); 6] = [(0, "By file _name"), (1, "By _size"), (2, "By s_tatus"),
    (3, "By transfer _rate"), (4, "By time _left"), (5, "By _date added")];

/// A line: (label, action, the shortcut shown).
fn item(label: &str, action: &str, accel: Option<&str>) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(action));
    if let Some(accel) = accel {
        item.set_attribute_value("accel", Some(&accel.to_variant()));
    }
    item
}

/// Lines in groups, a line between each.
fn menu(groups: &[&[gio::MenuItem]]) -> gio::Menu {
    let menu = gio::Menu::new();
    for group in groups {
        let section = gio::Menu::new();
        for item in group.iter() {
            section.append_item(item);
        }
        menu.append_section(None, &section);
    }
    menu
}

fn submenu(label: &str, menu: &gio::Menu) -> gio::MenuItem {
    gio::MenuItem::new_submenu(Some(label), menu)
}

/// The menu bar.
pub fn bar() -> gio::Menu {
    let tasks = menu(&[
        &[item("_Add new download...", "win.add", Some("<Control>n")),
          item("Download _video...", "win.add-video", None),
          item("Add _torrent file...", "win.add-torrent", None)],
        &[item("Add _batch download...", "win.batch-add", None),
          item("Add batch download from _clipboard", "win.clipboard-links", None)],
        &[submenu("_Import", &menu(&[&[item("From _text file (one address a line)...", "win.import", None)]])),
          submenu("_Export", &menu(&[&[item("To _text file...", "win.export", None)]]))],
        &[item("E_xit", "app.quit", Some("<Control>q"))],
    ]);
    let file = menu(&[
        &[item("_Open", "win.open", None), item("Open _with...", "win.open-with", None),
          item("Open f_older", "win.open-folder", None),
          item("Download comple_te window...", "win.complete-window", None)],
        &[item("Re_download...", "win.redownload", None),
          item("Refresh download _address...", "win.refresh", None),
          item("_Copy address", "win.copy-url", None)],
        &[item("_Properties (progress details)", "win.details", Some("<Control>i"))],
        &[item("D_elete...", "win.remove", Some("Delete")), item("Delete with _file...", "win.delete-file", None)],
    ]);
    let speeds = gio::Menu::new();
    for (kib, name) in SPEEDS {
        let item = gio::MenuItem::new(Some(name), None);
        item.set_action_and_target_value(Some("win.speed-limit"), Some(&kib.to_variant()));
        speeds.append_item(&item);
    }
    let downloads = menu(&[
        &[item("_Resume", "win.resume", Some("<Control>r")), item("_Pause", "win.pause", Some("<Control>p")),
          item("Pause _or resume", "win.pause-resume", Some("space"))],
        &[item("Resume a_ll", "win.resume-all", Some("F5")), item("Pa_use all", "win.pause-all", None),
          item("Delete all _completed (from the list)", "win.delete-completed", None)],
        &[item("_Find...", "win.find", Some("<Control>f")), submenu("_Speed limiter", &speeds)],
        &[item("Open download f_older", "win.open-download-folder", None),
          item("_Options...", "win.settings", Some("<Control>comma"))],
    ]);
    let sorts = gio::Menu::new();
    for (column, name) in SORTS {
        let item = gio::MenuItem::new(Some(name), None);
        item.set_action_and_target_value(Some("win.sort"), Some(&column.to_variant()));
        sorts.append_item(&item);
    }
    let view = menu(&[
        &[item("_Categories", "win.show-categories", None), item("_Toolbar", "win.show-toolbar", None),
          item("Toolbar te_xt", "win.toolbar-text", None)],
        &[submenu("_Arrange files", &sorts)],
        &[item("Show download _progress window", "win.show-progress", None),
          item("Show download comp_lete window", "win.show-complete", None)],
    ]);
    let help = menu(&[
        &[item("TurboDM on _GitHub", "win.github", None), item("_Report a problem...", "win.report", None)],
        &[item("Add to your _browser...", "win.browsers", None),
          item("_YouTube in your browser...", "win.youtube", None),
          item("Check for _updates...", "win.check-updates", None)],
        &[item("_About TurboDM", "win.about", None)],
    ]);
    let bar = gio::Menu::new();
    for (name, menu) in [("_Tasks", tasks), ("_File", file), ("_Downloads", downloads), ("_View", view), ("_Help", help)] {
        bar.append_submenu(Some(name), &menu);
    }
    bar
}

/// The downloads list's right-click menu.
pub fn context() -> gio::Menu {
    menu(&[
        &[item("_Open", "win.open", None), item("Open _with...", "win.open-with", None),
          item("Open f_older", "win.open-folder", None),
          item("Download comple_te window...", "win.complete-window", None),
          item("Progress _details", "win.details", None)],
        &[item("_Resume", "win.resume", None), item("_Pause", "win.pause", None),
          item("Re_download...", "win.redownload", None),
          item("Refresh download _address...", "win.refresh", None), item("_Copy address", "win.copy-url", None)],
        &[item("De_lete...", "win.remove", None), item("Delete with _file...", "win.delete-file", None)],
    ])
}
