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

/// The monitor chosen, where it is in the list the window system gave, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    pub index: usize,
    pub monitor: Monitor,
    /// `NV_MONITOR`, `last used`, `primary` or `first`.
    pub why: &'static str,
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

/// What `NV_MONITOR` names: an index in the system's list, or an output's name (`DP-1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wanted {
    Index(usize),
    Name(String),
}

/// `NV_MONITOR`'s value; empty is none.
pub fn parse_wanted(value: Option<&str>) -> Option<Wanted> {
    let v = value?.trim();
    if v.is_empty() {
        return None;
    }
    Some(
        v.parse()
            .map_or_else(|_| Wanted::Name(v.to_string()), Wanted::Index),
    )
}

/// The monitor to use, in this order: `wanted` (`NV_MONITOR`) when it names one in the list,
/// the output the game last ran on (`last`, by name: Wayland has no query for the monitor holding
/// the focus or the cursor, so the one the compositor placed the window on last time stands in),
/// the `primary` the system names, the first. Monitors with no area are never picked.
pub fn pick(
    monitors: &[Monitor],
    names: &[String],
    wanted: Option<&Wanted>,
    last: Option<&str>,
    primary: Option<usize>,
) -> Option<Display> {
    let usable = |i: &usize| {
        monitors
            .get(*i)
            .is_some_and(|m| m.width > 0 && m.height > 0)
    };
    let by_name = |n: &str| names.iter().position(|x| x == n);
    let wanted_at = match wanted {
        Some(Wanted::Index(i)) => Some(*i),
        Some(Wanted::Name(n)) => by_name(n),
        None => None,
    };
    let candidates = [
        (wanted_at, "NV_MONITOR"),
        (last.and_then(by_name), "last used"),
        (primary, "primary"),
    ];
    for (at, why) in candidates {
        if let Some(index) = at.filter(usable) {
            return Some(Display {
                index,
                monitor: monitors[index],
                why,
            });
        }
    }
    (0..monitors.len()).find(usable).map(|index| Display {
        index,
        monitor: monitors[index],
        why: "first",
    })
}

fn state_file() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/state"))
        })?;
    Some(base.join("nv-rs").join("last-monitor"))
}

/// The name of the output the game's window last landed on (kept across runs), if any.
pub fn last_monitor() -> Option<String> {
    let text = std::fs::read_to_string(state_file()?).ok()?;
    Some(text.trim().to_string()).filter(|t| !t.is_empty())
}

/// Keeps the output the window landed on for the next run's pick.
pub fn remember_monitor(name: &str) {
    if let Some(f) = state_file() {
        let _ = f.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(f, name);
    }
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

    const fn m(w: u32, h: u32, mhz: u32) -> Monitor {
        Monitor {
            width: w,
            height: h,
            refresh_mhz: mhz,
        }
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    const LIST: [Monitor; 3] = [
        m(1080, 1920, 60000),
        m(2560, 1440, 143912),
        m(1920, 1080, 60000),
    ];

    #[test]
    fn the_override_wins_then_the_last_then_the_primary_then_the_first() {
        let n = names(&["A", "B", "C"]);
        let w = Wanted::Index(2);
        assert_eq!(
            pick(&LIST, &n, Some(&w), Some("B"), Some(1)).unwrap().index,
            2
        );
        let d = pick(&LIST, &n, None, Some("B"), Some(0)).unwrap();
        assert_eq!((d.index, d.why), (1, "last used"));
        assert_eq!(pick(&LIST, &n, None, None, Some(2)).unwrap().why, "primary");
        let d = pick(&LIST, &n, None, Some("gone"), None).unwrap();
        assert_eq!((d.index, d.why), (0, "first"));
    }

    #[test]
    fn an_output_name_is_an_override_too() {
        let n = names(&["DP-1", "DP-2", "HDMI-1"]);
        let w = Wanted::Name("DP-2".into());
        assert_eq!(pick(&LIST, &n, Some(&w), None, None).unwrap().index, 1);
        let w = Wanted::Name("nope".into());
        assert_eq!(pick(&LIST, &n, Some(&w), None, Some(2)).unwrap().index, 2);
    }

    #[test]
    fn a_bad_override_or_a_monitor_without_area_falls_through() {
        let list = [m(0, 0, 0), m(2560, 1440, 144000)];
        let n = names(&["A", "B"]);
        let w = Wanted::Index(9);
        assert_eq!(pick(&list, &n, Some(&w), None, None).unwrap().index, 1);
        let w = Wanted::Index(0);
        assert_eq!(pick(&list, &n, Some(&w), None, Some(0)).unwrap().index, 1);
        assert_eq!(pick(&[], &[], None, None, None), None);
    }

    #[test]
    fn the_refresh_is_rounded_to_hertz() {
        assert_eq!(hz_of(143_912), 144);
        assert_eq!(hz_of(59_940), 60);
        assert_eq!(hz_of(0), 60);
    }

    #[test]
    fn the_override_is_an_index_or_a_name() {
        assert_eq!(parse_wanted(Some(" 1 ")), Some(Wanted::Index(1)));
        assert_eq!(
            parse_wanted(Some("DP-1")),
            Some(Wanted::Name("DP-1".into()))
        );
        assert_eq!(parse_wanted(Some("  ")), None);
        assert_eq!(parse_wanted(None), None);
    }
}
