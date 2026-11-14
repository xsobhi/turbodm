//! Options → Scheduler, as IDM's: start the queue at a time on the days ticked (or once, on a
//! date), stop it at another time, and sleep or shut down once it's done. Under the days, when it
//! next starts, kept up to date as the times and days change.

use super::settings::Line::{self, Heading, Name, Row, Setting};
use super::wnd::{is_checked, set_text, text, Window};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use turbodm::power::Power;
use turbodm::schedule::{clock, format_time, parse_time, Schedule, DAYS};
use windows::Win32::Foundation::HWND;

/// What's done when the scheduled downloads are, in the order `Power::from_index` counts.
const WHEN_DONE: [&str; 3] = ["Do nothing", "Sleep", "Shut down"];

pub struct SchedulerPage {
    start: HWND,
    start_at: HWND,
    days: [HWND; 7],
    once: HWND,
    next: HWND,
    stop: HWND,
    stop_at: HWND,
    when_done: (HWND, Rc<Cell<usize>>),
    was: Schedule,
}

impl SchedulerPage {
    pub fn new(window: &Window, plan: &Schedule) -> Rc<Self> {
        // (each change says the next start again, once the page is there to read)
        let changed: Rc<RefCell<Option<Rc<SchedulerPage>>>> = Rc::default();
        let again = {
            let c = changed.clone();
            move || if let Some(page) = c.borrow().as_ref() { page.say_next() }
        };
        let switch = |on: bool| { let a = again.clone(); window.switch(on, move |_| a()) };
        let field = |value: &str| window.edit(value, again.clone());
        let days = std::array::from_fn(|i| { let a = again.clone(); window.checkbox(DAYS[i], plan.days[i], move |_| a()) });
        let picked = Rc::new(Cell::new(plan.when_done as usize));
        let p = picked.clone();
        let page = Rc::new(SchedulerPage {
            start: switch(plan.start),
            start_at: field(&format_time(plan.start_at)),
            days,
            once: field(&plan.once),
            next: window.label(""),
            stop: switch(plan.stop),
            stop_at: field(&format_time(plan.stop_at)),
            when_done: (window.choice(&WHEN_DONE, picked.get(), move |i| p.set(i)), picked),
            was: plan.clone(),
        });
        page.say_next();
        *changed.borrow_mut() = Some(page.clone());
        page
    }

    /// The page's lines, for settings.rs to lay out.
    pub fn lines(&self) -> Vec<Line> {
        vec![Heading("Start downloads at a set time"),
             Setting("Start the queue at a set time", self.start, 44),
             Setting("At (24-hour clock, as 02:30)", self.start_at, 96),
             Name("On these days"),
             Row(self.days.to_vec()),
             Setting("Or only once, on (as 2026-11-20)", self.once, 150),
             Row(vec![self.next]),
             Heading("Stop, and when it's done"),
             Setting("Stop downloading at a set time", self.stop, 44),
             Setting("At (24-hour clock)", self.stop_at, 96),
             Setting("Then, once they're all done", self.when_done.0, 150)]
    }

    /// The schedule as set on the page (a time that doesn't read as one stays as it was).
    pub fn chosen(&self) -> Schedule {
        Schedule {
            start: is_checked(self.start),
            start_at: parse_time(&text(self.start_at)).unwrap_or(self.was.start_at),
            days: self.days.map(is_checked),
            once: text(self.once).trim().to_string(),
            stop: is_checked(self.stop),
            stop_at: parse_time(&text(self.stop_at)).unwrap_or(self.was.stop_at),
            when_done: Power::from_index(self.when_done.1.get() as u32),
        }
    }

    fn say_next(&self) {
        set_text(self.next, &self.chosen().describe(clock::now()));
    }
}
