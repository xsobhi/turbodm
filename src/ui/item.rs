//! One row of the downloads list: a GObject whose properties the columns bind to.

use gtk::glib;
use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use super::sidebar::state_of;
use turbodm::categories::icon_for;
use turbodm::engine::Snapshot;
use turbodm::text::status_text;
use turbodm::util::{human_eta, human_size, human_speed};

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::DownloadItem)]
    pub struct DownloadItem {
        #[property(get, set)]
        pub id: RefCell<String>,
        #[property(get, set)]
        pub name: RefCell<String>,
        #[property(get, set)]
        pub icon: RefCell<String>,
        #[property(get, set)]
        pub size: RefCell<String>,
        #[property(get, set)]
        pub status: RefCell<String>,
        #[property(get, set)]
        pub progress: Cell<f64>,
        #[property(get, set)]
        pub speed: RefCell<String>,
        #[property(get, set)]
        pub eta: RefCell<String>,
        #[property(get, set)]
        pub added: RefCell<String>,
        // for the sidebar filter and column sorting
        #[property(get, set)]
        pub category: RefCell<String>,
        #[property(get, set)]
        pub state: RefCell<String>,
        #[property(get, set)]
        pub bytes: Cell<u64>,
        #[property(get, set)]
        pub added_at: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DownloadItem {
        const NAME: &'static str = "TdmDownloadItem";
        type Type = super::DownloadItem;
    }

    #[glib::derived_properties]
    impl ObjectImpl for DownloadItem {}
}

glib::wrapper! {
    pub struct DownloadItem(ObjectSubclass<imp::DownloadItem>);
}

fn format_time(secs: u64) -> String {
    glib::DateTime::from_unix_local(secs as i64)
        .and_then(|d| d.format("%b %e, %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

impl DownloadItem {
    pub fn new(snap: &Snapshot) -> Self {
        let item: Self = glib::Object::builder().property("id", &snap.id)
            .property("added", format_time(snap.added)).property("added-at", snap.added).build();
        item.update(snap);
        item
    }

    /// Copy a snapshot in, touching only properties that changed (cheap redraws). True when
    /// it moved to another sidebar view (another state or category).
    pub fn update(&self, snap: &Snapshot) -> bool {
        let set = |name: &str, value: String| {
            if self.property::<String>(name) != value {
                self.set_property(name, value);
            }
        };
        set("name", snap.filename.clone());
        set("icon", icon_for(&snap.filename).into());
        set("size", human_size(snap.size));
        set("status", status_text(snap));
        set("speed", human_speed(snap.speed));
        set("eta", human_eta(snap.eta));
        let progress = snap.progress.unwrap_or(0.0).clamp(0.0, 1.0);
        if (self.progress() - progress).abs() > 1e-4 {
            self.set_progress(progress);
        }
        if self.bytes() != snap.size.unwrap_or(0) {
            self.set_bytes(snap.size.unwrap_or(0));
        }
        let moved = self.category() != snap.category || self.state() != state_of(snap.status);
        if moved {
            self.set_category(snap.category);
            self.set_state(state_of(snap.status));
        }
        moved
    }
}
