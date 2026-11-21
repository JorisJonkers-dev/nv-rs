// SPDX-License-Identifier: GPL-3.0-only
//! `NV_STOP_AFTER_FRAMES=N` (nv-rs#463): the engine ends with exit status 0 after N of main's
//! frames, with the real threads kept (unlike `NV_REPEATABLE`, which steps the threads). It is a
//! start check, not a test mode: the run is the player's run until frame N, so a build that
//! stops early (nv-rs#451 stopped in frame 2) ends with the stop's own status and never with 0.
//! Off unless the variable is set; a value that is not a positive whole number is refused at
//! start with a plain message.

/// The variable's name.
pub const VARIABLE: &str = "NV_STOP_AFTER_FRAMES";

/// The frame count in `value`: `Ok(None)` for unset or empty, `Ok(Some(n))` for a positive whole
/// number, `Err(message)` for anything else.
pub fn parse(value: Option<&str>) -> Result<Option<u64>, String> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    match v.parse::<u64>() {
        Ok(n) if n > 0 => Ok(Some(n)),
        _ => Err(format!(
            "{VARIABLE}={v}: give the number of main frames after which the run ends, a whole number above 0 \
             (unset the variable for a normal run)"
        )),
    }
}

/// [`parse`] of the process's environment.
pub fn from_env() -> Result<Option<u64>, String> {
    parse(std::env::var(VARIABLE).ok().as_deref())
}

/// Whether `main_frames` of main's frames have run, so the run ends.
pub fn reached(limit: Option<u64>, main_frames: u64) -> bool {
    limit.is_some_and(|n| main_frames >= n)
}

/// The one log line when the switch ends the run: a check tells "ended by the switch after N
/// frames" from any other exit by it.
pub fn end_line(limit: u64, main_frames: u64) -> String {
    format!("{VARIABLE}: ended by the switch after {main_frames} main frames (limit {limit}); exit status 0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_or_empty_is_off() {
        assert_eq!(parse(None), Ok(None));
        assert_eq!(parse(Some("")), Ok(None));
        assert_eq!(parse(Some("  ")), Ok(None));
    }

    #[test]
    fn a_positive_whole_number_is_the_limit() {
        assert_eq!(parse(Some("300")), Ok(Some(300)));
        assert_eq!(parse(Some(" 1 ")), Ok(Some(1)));
    }

    #[test]
    fn anything_else_is_refused_with_the_value_named() {
        for bad in ["0", "-5", "abc", "1.5", "10 frames", "0x10"] {
            let e = parse(Some(bad)).unwrap_err();
            assert!(e.contains(&format!("NV_STOP_AFTER_FRAMES={bad}")), "{e}");
            assert!(e.contains("whole number above 0"), "{e}");
        }
    }

    #[test]
    fn ends_at_the_limit_and_never_before_or_when_off() {
        assert!(!reached(None, 1_000_000));
        assert!(!reached(Some(300), 299));
        assert!(reached(Some(300), 300));
        assert!(reached(Some(300), 301));
    }

    #[test]
    fn the_line_names_the_switch_the_frames_and_the_status() {
        let l = end_line(300, 300);
        assert!(
            l.contains("NV_STOP_AFTER_FRAMES")
                && l.contains("after 300 main frames")
                && l.contains("exit status 0")
        );
    }
}
