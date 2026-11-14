//! The scheduler's arithmetic: the next start on chosen days, past midnight, once on a date,
//! and when a started queue stops.

use super::clock::{civil_from_days, days_from_civil, weekday, Minute, DAY};
use super::*;

/// A moment: a date and a time on the wall clock.
fn at(year: i32, month: u32, day: u32, hour: i64, minute: i64) -> Minute {
    days_from_civil(year, month, day) * DAY + hour * 60 + minute
}

fn plan(start_at: &str, days: &str) -> Schedule {
    let mut s = Schedule { start: true, start_at: parse_time(start_at).unwrap(), ..Schedule::default() };
    s.days = std::array::from_fn(|i| days.contains(DAYS[i]));
    s
}

#[test]
fn dates_and_days_of_the_week() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(civil_from_days(days_from_civil(2028, 2, 29)), (2028, 2, 29));
    assert_eq!(weekday(0), 3); // a Thursday
    assert_eq!(weekday(days_from_civil(2026, 11, 14)), 5); // a Saturday
    assert_eq!(weekday(days_from_civil(2026, 11, 16)), 0); // a Monday
}

#[test]
fn later_today_or_on_the_next_chosen_day() {
    let s = plan("02:30", "Mon Tue Wed Thu Fri Sat Sun");
    let saturday_1am = at(2026, 11, 14, 1, 0);
    assert_eq!(s.next_start(saturday_1am), Some(at(2026, 11, 14, 2, 30)));
    // at the start itself, or after: tomorrow's
    assert_eq!(s.next_start(at(2026, 11, 14, 2, 30)), Some(at(2026, 11, 15, 2, 30)));
    // weekdays only: Saturday's and Sunday's are skipped
    let weekdays = plan("02:30", "Mon Tue Wed Thu Fri");
    assert_eq!(weekdays.next_start(saturday_1am), Some(at(2026, 11, 16, 2, 30)));
    // one day a week, and that day's start gone by: a week later
    let saturdays = plan("02:30", "Sat");
    assert_eq!(saturdays.next_start(at(2026, 11, 14, 3, 0)), Some(at(2026, 11, 21, 2, 30)));
}

#[test]
fn past_midnight() {
    // late in the evening, a start in the small hours is tomorrow's, on tomorrow's weekday
    let s = plan("00:15", "Sun");
    assert_eq!(s.next_start(at(2026, 11, 14, 23, 50)), Some(at(2026, 11, 15, 0, 15)));
    // across the end of a month and of a year
    let every_day = plan("01:00", "Mon Tue Wed Thu Fri Sat Sun");
    assert_eq!(every_day.next_start(at(2026, 12, 31, 22, 0)), Some(at(2027, 1, 1, 1, 0)));
    // a stop earlier in the day than the start is the next morning's
    let mut night = plan("23:00", "Sat");
    (night.stop, night.stop_at) = (true, parse_time("06:00").unwrap());
    let started = at(2026, 11, 14, 23, 0);
    assert_eq!(night.stop_after(started), Some(at(2026, 11, 15, 6, 0)));
    night.stop_at = parse_time("23:45").unwrap();
    assert_eq!(night.stop_after(started), Some(at(2026, 11, 14, 23, 45)));
    night.stop = false;
    assert_eq!(night.stop_after(started), None);
}

#[test]
fn once_on_a_date() {
    let mut s = plan("09:00", "");
    s.once = "2026-11-20".into();
    assert_eq!(s.next_start(at(2026, 11, 14, 12, 0)), Some(at(2026, 11, 20, 9, 0)));
    assert_eq!(s.next_start(at(2026, 11, 20, 8, 59)), Some(at(2026, 11, 20, 9, 0)));
    assert_eq!(s.next_start(at(2026, 11, 20, 9, 0)), None); // gone by: never again
    s.once = "2026-02-30".into();
    assert_eq!(s.next_start(at(2026, 1, 1, 0, 0)), None); // no such day
}

#[test]
fn never_when_off_or_no_day_is_ticked() {
    let mut s = plan("02:30", "");
    assert_eq!(s.next_start(at(2026, 11, 14, 1, 0)), None);
    s = plan("02:30", "Mon");
    s.start = false;
    assert_eq!(s.next_start(at(2026, 11, 14, 1, 0)), None);
    assert_eq!(s.describe(at(2026, 11, 14, 1, 0)), "Not scheduled.");
}

#[test]
fn typed_times_and_dates() {
    assert_eq!(parse_time("2:30"), Some(150));
    assert_eq!(parse_time(" 14.05 "), Some(14 * 60 + 5));
    assert_eq!(parse_time("7"), Some(7 * 60));
    assert_eq!(parse_time("24:00"), None);
    assert_eq!(parse_time("12:60"), None);
    assert_eq!(parse_time("noon"), None);
    assert_eq!(format_time(150), "02:30");
    assert_eq!(parse_date("2026-11-20"), Some((2026, 11, 20)));
    assert_eq!(parse_date("2026-13-01"), None);
    assert_eq!(parse_date("2026-11"), None);
    assert_eq!(format_date(at(2026, 3, 5, 10, 0)), "2026-03-05");
}

#[test]
fn says_when_it_next_starts() {
    let s = plan("02:30", "Mon");
    assert_eq!(s.describe(at(2026, 11, 14, 1, 0)), "Next start: Mon 16 Nov at 02:30");
    assert_eq!(s.describe(at(2026, 12, 29, 1, 0)), "Next start: Mon 4 Jan 2027 at 02:30");
}
