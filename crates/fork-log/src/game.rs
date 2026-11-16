//! The `game` event trace: what the game and the port did, in order, to diagnose a run from its
//! log (menus, movies, loading screens, audio, input, draws of the screen's surfaces).
//!
//! Every event is on target `game`, at `info` (a state change: a menu opens, a movie starts) or
//! `debug` (detail), and carries the main frame and a monotonic millisecond clock, in the text
//! (`[f123 +4567ms] movie.open: ...`) and as the JSON fields `kind` and `ms`. Enable it with
//! `NV_LOG=game=debug`. Repeating events go through [`throttled`]: the first few of a key are
//! logged, then every thousandth, and the counts are in the exit summary.

use crate::Level;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// Milliseconds since the first event or call of this process.
pub fn ms() -> u64 {
    start().elapsed().as_millis() as u64
}

/// One event: `kind` is a dotted name (`menu.open`, `movie.skip`), `args` its detail.
pub fn event(level: Level, kind: &str, args: fmt::Arguments<'_>) {
    if !crate::enabled(level, "game") {
        return;
    }
    let ms = ms();
    let frame = crate::frame().map_or_else(|| "-".to_string(), |f| f.to_string());
    crate::event(
        level,
        "game",
        format_args!("[f{frame} +{ms}ms] {kind}: {args}"),
        &[("kind", &kind), ("ms", &ms)],
    );
}

static COUNTS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

/// A repeating event: the first five of `key` and then every thousandth are logged (at `level`),
/// all are counted ([`counts`]).
pub fn throttled(level: Level, key: &str, args: fmt::Arguments<'_>) {
    let n = {
        let mut c = COUNTS.lock().unwrap_or_else(|p| p.into_inner());
        let n = c
            .get_or_insert_with(HashMap::new)
            .entry(key.to_string())
            .or_insert(0);
        *n += 1;
        *n
    };
    if n <= 5 || n % 1000 == 0 {
        event(level, key, format_args!("{args} (#{n})"));
    }
}

/// The counts of throttled events, most frequent first.
pub fn counts() -> Vec<(String, u64)> {
    let c = COUNTS.lock().unwrap_or_else(|p| p.into_inner());
    let mut v: Vec<_> = c
        .as_ref()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

/// `game_info!("movie.open", "{} {}x{}", name, w, h)`.
#[macro_export]
macro_rules! game_info { ($k:expr, $($a:tt)+) => { $crate::game::event($crate::Level::Info, $k, format_args!($($a)+)) }; }
#[macro_export]
macro_rules! game_debug { ($k:expr, $($a:tt)+) => { $crate::game::event($crate::Level::Debug, $k, format_args!($($a)+)) }; }
/// A repeating event at debug level, throttled by its kind.
#[macro_export]
macro_rules! game_often { ($k:expr, $($a:tt)+) => { $crate::game::throttled($crate::Level::Debug, $k, format_args!($($a)+)) }; }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_counts_everything() {
        for i in 0..2500 {
            throttled(Level::Trace, "test.repeat", format_args!("i={i}"));
        }
        let n = counts()
            .into_iter()
            .find(|(k, _)| k == "test.repeat")
            .unwrap()
            .1;
        assert_eq!(n, 2500);
        assert!(ms() < 60_000);
    }
}
