//! HTTP dates (RFC 9110 §5.6.7's IMF-fixdate, `Sun, 06 Nov 1994 08:49:37
//! GMT`): a relay's clock as its WebSocket handshake's `Date` header says
//! (ruling FR12b-3: `rl-server --nat-check` prints how far each relay's
//! clock is from this PC's). Sans-IO; parsing never panics.

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's
/// `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day since 1970-01-01 (`civil_from_days`).
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `unix` (seconds) as an IMF-fixdate.
pub fn format(unix: u64) -> String {
    let secs = i64::try_from(unix).unwrap_or(i64::MAX / 2);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    let weekday = DAYS[usize::try_from(days.rem_euclid(7)).unwrap_or(0)];
    let month = MONTHS[usize::try_from(m - 1).unwrap_or(0)];
    format!("{weekday}, {d:02} {month} {y:04} {:02}:{:02}:{:02} GMT", rest / 3600, rest / 60 % 60, rest % 60)
}

/// An IMF-fixdate's unix seconds; `None` for anything else (the obsolete
/// RFC 850 and asctime forms are not read).
pub fn parse(text: &str) -> Option<u64> {
    let mut f = text.trim().split(' ');
    let (_weekday, day, month, year, time, zone) =
        (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
    if f.next().is_some() || zone != "GMT" || day.len() != 2 || year.len() != 4 {
        return None;
    }
    let num = |s: &str| -> Option<i64> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        s.parse().ok()
    };
    let d = num(day)?;
    let m = i64::try_from(MONTHS.iter().position(|&n| n == month)?).ok()? + 1;
    let y = num(year)?;
    let mut hms = time.split(':');
    let (h, mi, s) = (num(hms.next()?)?, num(hms.next()?)?, num(hms.next()?)?);
    if hms.next().is_some() || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 || y < 1970 {
        return None;
    }
    let secs = days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60 + s;
    u64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_and_write_as_rfc_9110_says() {
        assert_eq!(parse("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777));
        assert_eq!(format(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(format(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        for unix in [0, 951_782_400, 1_800_000_000, 4_102_444_800] {
            assert_eq!(parse(&format(unix)), Some(unix), "{}", format(unix));
        }
        for bad in [
            "",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
            "Sun, 06 Nov 1994 08:49:37 UTC",
            "Sun, 6 Nov 1994 08:49:37 GMT",
            "Sun, 06 Nox 1994 08:49:37 GMT",
            "Sun, 06 Nov 1994 24:49:37 GMT",
            "Sun, 06 Nov 1994 08:49 GMT",
            "Sun, 06 Nov +994 08:49:37 GMT",
        ] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    proptest::proptest! {
        #[test]
        fn any_text_parses_without_panic(text in ".{0,64}") {
            let _ = parse(&text);
        }

        #[test]
        fn every_date_round_trips(unix in 0u64..253_402_300_800) {
            proptest::prop_assert_eq!(parse(&format(unix)), Some(unix));
        }
    }
}
