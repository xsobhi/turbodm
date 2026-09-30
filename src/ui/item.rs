//! One row of the downloads list: a GObject whose properties the columns bind to.

use gtk::glib;
use gtk::glib::subclass::prelude::*;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use turbodm::categories::icon_for;
use turbodm::engine::{Snapshot, Status};
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

pub fn status_text(snap: &Snapshot) -> String {
    let pct = snap.progress.map(|p| format!("{:.1}%", p * 100.0));
    match snap.status {
        Status::Downloading => pct.unwrap_or_else(|| human_size(Some(snap.downloaded))),
        Status::Paused => pct.map_or("Paused".into(), |p| format!("Paused · {p}")),
        Status::Error => format!("Error: {}", snap.error.as_deref().unwrap_or("unknown")),
        other => other.label().into(),
    }
}

fn format_time(secs: u64) -> String {
    glib::DateTime::from_unix_local(secs as i64)
        .and_then(|d| d.format("%b %e, %H:%M"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

impl DownloadItem {
    pub fn new(snap: &Snapshot) -> Self {
        let item: Self = glib::Object::builder().property("id", &snap.id).build();
        item.update(snap);
        item
    }

    /// Copy a snapshot in, touching only properties that changed (cheap redraws).
    pub fn update(&self, snap: &Snapshot) {
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
        set("added", format_time(snap.added));
        let progress = snap.progress.unwrap_or(0.0).clamp(0.0, 1.0);
        if (self.progress() - progress).abs() > 1e-4 {
            self.set_progress(progress);
        }
    }
}
