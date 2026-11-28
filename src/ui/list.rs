//! The downloads table (ColumnView) and keeping it in sync with the engine.

use super::item::DownloadItem;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::collections::HashMap;
use std::rc::Rc;
use turbodm::engine::Snapshot;

/// Called on right-click with (row position, widget clicked, x, y).
pub type MenuHandler = Rc<dyn Fn(u32, &gtk::Widget, f64, f64)>;

/// The table over `store`, through `filter` (sidebar and search), sortable by clicking headers:
/// newest first at the start; a new column goes up for names, down for the rest (biggest,
/// fastest, newest first), as on Windows.
pub fn build(store: &gio::ListStore, filter: &gtk::CustomFilter, on_menu: MenuHandler)
             -> (gtk::ColumnView, gtk::MultiSelection) {
    let view = gtk::ColumnView::new(None::<gtk::MultiSelection>);
    let filtered = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
    let sorted = gtk::SortListModel::new(Some(filtered), view.sorter());
    let selection = gtk::MultiSelection::new(Some(sorted));
    view.set_model(Some(&selection));
    view.set_show_row_separators(false); // (drawn by the stylesheet, as on Windows)
    view.set_reorderable(false);
    view.append_column(&name_column(&selection, &on_menu));
    view.append_column(&text_column("Size", "size", 90, by(|i| i.bytes()), &selection, &on_menu));
    view.append_column(&status_column(by(|i| (i.progress() * 1e6) as u64), &selection, &on_menu));
    view.append_column(&text_column("Speed", "speed", 100, by(|i| i.rate()), &selection, &on_menu));
    view.append_column(&text_column("Time left", "eta", 80, by(|i| i.left()), &selection, &on_menu));
    view.append_column(&text_column("Added", "added", 130, by(|i| i.added_at()), &selection, &on_menu));
    sort_by(&view, 5, false);
    // a column clicked for the first time: names A to Z, the rest biggest first
    let last = Rc::new(std::cell::Cell::new(5usize));
    let busy = Rc::new(std::cell::Cell::new(false));
    if let Some(sorter) = view.sorter().and_downcast::<gtk::ColumnViewSorter>() {
        let v = view.downgrade();
        sorter.connect_changed(move |sorter, _| {
            let (Some(view), false) = (v.upgrade(), busy.get()) else { return };
            let Some(column) = sorter.primary_sort_column() else { return };
            let index = (0..view.columns().n_items()).find(|&i| view.columns().item(i).as_ref() == Some(column.upcast_ref()))
                .unwrap_or(0) as usize;
            if index != last.replace(index) && index != 0 && sorter.primary_sort_order() == gtk::SortType::Ascending {
                busy.set(true);
                sort_by(&view, index, false);
                busy.set(false);
            }
        });
    }
    (view, selection)
}

/// Sort by column `index`, up or down.
pub fn sort_by(view: &gtk::ColumnView, index: usize, ascending: bool) {
    let column = view.columns().item(index as u32).and_downcast::<gtk::ColumnViewColumn>();
    let order = if ascending { gtk::SortType::Ascending } else { gtk::SortType::Descending };
    view.sort_by_column(column.as_ref(), order);
}

/// The column it's sorted by, and whether going up.
pub fn sorting(view: &gtk::ColumnView) -> (usize, bool) {
    let Some(sorter) = view.sorter().and_downcast::<gtk::ColumnViewSorter>() else { return (5, false) };
    let index = sorter.primary_sort_column().and_then(|c| {
        (0..view.columns().n_items()).find(|&i| view.columns().item(i).as_ref() == Some(c.upcast_ref()))
    }).unwrap_or(5) as usize;
    (index, sorter.primary_sort_order() == gtk::SortType::Ascending)
}

/// Every row selected.
pub fn select_all(selection: &gtk::MultiSelection) {
    selection.select_all();
}

/// Sort rows by a key.
fn by<T: Ord>(key: impl Fn(&DownloadItem) -> T + 'static) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| {
        let (Some(a), Some(b)) = (a.downcast_ref::<DownloadItem>(), b.downcast_ref::<DownloadItem>())
            else { return gtk::Ordering::Equal };
        key(a).cmp(&key(b)).into()
    })
}

fn bind(list_item: &gtk::ListItem, prop: &str, widget: &impl IsA<gtk::Widget>, target: &str) {
    list_item
        .property_expression("item")
        .chain_property::<DownloadItem>(prop)
        .bind(widget.upcast_ref::<gtk::Widget>(), target, gtk::Widget::NONE);
}

/// Right-click on any cell selects that row and opens the context menu.
fn attach_menu(widget: &impl IsA<gtk::Widget>, list_item: &gtk::ListItem,
               selection: &gtk::MultiSelection, on_menu: &MenuHandler) {
    let gesture = gtk::GestureClick::builder().button(3).build();
    let (item, selection, on_menu) = (list_item.downgrade(), selection.clone(), on_menu.clone());
    let target = widget.clone().upcast::<gtk::Widget>();
    gesture.connect_pressed(move |_, _, x, y| {
        let Some(item) = item.upgrade() else { return };
        let pos = item.position();
        if !selection.is_selected(pos) {
            selection.select_item(pos, true);
        }
        on_menu(pos, &target, x, y);
    });
    widget.add_controller(gesture);
}

