//! The scheduler, as IDM's: start the queue at a set time on chosen days of the week (or once,
//! on a date), stop it at another time if wanted, and sleep or shut down once it's done. This is
//! the plan and its arithmetic; the engine's timer (engine/manager/scheduler.rs) follows it.

pub mod clock;

use crate::power::Power;
use clock::{civil_from_days, days_from_civil, weekday, Minute, DAY};
use serde::{Deserialize, Serialize};

/// The days' names, Monday first (as `Schedule::days`).
pub const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Options → Scheduler. Times are minutes after midnight, on the computer's clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub start: bool,     // start the queue at `start_at`
    pub start_at: u16,
    pub days: [bool; 7], // on these days, Monday first
    pub once: String,    // or only once, on this date ("2026-11-20"); empty: by the days
    pub stop: bool,      // stop it at `stop_at` (the next day, if that comes before the start)
    pub stop_at: u16,
    pub when_done: Power, // once every download it started is done
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule { start: false, start_at: 2 * 60, days: [true; 7], once: String::new(), stop: false,
                   stop_at: 7 * 60, when_done: Power::Nothing }
    }
}

impl Schedule {
    /// The first start after `after` (not at it), or none: the start is off, no day is ticked,
    /// or its one date has gone by.
    pub fn next_start(&self, after: Minute) -> Option<Minute> {
        if !self.start {
            return None;
        }
        let at = Minute::from(self.start_at.min(DAY as u16 - 1));
        if !self.once.trim().is_empty() {
            let (year, month, day) = parse_date(&self.once)?;
            let start = days_from_civil(year, month, day) * DAY + at;
            return (start > after).then_some(start);
        }
        let today = after.div_euclid(DAY);
        // today's start if it's still to come, else the next ticked day's, within a week
        (today..=today + 7).map(|day| day * DAY + at)
            .find(|&start| start > after && self.days[weekday(start.div_euclid(DAY))])
    }

    /// When a queue started at `started` stops: the first `stop_at` after it (past midnight if
    /// that's earlier in the day), or never.
    pub fn stop_after(&self, started: Minute) -> Option<Minute> {
        if !self.stop {
            return None;
        }
        let stop = started.div_euclid(DAY) * DAY + Minute::from(self.stop_at.min(DAY as u16 - 1));
        Some(if stop > started { stop } else { stop + DAY })
    }

    /// The times kept within a day.
    pub fn clamp(mut self) -> Self {
        (self.start_at, self.stop_at) = (self.start_at.min(DAY as u16 - 1), self.stop_at.min(DAY as u16 - 1));
        self
    }

    /// When it next starts, for the options: "Next start: Mon 17 Nov at 02:30".
    pub fn describe(&self, now: Minute) -> String {
        match self.next_start(now) {
            None if self.start => "It won't start: tick a day, or set a date still to come.".to_string(),
            None => "Not scheduled.".to_string(),
            Some(at) => {
                let days = at.div_euclid(DAY);
                let (year, month, day) = civil_from_days(days);
                let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                let this_year = civil_from_days(now.div_euclid(DAY)).0 == year;
                let year = if this_year { String::new() } else { format!(" {year}") };
                format!("Next start: {} {day} {}{year} at {}", DAYS[weekday(days)], months[month as usize - 1],
                        format_time(at.rem_euclid(DAY) as u16))
            }
        }
    }
}

/// "02:30" for 150 minutes after midnight.
pub fn format_time(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
}

/// Minutes after midnight of a typed time: "2:30", "02:30", "14.05", "7" (on the hour).
pub fn parse_time(text: &str) -> Option<u16> {
    let text = text.trim();
    let (hours, minutes) = text.split_once([':', '.']).unwrap_or((text, "0"));
    let (hours, minutes): (u16, u16) = (hours.trim().parse().ok()?, minutes.trim().parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// A typed date, "2026-11-20" (year, month, day), if there's such a day.
pub fn parse_date(text: &str) -> Option<(i32, u32, u32)> {
    let mut parts = text.trim().splitn(3, '-').map(|p| p.trim().parse::<u32>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    let date = (i32::try_from(year).ok()?, month, day);
    // (the 31st of a 30-day month comes back as another day)
    ((1..=12).contains(&month) && civil_from_days(days_from_civil(date.0, month, day)) == date).then_some(date)
}

/// A date as it's typed: "2026-11-20".
pub fn format_date(minute: Minute) -> String {
    let (year, month, day) = civil_from_days(minute.div_euclid(DAY));
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests;
