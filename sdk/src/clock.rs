//! Local wall-clock time of a Unix timestamp, like C `localtime`.

/// Calendar fields of a moment in the local time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// `dd.mm HH:MM` in the local time zone.
pub fn local_time(timestamp: f64) -> String {
    let time = local(timestamp);
    format!(
        "{:02}.{:02} {:02}:{:02}",
        time.day, time.month, time.hour, time.minute
    )
}

#[cfg(unix)]
pub fn local(timestamp: f64) -> LocalTime {
    use std::ffi::{c_char, c_int, c_long};

    #[repr(C)]
    struct Tm {
        sec: c_int,
        min: c_int,
        hour: c_int,
        mday: c_int,
        mon: c_int,
        year: c_int,
        wday: c_int,
        yday: c_int,
        isdst: c_int,
        gmtoff: c_long,
        zone: *const c_char,
    }

    unsafe extern "C" {
        fn localtime_r(time: *const c_long, result: *mut Tm) -> *mut Tm;
    }

    let time = timestamp.floor() as c_long;
    let mut tm = std::mem::MaybeUninit::<Tm>::zeroed();
    // SAFETY: both pointers are valid for the call; the result is read only when filled.
    let filled = unsafe { !localtime_r(&time, tm.as_mut_ptr()).is_null() };
    if !filled {
        return utc(timestamp);
    }
    // SAFETY: localtime_r succeeded and initialised the struct.
    let tm = unsafe { tm.assume_init() };
    LocalTime {
        year: tm.year as i64 + 1900,
        month: (tm.mon + 1) as u32,
        day: tm.mday as u32,
        hour: tm.hour as u32,
        minute: tm.min as u32,
        second: tm.sec as u32,
    }
}

#[cfg(not(unix))]
pub fn local(timestamp: f64) -> LocalTime {
    utc(timestamp)
}

/// Calendar fields in UTC.
pub fn utc(timestamp: f64) -> LocalTime {
    let seconds = timestamp.floor() as i64;
    let (days, rest) = (seconds.div_euclid(86400), seconds.rem_euclid(86400));
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    LocalTime {
        year,
        month: month as u32,
        day: day as u32,
        hour: (rest / 3600) as u32,
        minute: (rest % 3600 / 60) as u32,
        second: (rest % 60) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_day_month_and_time() {
        let time = utc(1_700_000_000.);
        assert_eq!(
            (
                time.year,
                time.month,
                time.day,
                time.hour,
                time.minute,
                time.second
            ),
            (2023, 11, 14, 22, 13, 20)
        );
        assert_eq!(utc(0.).year, 1970);
        assert_eq!(local_time(1_700_000_000.).len(), "14.11 22:13".len());
    }
}
