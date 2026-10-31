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

#[cfg(test)]
mod tests {
    use super::*;

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
