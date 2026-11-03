//! Miniflux's RFC 3339 timestamps (`2026-10-24T08:12:00+02:00`, `…T06:12:00.123Z`) as unix milliseconds, for
//! paging. (The article header formats them in QML, in local time.) Small enough not to need a date crate.

/// Unix milliseconds, or `None` if `s` isn't RFC 3339.
pub fn unix_millis(s: &str) -> Option<i64> {
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.split('-').map(str::parse::<i64>);
    let (year, month, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let hh: i64 = rest.get(0..2)?.parse().ok()?;
    let mm: i64 = rest.get(3..5)?.parse().ok()?;
    let ss: i64 = rest.get(6..8)?.parse().ok()?;
    let after = rest.get(8..)?;
    let (millis, zone) = match after.strip_prefix('.') {
        Some(frac) => {
            let digits = frac.find(|c: char| !c.is_ascii_digit()).unwrap_or(frac.len());
            let ms = format!("{:0<3}", &frac[..digits.min(3)]).parse::<i64>().ok()?;
            (ms, &frac[digits..])
        }
        None => (0, after),
    };
    let offset = match zone {
        "Z" | "z" | "" => 0,
        z => {
            let sign = match z.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let h: i64 = z.get(1..3)?.parse().ok()?;
            let m: i64 = z.get(4..6)?.parse().ok()?;
            sign * (h * 3600 + m * 60)
        }
    };
    let secs = days_from_civil(year, month, day) * 86_400 + hh * 3600 + mm * 60 + ss - offset;
    Some(secs * 1000 + millis)
}

/// "24 Oct 2026, 08:12", as the article header shows it in the terminal: at the offset the server gave, which is
/// the time zone set in Miniflux.
#[cfg(feature = "tui")]
pub fn display(s: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    unix_millis(s)?; // it is a date
    let (year, month, day) = (s.get(0..4)?, s.get(5..7)?, s.get(8..10)?);
    let month = MONTHS.get(month.parse::<usize>().ok()?.checked_sub(1)?)?;
    let day = day.trim_start_matches('0');
    Some(format!("{day} {month} {year}, {}", s.get(11..16)?))
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The other way round: `2026-10-01T00:00:00.000Z` for unix milliseconds. For test data.
#[cfg(test)]
pub fn rfc3339_utc(millis: i64) -> String {
    let (secs, ms) = (millis.div_euclid(1000), millis.rem_euclid(1000));
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{ms:03}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "tui")]
    #[test]
    fn display_reads_as_written_at_the_servers_offset() {
        assert_eq!(display("2026-10-04T08:12:00+02:00").as_deref(), Some("4 Oct 2026, 08:12"));
        assert_eq!(display("2026-01-31T23:59:59.5Z").as_deref(), Some("31 Jan 2026, 23:59"));
        assert_eq!(display("yesterday"), None);
        assert_eq!(display("2026-13-04T08:12:00Z"), None);
    }

    #[test]
    fn parses_offsets_and_fractions() {
        assert_eq!(unix_millis("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_millis("2026-10-24T08:12:00+02:00"), Some(1_792_822_320_000));
        assert_eq!(unix_millis("2026-10-24T06:12:00.123456Z"), Some(1_792_822_320_123));
        assert_eq!(unix_millis("2026-10-24T06:12:00.5Z"), Some(1_792_822_320_500));
        assert_eq!(unix_millis("2026-10-24T01:12:00-05:00"), Some(1_792_822_320_000));
        assert_eq!(unix_millis("yesterday"), None);
        assert_eq!(unix_millis("2026-10-24T01:12:00?05:00"), None);
    }

    #[test]
    fn formats_back() {
        for ms in [0, 1_792_822_320_123, 951_782_400_000 /* 2000-02-29 */, 4_107_542_400_999] {
            assert_eq!(unix_millis(&rfc3339_utc(ms)), Some(ms));
        }
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00.000Z");
    }
}
