//! Options → Scheduler, as on Windows and IDM's: start the queue at a time on the days ticked (or once, on
//! a date), stop it at another time, and sleep or shut down once it's done. Under the days, when
//! it next starts, kept up to date as the times and days change.

use super::settings::Line;
use gtk::prelude::*;
use std::rc::Rc;
use turbodm::power::Power;
use turbodm::schedule::{clock, format_time, parse_time, Schedule, DAYS};

pub struct SchedulerPage {
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
    gtk::Switch::builder().active(active).build()
}

fn field(text: &str) -> gtk::Entry {
    gtk::Entry::builder().text(text).width_chars(4).build()
}

impl SchedulerPage {
    pub fn new(plan: &Schedule) -> Rc<Self> {
        let when_done = super::frame::choice(&["Do nothing", "Sleep", "Shut down"], plan.when_done as u32);
        let page = Rc::new(SchedulerPage {
            start: switch(plan.start),
            start_at: field(&format_time(plan.start_at)),
            days: DAYS.iter().zip(plan.days).map(|(day, on)| gtk::CheckButton::builder().label(*day).active(on).build())
                .collect(),
            once: field(&plan.once),
            next: gtk::Label::builder().xalign(0.0).css_classes(["dim-label"]).build(),
            stop: switch(plan.stop),
            stop_at: field(&format_time(plan.stop_at)),
            when_done,
            was: plan.clone(),
        });
        page.once.set_placeholder_text(Some("2026-11-20"));
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

    /// The page's lines (settings.rs lays them out).
    pub fn lines(&self) -> Vec<Line> {
        use Line::{Heading, Name, Row, Setting};
        vec![Heading("Start downloads at a set time"),
             Setting("Start the queue at a set time", self.start.clone().upcast(), 44),
             Setting("At (24-hour clock, as 02:30)", self.start_at.clone().upcast(), 96),
             Name("On these days"),
             Row(self.days.iter().map(|d| d.clone().upcast()).collect()),
             Setting("Or only once, on (as 2026-11-20)", self.once.clone().upcast(), 150),
             Row(vec![self.next.clone().upcast()]),
             Heading("Stop, and when it's done"),
             Setting("Stop downloading at a set time", self.stop.clone().upcast(), 44),
             Setting("At (24-hour clock)", self.stop_at.clone().upcast(), 96),
             Setting("Then, once they're all done", self.when_done.clone().upcast(), 150)]
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
