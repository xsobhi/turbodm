//! The downloads table (ColumnView) and keeping it in sync with the engine.

use super::item::DownloadItem;
use gtk::prelude::*;
use gtk::gio;
use std::collections::HashMap;
use std::rc::Rc;
use turbodm::engine::Snapshot;

/// Called on right-click with (row position, widget clicked, x, y).
pub type MenuHandler = Rc<dyn Fn(u32, &gtk::Widget, f64, f64)>;

pub fn build(store: &gio::ListStore, on_menu: MenuHandler) -> (gtk::ColumnView, gtk::MultiSelection) {
    let selection = gtk::MultiSelection::new(Some(store.clone()));
    let view = gtk::ColumnView::new(Some(selection.clone()));
    view.set_show_row_separators(true);
    view.set_reorderable(false);
    view.append_column(&name_column(&selection, &on_menu));
    view.append_column(&text_column("Size", "size", 90, &selection, &on_menu));
    view.append_column(&status_column(&selection, &on_menu));
    view.append_column(&text_column("Speed", "speed", 100, &selection, &on_menu));
    view.append_column(&text_column("Time left", "eta", 80, &selection, &on_menu));
    view.append_column(&text_column("Added", "added", 120, &selection, &on_menu));
    (view, selection)
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

fn column(title: &str, factory: gtk::SignalListItemFactory, width: i32) -> gtk::ColumnViewColumn {
    let col = gtk::ColumnViewColumn::new(Some(title), Some(factory));
    col.set_resizable(true);
    col.set_fixed_width(width);
    col
}

fn text_column(title: &str, prop: &'static str, width: i32,
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
    column(title, factory, width)
}

fn name_column(selection: &gtk::MultiSelection, on_menu: &MenuHandler) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let (selection, on_menu) = (selection.clone(), on_menu.clone());
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let icon = gtk::Image::new();
        let label = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::Middle).build();
        row.append(&icon);
        row.append(&label);
        bind(list_item, "icon", &icon, "icon-name");
        bind(list_item, "name", &label, "label");
        bind(list_item, "name", &row, "tooltip-text");
        attach_menu(&row, list_item, &selection, &on_menu);
        list_item.set_child(Some(&row));
    });
    let col = column("File name", factory, 320);
    col.set_expand(true);
    col
}

fn status_column(selection: &gtk::MultiSelection, on_menu: &MenuHandler) -> gtk::ColumnViewColumn {
    let factory = gtk::SignalListItemFactory::new();
    let (selection, on_menu) = (selection.clone(), on_menu.clone());
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<gtk::ListItem>().unwrap();
        let bar = gtk::ProgressBar::builder().show_text(true).valign(gtk::Align::Center).build();
        bar.set_ellipsize(gtk::pango::EllipsizeMode::End);
        bind(list_item, "progress", &bar, "fraction");
        bind(list_item, "status", &bar, "text");
        bind(list_item, "status", &bar, "tooltip-text");
        attach_menu(&bar, list_item, &selection, &on_menu);
        list_item.set_child(Some(&bar));
    });
    column("Status", factory, 170)
}

/// Update rows in place, append new downloads, drop removed ones.
pub fn sync(store: &gio::ListStore, snapshots: &[Snapshot]) {
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
            Some(item) => item.update(snap),
            None => store.append(&DownloadItem::new(snap)),
        }
    }
    for pos in (0..store.n_items()).rev() {
        let item = store.item(pos).and_downcast::<DownloadItem>();
        if item.is_some_and(|i| !seen.contains(i.id().as_str())) {
            store.remove(pos);
        }
    }
}

/// Ids of the selected rows.
pub fn selected_ids(selection: &gtk::MultiSelection) -> Vec<String> {
    let set = selection.selection();
    (0..set.size() as u32)
        .filter_map(|i| selection.item(set.nth(i)).and_downcast::<DownloadItem>())
        .map(|item| item.id())
        .collect()
}
