//! The local clock, as the scheduler counts it: minutes since 1970-01-01 00:00 on the wall clock
//! (not UTC), so "02:30 on Mondays" is plain arithmetic, and a clock put forward or back for
//! summer time is just a jump the scheduler notices.

/// A moment on the local wall clock, in minutes since 1970-01-01 00:00.
pub type Minute = i64;

pub const DAY: Minute = 24 * 60;

/// Days since 1970-01-01 of a date (Howard Hinnant's algorithm; any year, month 1-12).
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (i64::from(month) + 9) % 12; // March is 0
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01, as (year, month, day).
pub fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(month <= 2)) as i32, month, day)
}

/// The day of the week of a day since 1970-01-01: 0 is Monday, 6 Sunday (1970-01-01 was a Thursday).
pub fn weekday(days: i64) -> usize {
    (days + 3).rem_euclid(7) as usize
}

/// Now, on this computer's wall clock.
#[cfg(windows)]
pub fn now() -> Minute {
    // SAFETY: fills in the struct it's given
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    days_from_civil(t.wYear.into(), t.wMonth.into(), t.wDay.into()) * DAY + i64::from(t.wHour) * 60 + i64::from(t.wMinute)
}

/// Now, on this computer's wall clock (GLib's, which knows the time zone and summer time).
#[cfg(not(windows))]
pub fn now() -> Minute {
    let Ok(t) = gtk::glib::DateTime::now_local() else {
        // (no time zone to be had: UTC, as better than nothing)
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        return (secs / 60) as Minute;
    };
    days_from_civil(t.year(), t.month() as u32, t.day_of_month() as u32) * DAY + i64::from(t.hour() * 60 + t.minute())
}
