//! Which display the game runs on: the monitor, its size and its refresh rate.
//!
//! The pick is pure ([`pick`]) so it is tested without a display; [`probe`] asks the compositor
//! once (on a short-lived event loop of its own) and keeps the answer for the process.

use std::sync::OnceLock;

/// One monitor as the window system reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    /// Millihertz (144000 for 144 Hz); 0 where the system does not say.
    pub refresh_mhz: u32,
}

/// The monitor chosen, and where it is in the list the window system gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    pub index: usize,
    pub monitor: Monitor,
}

impl Display {
    /// The refresh rate in whole hertz, 60 where the display does not say.
    pub fn refresh_hz(&self) -> u32 {
        hz_of(self.monitor.refresh_mhz)
    }
}

/// Whole hertz of a rate in millihertz (rounded), 60 for none.
pub fn hz_of(refresh_mhz: u32) -> u32 {
    if refresh_mhz == 0 {
        60
    } else {
        (refresh_mhz + 500) / 1000
    }
}

/// The monitor to use: `wanted` (`NV_MONITOR`) when it is in the list, else the `primary` the
/// system names, else the first (Wayland has no primary and no query for the monitor holding the
/// focus or the cursor, so the first output stands in). Monitors with no area are never picked.
pub fn pick(
    monitors: &[Monitor],
    wanted: Option<usize>,
    primary: Option<usize>,
) -> Option<Display> {
    let usable = |i: &usize| {
        monitors
            .get(*i)
            .is_some_and(|m| m.width > 0 && m.height > 0)
    };
    wanted
        .into_iter()
        .chain(primary)
        .chain(0..monitors.len())
        .find(usable)
        .map(|index| Display {
            index,
            monitor: monitors[index],
        })
}

/// `NV_MONITOR`'s value: a monitor's index in the system's list.
pub fn parse_wanted(value: Option<&str>) -> Option<usize> {
    value.and_then(|v| v.trim().parse().ok())
}

/// The display the game runs on, asked of the window system on first use and kept; `None` where
/// there is no display to ask.
pub fn display() -> Option<Display> {
    static CHOSEN: OnceLock<Option<Display>> = OnceLock::new();
    *CHOSEN.get_or_init(probe)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn probe() -> Option<Display> {
    // A thread of its own: its event loop is made and dropped there, before the window's.
    std::thread::Builder::new()
        .name("nv-display-probe".into())
        .spawn(crate::platform::probe_monitors)
        .ok()?
        .join()
        .ok()
        .flatten()
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn probe() -> Option<Display> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(w: u32, h: u32, mhz: u32) -> Monitor {
        Monitor {
            width: w,
            height: h,
            refresh_mhz: mhz,
        }
    }

    #[test]
    fn the_override_wins_then_the_primary_then_the_first() {
        let list = [
            m(1080, 1920, 60000),
            m(2560, 1440, 143912),
            m(1920, 1080, 60000),
        ];
        assert_eq!(pick(&list, Some(2), Some(1)).unwrap().index, 2);
        assert_eq!(pick(&list, None, Some(1)).unwrap().index, 1);
        assert_eq!(pick(&list, None, None).unwrap().index, 0);
    }

    #[test]
    fn a_bad_override_or_a_monitor_without_area_falls_through() {
        let list = [m(0, 0, 0), m(2560, 1440, 144000)];
        assert_eq!(pick(&list, Some(9), None).unwrap().index, 1);
        assert_eq!(pick(&list, Some(0), Some(0)).unwrap().index, 1);
        assert_eq!(pick(&[], None, None), None);
    }

    #[test]
    fn the_refresh_is_rounded_to_hertz() {
        assert_eq!(hz_of(143_912), 144);
        assert_eq!(hz_of(59_940), 60);
        assert_eq!(hz_of(0), 60);
        assert_eq!(
            pick(&[m(2560, 1440, 143_912)], None, None)
                .unwrap()
                .refresh_hz(),
            144
        );
    }

    #[test]
    fn the_override_is_an_index() {
        assert_eq!(parse_wanted(Some(" 1 ")), Some(1));
        assert_eq!(parse_wanted(Some("DP-1")), None);
        assert_eq!(parse_wanted(None), None);
    }
}
