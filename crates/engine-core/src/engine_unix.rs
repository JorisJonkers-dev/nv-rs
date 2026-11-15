//! The real clock's start off Windows (fork-owned; `Engine::start_real_clock`): the machine's
//! uptime in milliseconds, as `GetTickCount` counts it. Code that takes a zero start stamp as
//! long ago depends on a start past zero (the loading menu's intro-movie check).

/// `/proc/uptime`'s first field (seconds since boot, with a fraction) as milliseconds, wrapped to
/// 32 bits as `GetTickCount` wraps. `None` for text that is not an uptime.
pub(crate) fn parse_uptime_ms(text: &str) -> Option<u32> {
    let first = text.split_whitespace().next()?;
    let (secs, frac) = first.split_once('.').unwrap_or((first, ""));
    if secs.is_empty() || !secs.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis: u64 = format!("{frac:0<3}")[..3].parse().ok()?;
    let secs: u64 = secs.parse().ok()?;
    Some((secs.wrapping_mul(1000).wrapping_add(millis)) as u32)
}

/// The machine's uptime now in milliseconds; `None` where `/proc/uptime` is not there.
pub(crate) fn tick_count() -> Option<u32> {
    parse_uptime_ms(&std::fs::read_to_string("/proc/uptime").ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_text_gives_milliseconds() {
        assert_eq!(parse_uptime_ms("12345.67 98765.43\n"), Some(12_345_670));
        assert_eq!(parse_uptime_ms("5.5 1.0"), Some(5_500));
        assert_eq!(parse_uptime_ms("0.04 0.00"), Some(40));
        assert_eq!(parse_uptime_ms("7"), Some(7_000));
        assert_eq!(parse_uptime_ms("1.23456 2"), Some(1_234));
    }

    #[test]
    fn the_tick_count_wraps_at_32_bits_like_gettickcount() {
        // 50 days: 4_320_000 s = 4_320_000_000 ms, past u32::MAX (4_294_967_295).
        assert_eq!(parse_uptime_ms("4320000.00 0"), Some(25_032_704));
    }

    #[test]
    fn text_that_is_no_uptime_gives_none() {
        assert_eq!(parse_uptime_ms(""), None);
        assert_eq!(parse_uptime_ms("up 3 days"), None);
        assert_eq!(parse_uptime_ms("-1.0 0"), None);
        assert_eq!(parse_uptime_ms(".5 1"), None);
    }

    #[test]
    fn the_machine_has_been_up_for_some_time() {
        let ms = tick_count().expect("/proc/uptime");
        assert!(ms > 0);
    }

    #[test]
    fn the_real_clock_starts_at_the_machines_uptime() {
        let mut e = crate::Engine::new();
        assert_eq!(e.clock_ms, 0);
        e.start_real_clock();
        assert!(e.clock_ms > 0);
    }
}
