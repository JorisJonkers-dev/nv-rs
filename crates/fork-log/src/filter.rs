//! Level filters in the `RUST_LOG` syntax: `info,d3d9=debug,sound=off`.

use std::str::FromStr;

/// A message's severity; lower is more severe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    /// The lower-case name, as the JSON log writes it.
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }
}

impl FromStr for Level {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" | "err" | "1" => Ok(Level::Error),
            "warn" | "warning" | "2" => Ok(Level::Warn),
            "info" | "3" => Ok(Level::Info),
            "debug" | "4" => Ok(Level::Debug),
            "trace" | "5" => Ok(Level::Trace),
            _ => Err(()),
        }
    }
}

/// The most verbose level a target shows: 0 is off, else a [`Level`]'s number.
type Max = u8;

/// A set of per-target maxima with a default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    default: Max,
    directives: Vec<(String, Max)>,
}

impl Filter {
    /// Parses `spec`; a part that does not parse is skipped. `fallback` is the default when the
    /// spec names none.
    pub fn parse(spec: &str, fallback: Level) -> Filter {
        let mut f = Filter {
            default: fallback as Max,
            directives: Vec::new(),
        };
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.split_once('=') {
                Some((target, level)) => {
                    let max = if level.trim().eq_ignore_ascii_case("off") {
                        Some(0)
                    } else {
                        level.parse::<Level>().ok().map(|l| l as Max)
                    };
                    if let Some(max) = max {
                        f.directives.push((target.trim().to_string(), max));
                    }
                }
                None if part.eq_ignore_ascii_case("off") => f.default = 0,
                None => match part.parse::<Level>() {
                    Ok(l) => f.default = l as Max,
                    // A bare target (RUST_LOG's convention) shows everything of it.
                    Err(()) => f.directives.push((part.to_string(), Level::Trace as Max)),
                },
            }
        }
        // The longest target wins.
        f.directives
            .sort_by_key(|(t, _)| std::cmp::Reverse(t.len()));
        f
    }

    fn max_for(&self, target: &str) -> Max {
        for (t, max) in &self.directives {
            if target == t
                || (target.starts_with(t.as_str()) && target[t.len()..].starts_with("::"))
            {
                return *max;
            }
        }
        self.default
    }

    /// Whether a message of `level` from `target` passes.
    pub fn allows(&self, level: Level, target: &str) -> bool {
        (level as Max) <= self.max_for(target)
    }

    /// The most verbose level any target shows (0 when all is off).
    pub fn loosest(&self) -> u8 {
        self.directives
            .iter()
            .map(|(_, m)| *m)
            .chain(std::iter::once(self.default))
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_targets() {
        let f = Filter::parse("warn,d3d9=debug,sound=off", Level::Info);
        assert!(f.allows(Level::Warn, "files"));
        assert!(!f.allows(Level::Info, "files"));
        assert!(f.allows(Level::Debug, "d3d9"));
        assert!(f.allows(Level::Debug, "d3d9::device"));
        assert!(!f.allows(Level::Debug, "d3d9x"));
        assert!(!f.allows(Level::Error, "sound"));
        assert_eq!(f.loosest(), 4);
    }

    #[test]
    fn fallback_and_junk() {
        let f = Filter::parse("nonsense=loud,,window", Level::Info);
        assert!(f.allows(Level::Info, "files"));
        assert!(!f.allows(Level::Debug, "files"));
        assert!(f.allows(Level::Trace, "window"));
        assert!(Filter::parse("off", Level::Info).loosest() == 0);
    }

    #[test]
    fn longest_target_wins() {
        let f = Filter::parse("d3d9=warn,d3d9::device=trace", Level::Info);
        assert!(f.allows(Level::Trace, "d3d9::device"));
        assert!(!f.allows(Level::Info, "d3d9"));
    }
}
