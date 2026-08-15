//! ISO-8601 timestamp conversion to epoch milliseconds.
//!
//! Agents write timestamps like `2026-08-15T04:22:14.101Z`. This is all the date
//! handling that is needed, so an extra dependency is avoided.

/// Days since the epoch for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Converts `YYYY-MM-DDTHH:MM:SS[.sss][Z|±HH:MM]` to epoch ms.
pub fn parse_iso8601_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> { s.get(from..to)?.parse::<i64>().ok() };

    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let min = num(14, 16)?;
    let sec = num(17, 19)?;

    let mut ms = 0i64;
    let mut i = 19;
    if b.get(19) == Some(&b'.') {
        let mut frac = String::new();
        i = 20;
        while i < b.len() && b[i].is_ascii_digit() && frac.len() < 3 {
            frac.push(b[i] as char);
            i += 1;
        }
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        while frac.len() < 3 {
            frac.push('0');
        }
        ms = frac.parse().unwrap_or(0);
    }

    // Optional timezone offset.
    let mut offset_min = 0i64;
    if let Some(&c) = b.get(i) {
        if c == b'+' || c == b'-' {
            let sign = if c == b'+' { 1 } else { -1 };
            let oh = num(i + 1, i + 3)?;
            let om = if b.get(i + 3) == Some(&b':') {
                num(i + 4, i + 6)?
            } else {
                num(i + 3, i + 5).unwrap_or(0)
            };
            offset_min = sign * (oh * 60 + om);
        }
    }

    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + min * 60 + sec - offset_min * 60;
    Some(seconds * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_epochs() {
        assert_eq!(parse_iso8601_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_iso8601_ms("2000-01-01T00:00:00Z"), Some(946_684_800_000));
    }

    #[test]
    fn milliseconds_and_timezone() {
        // Claude Code's real format (reference value: Date.parse).
        let a = parse_iso8601_ms("2026-08-15T04:22:14.101Z").unwrap();
        assert_eq!(a, 1_786_767_734_101);
        let b = parse_iso8601_ms("2026-08-15T04:22:15.101Z").unwrap();
        assert_eq!(b - a, 1000);
        // The same instant written with an offset.
        let c = parse_iso8601_ms("2026-08-15T06:22:14.101+02:00").unwrap();
        assert_eq!(a, c);
    }

    #[test]
    fn nanoseconds_are_truncated_to_ms() {
        let a = parse_iso8601_ms("2026-07-17T13:28:16.123456789Z").unwrap();
        let b = parse_iso8601_ms("2026-07-17T13:28:16.123Z").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn invalid_inputs() {
        assert!(parse_iso8601_ms("").is_none());
        assert!(parse_iso8601_ms("yesterday").is_none());
        assert!(parse_iso8601_ms("2026-08-15").is_none());
    }
}
