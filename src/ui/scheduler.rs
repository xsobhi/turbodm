//! Preferences → Scheduler, as IDM's: start the queue at a time on the days ticked (or once, on
//! a date), stop it at another time, and sleep or shut down once it's done. Under the days, when
//! it next starts, kept up to date as the times and days change.

use gtk::prelude::*;
use std::rc::Rc;
use turbodm::power::Power;
use turbodm::schedule::{clock, format_time, parse_time, Schedule, DAYS};

pub struct SchedulerPage {
    pub body: gtk::Grid,
    start: gtk::Switch,
    start_at: gtk::Entry,
    days: Vec<gtk::CheckButton>,
    once: gtk::Entry,
    next: gtk::Label,
    stop: gtk::Switch,
    stop_at: gtk::Entry,
    when_done: gtk::DropDown,
    was: Schedule,
}

fn switch(active: bool) -> gtk::Switch {
    gtk::Switch::builder().active(active).halign(gtk::Align::Start).build()
}

fn field(text: &str, chars: i32) -> gtk::Entry {
    gtk::Entry::builder().text(text).width_chars(chars).halign(gtk::Align::Start).build()
}

fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder().label(text).xalign(0.0).css_classes(["heading"]).build()
}

impl SchedulerPage {
    pub fn new(plan: &Schedule) -> Rc<Self> {
        let when_done = gtk::DropDown::from_strings(&["Do nothing", "Sleep", "Shut down"]);
        when_done.set_selected(plan.when_done as u32);
        let page = Rc::new(SchedulerPage {
            body: gtk::Grid::builder().row_spacing(10).column_spacing(16)
                .margin_top(18).margin_bottom(18).margin_start(18).margin_end(18).build(),
            start: switch(plan.start),
            start_at: field(&format_time(plan.start_at), 6),
            days: DAYS.iter().zip(plan.days).map(|(day, on)| gtk::CheckButton::builder().label(*day).active(on).build())
                .collect(),
            once: field(&plan.once, 12),
            next: gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).build(),
            stop: switch(plan.stop),
            stop_at: field(&format_time(plan.stop_at), 6),
            when_done,
            was: plan.clone(),
        });
        page.once.set_placeholder_text(Some("2026-11-20"));
        page.lay_out();
        // each change says the next start again
        let weak = Rc::downgrade(&page);
        let again = move || if let Some(page) = weak.upgrade() { page.say_next() };
        let a = again.clone();
        page.start.connect_active_notify(move |_| a());
        for entry in [&page.start_at, &page.once] {
            let a = again.clone();
            entry.connect_changed(move |_| a());
        }
        for day in &page.days {
            let a = again.clone();
            day.connect_toggled(move |_| a());
        }
        page.say_next();
        page
    }

    fn lay_out(&self) {
        let days = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        self.days.iter().for_each(|d| days.append(d));
        let rows: [(&str, Option<&gtk::Widget>); 10] = [
            ("Start downloads at a set time", None),
            ("Start the queue at a set time", Some(self.start.upcast_ref())),
            ("At (24-hour clock, as 02:30)", Some(self.start_at.upcast_ref())),
            ("On these days", Some(days.upcast_ref())),
            ("Or only once, on (year-month-day)", Some(self.once.upcast_ref())),
            ("", Some(self.next.upcast_ref())),
            ("Stop, and when it's done", None),
            ("Stop downloading at a set time", Some(self.stop.upcast_ref())),
            ("At (24-hour clock)", Some(self.stop_at.upcast_ref())),
            ("Then, once they're all done", Some(self.when_done.upcast_ref())),
        ];
        for (y, (name, widget)) in rows.into_iter().enumerate() {
            match widget {
                None => self.body.attach(&heading(name), 0, y as i32, 2, 1),
                Some(widget) => {
                    self.body.attach(&gtk::Label::builder().label(name).xalign(0.0).hexpand(true).build(), 0, y as i32, 1, 1);
                    self.body.attach(widget, 1, y as i32, 1, 1);
                }
            }
        }
    }

    /// The schedule as set on the page (a time that doesn't read as one stays as it was).
    pub fn chosen(&self) -> Schedule {
        Schedule {
            start: self.start.is_active(),
            start_at: parse_time(&self.start_at.text()).unwrap_or(self.was.start_at),
            days: std::array::from_fn(|i| self.days[i].is_active()),
            once: self.once.text().trim().to_string(),
            stop: self.stop.is_active(),
            stop_at: parse_time(&self.stop_at.text()).unwrap_or(self.was.stop_at),
            when_done: Power::from_index(self.when_done.selected()),
        }
    }

    fn say_next(&self) {
        self.next.set_text(&self.chosen().describe(clock::now()));
    }
}
