//! Calendar dates without a date crate.

/// Hinnant's `civil_from_days`: days since 1970-01-01 to (year, month, day).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Today as `yyyy-mm-dd` in local time.
pub fn local_date() -> String {
    // SAFETY: `time` accepts a null pointer; `localtime_r` fills the zeroed
    // `tm` we own.
    let (now, offset) = unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        (now as i64, tm.tm_gmtoff as i64)
    };
    let (y, m, d) = civil_from_days((now + offset).div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// A moment in local time: the calendar date, the weekday (0 is Monday) and the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    /// Days since 1970-01-01 in local time: equal for two moments on one local day.
    pub day_number: i64,
}

/// The local-time offset from UTC at Unix time `at`, in seconds.
pub fn local_offset(at: i64) -> i64 {
    // SAFETY: `localtime_r` fills the zeroed `tm` we own from a `time_t` we own.
    unsafe {
        let t = at as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_gmtoff as i64
    }
}

/// Unix time `at` in local time.
pub fn local(at: i64) -> Local {
    in_zone(at, local_offset(at))
}

/// Unix time `at` at `offset` seconds from UTC.
pub fn in_zone(at: i64, offset: i64) -> Local {
    let t = at + offset;
    let day_number = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(day_number);
    Local {
        year,
        month,
        day,
        // 1970-01-01 was a Thursday (3 with Monday as 0).
        weekday: (day_number + 3).rem_euclid(7) as u32,
        hour: (secs / 3600) as u32,
        minute: (secs % 3600 / 60) as u32,
        day_number,
    }
}

/// Short weekday names, Monday first.
pub const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// When `at` is, said from `now` in local time: `16:30` today, `Tomorrow 09:00`, `Mon 09:00`
/// within the week, else `12 Nov 09:00`.
pub fn when_label(at: i64, now: i64) -> String {
    when_label_in(local(at), local(now))
}

/// [`when_label`] for two moments already in one zone.
pub fn when_label_in(at: Local, now: Local) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let clock = format!("{:02}:{:02}", at.hour, at.minute);
    match at.day_number - now.day_number {
        0 => clock,
        1 => format!("Tomorrow {clock}"),
        -1 => format!("Yesterday {clock}"),
        2..=6 => format!("{} {clock}", WEEKDAYS[at.weekday as usize]),
        _ => format!("{} {} {clock}", at.day, MONTHS[(at.month - 1) as usize]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moments_in_a_zone_and_their_labels() {
        // 2025-10-29 (a Wednesday) 14:05 UTC, read at UTC+2.
        let at = 20390 * 86_400 + 14 * 3600 + 5 * 60;
        let l = in_zone(at, 7200);
        assert_eq!(
            (l.year, l.month, l.day, l.hour, l.minute),
            (2025, 10, 29, 16, 5)
        );
        assert_eq!(WEEKDAYS[l.weekday as usize], "Wed");
        let now = in_zone(at - 3600, 7200);
        assert_eq!(when_label_in(l, now), "16:05");
        assert_eq!(
            when_label_in(in_zone(at + 86_400, 7200), now),
            "Tomorrow 16:05"
        );
        assert_eq!(
            when_label_in(in_zone(at + 3 * 86_400, 7200), now),
            "Sat 16:05"
        );
        assert_eq!(
            when_label_in(in_zone(at + 20 * 86_400, 7200), now),
            "18 Nov 16:05"
        );
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(20390), (2025, 10, 29));
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn local_date_is_well_formed() {
        let d = local_date();
        assert_eq!(d.len(), 10);
        assert_eq!(&d[4..5], "-");
    }
}