fn column(title: &str, factory: gtk::SignalListItemFactory, width: i32,
          sorter: Option<gtk::CustomSorter>) -> gtk::ColumnViewColumn {
    let col = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    col.set_sorter(sorter.as_ref());
    col.set_resizable(true);
    col.set_fixed_width(width);
    col
}

fn text_column(title: &str, prop: &'static str, width: i32, sorter: gtk::CustomSorter,
               selection: &gtk::MultiSelection, on_menu: &MenuHandler) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let (selection, on_menu) = (selection.clone(), on_menu.clone());
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
        bind(list_item, prop, &label, "label");
        attach_menu(&label, list_item, &selection, &on_menu);
        list_item.set_child(Some(&label));
    });
    column(title, factory, width, Some(sorter))
}

fn name_column(selection: &gtk::MultiSelection, on_menu: &MenuHandler) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let (selection, on_menu) = (selection.clone(), on_menu.clone());
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let row = gtk::Box::builder().spacing(8).build();
        let label = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        row.append(&file_icon(list_item));
        row.append(&label);
        bind(list_item, "name", &label, "label");
        bind(list_item, "name", &row, "tooltip-text");
        attach_menu(&row, list_item, &selection, &on_menu);
        list_item.set_child(Some(&row));
    });
    let col = column("File name", factory, 320, Some(by(|i| i.name().to_lowercase())));
    col.set_expand(true); // (the rest of the width)
    col
}

/// The file type's icon, small, as the desktop shows that type (Windows: the shell's).
fn file_icon(list_item: &gtk::ListItem) -> gtk::Image {
    let icon = gtk::Image::builder().pixel_size(16).build();
    bind(list_item, "icon", &icon, "icon-name");
    icon
}

fn status_column(sorter: gtk::CustomSorter, selection: &gtk::MultiSelection, on_menu: &MenuHandler) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let (selection, on_menu) = (selection.clone(), on_menu.clone());
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let bar = gtk::ProgressBar::builder().show_text(true).valign(gtk::Align::Center).margin_end(4).build();
        bar.set_ellipsize(gtk::pango::EllipsizeMode::End);
        bind(list_item, "progress", &bar, "fraction");
        // a finished download whose file is gone: its bar faded
        let b = bar.clone();
        list_item.property_expression("item").chain_property::<DownloadItem>("missing")
            .chain_closure::<Vec<String>>(glib::closure!(|_: Option<glib::Object>, missing: bool| {
                if missing { vec!["missing".to_string()] } else { Vec::new() }
            }))
            .bind(&b, "css-classes", gtk::Widget::NONE);
        bind(list_item, "status", &bar, "text");
        bind(list_item, "status", &bar, "tooltip-text");
        attach_menu(&bar, list_item, &selection, &on_menu);
        list_item.set_child(Some(&bar));
    });
    column("Status", factory, 170, Some(sorter))
}

/// Update rows in place, append new downloads, drop removed ones. True when a row moved to
/// another sidebar view (the filter needs another look).
pub fn sync(store: &gio::ListStore, snapshots: &[Snapshot]) -> bool {
    let mut moved = false;
    let mut index: HashMap<String, u32> = HashMap::new();
    for pos in 0..store.n_items() {
        if let Some(item) = store.item(pos).and_downcast::<DownloadItem>() {
            index.insert(item.id(), pos);
        }
    }
    let mut seen = std::collections::HashSet::new();
    for snap in snapshots {
        seen.insert(snap.id.as_str());
        match index.get(&snap.id).and_then(|&p| store.item(p)).and_downcast::<DownloadItem>() {
            Some(item) => moved |= item.update(snap),
            None => store.append(&DownloadItem::new(snap)),
        }
    }
    for pos in (0..store.n_items()).rev() {
        let item = store.item(pos).and_downcast::<DownloadItem>();
        if item.is_some_and(|i| !seen.contains(i.id().as_str())) {
            store.remove(pos);
        }
    }
    moved
}

/// Update just these downloads' rows: (all had a row, one moved to another sidebar view).
pub fn update(store: &gio::ListStore, snapshots: &[Snapshot]) -> (bool, bool) {
    if snapshots.is_empty() {
        return (true, false);
    }
    let (mut found, mut moved) = (0, false);
    for pos in 0..store.n_items() {
        let Some(item) = store.item(pos).and_downcast::<DownloadItem>() else { continue };
        if let Some(snap) = snapshots.iter().find(|s| s.id == item.id()) {
            moved |= item.update(snap);
            found += 1;
        }
    }
    (found == snapshots.len(), moved)
}

/// Ids of the selected rows.
pub fn selected_ids(selection: &gtk::MultiSelection) -> Vec<String> {
    let set = selection.selection();
    (0..set.size() as u32)
        .filter_map(|i| selection.item(set.nth(i)).and_downcast::<DownloadItem>())
        .map(|item| item.id())
        .collect()
}
