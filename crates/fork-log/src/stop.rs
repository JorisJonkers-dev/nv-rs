//! Stops: what a stop is, which are recoverable, and the policy `NV_STOP_POLICY`.
//!
//! A stop is the game's main thread ending its frames (`Game's main stopped in frame N: why`).
//! Most are not the platform's fault to hide: a function not translated or a memory fault means
//! the port itself is wrong, and going on would run the game on garbage. But a platform call the
//! game can handle failing (a file lookup, a device or audio call, a query) has a documented
//! failure result; with the policy `soft` the call logs an error and returns that result
//! ([`soft_stop`]) instead of stopping.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// What a stop's reason says went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopClass {
    /// Game code the port does not translate yet: always halts.
    Untranslated,
    /// A fault in guest memory: always halts.
    Memory,
    /// A platform service the port does not provide for these arguments: recoverable when the
    /// game can handle the failure.
    Platform,
    /// Anything else: halts.
    Other,
}

impl StopClass {
    pub fn name(self) -> &'static str {
        match self {
            StopClass::Untranslated => "untranslated",
            StopClass::Memory => "memory",
            StopClass::Platform => "platform",
            StopClass::Other => "other",
        }
    }

    /// Whether a stop of this class may become a soft stop.
    pub fn recoverable(self) -> bool {
        self == StopClass::Platform
    }
}

/// Classifies a stop's reason text.
pub fn classify(why: &str) -> StopClass {
    let w = why.to_ascii_lowercase();
    if w.contains("not translated") || w.contains("untranslated") {
        StopClass::Untranslated
    } else if [
        "access violation",
        "page fault",
        "unmapped",
        "out of bounds",
        "invalid memory",
        "segmentation",
    ]
    .iter()
    .any(|m| w.contains(m))
    {
        StopClass::Memory
    } else if w.contains("not provided") {
        StopClass::Platform
    } else {
        StopClass::Other
    }
}

/// The exe addresses a stop names: the words of its last `(in ...)`.
pub fn addresses(why: &str) -> Vec<String> {
    let Some(at) = why.rfind("(in ") else {
        return Vec::new();
    };
    let rest = &why[at + 4..];
    let rest = rest.split(')').next().unwrap_or("");
    rest.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
        .collect()
}

/// What a recoverable stop does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    Halt,
    Soft,
}

impl Policy {
    /// `halt` and `soft`; the default is `soft` off Windows (the Linux port's calls have a
    /// failure the game handles), `halt` on Windows where upstream's behaviour is the reference.
    pub fn parse(value: Option<&str>) -> Policy {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("halt") => Policy::Halt,
            Some("soft") => Policy::Soft,
            _ if cfg!(windows) => Policy::Halt,
            _ => Policy::Soft,
        }
    }
}

static POLICY: OnceLock<Policy> = OnceLock::new();
static SOFT_TOTAL: AtomicU64 = AtomicU64::new(0);
static SOFT_CALLS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

thread_local! {
    static OVERRIDE: std::cell::Cell<Option<Policy>> = const { std::cell::Cell::new(None) };
}

/// Runs `f` with `policy` in force on this thread (for tests: the environment's policy is read
/// once per process).
pub fn with_policy<R>(policy: Policy, f: impl FnOnce() -> R) -> R {
    let before = OVERRIDE.with(|o| o.replace(Some(policy)));
    struct Restore(Option<Policy>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(before);
    f()
}

/// The policy in force (this thread's override, else `NV_STOP_POLICY`, read once).
pub fn policy() -> Policy {
    if let Some(p) = OVERRIDE.with(std::cell::Cell::get) {
        return p;
    }
    *POLICY.get_or_init(|| Policy::parse(std::env::var("NV_STOP_POLICY").ok().as_deref()))
}

/// A recoverable platform failure: `call` (the Win32 or COM call, e.g. `FindFirstFileA`) failed
/// because of `why`. Under the `soft` policy this logs an error with the context, counts it and
/// returns `true`: the caller then returns the call's documented failure result to the game.
/// Under `halt` it returns `false` without a word: the caller stops as before.
pub fn soft_stop(area: &'static str, call: &str, why: fmt::Arguments<'_>) -> bool {
    soft_stop_with(policy(), area, call, why)
}

/// [`soft_stop`] under an explicit policy (what the tests use).
pub fn soft_stop_with(
    policy: Policy,
    area: &'static str,
    call: &str,
    why: fmt::Arguments<'_>,
) -> bool {
    if policy != Policy::Soft {
        return false;
    }
    SOFT_TOTAL.fetch_add(1, Ordering::Relaxed);
    let n = {
        let mut calls = SOFT_CALLS.lock().unwrap_or_else(|p| p.into_inner());
        let n = calls
            .get_or_insert_with(HashMap::new)
            .entry(call.to_string())
            .or_insert(0);
        *n += 1;
        *n
    };
    // A call that fails every frame says so five times, then every thousandth.
    if n <= 5 || n % 1000 == 0 {
        crate::event(
            crate::Level::Error,
            "stop",
            format_args!(
                "soft stop: {call}: {why} (the game gets the failure result and goes on; \
                 NV_STOP_POLICY=halt stops instead) [{n}]"
            ),
            &[("call", &call), ("area", &area), ("count", &n)],
        );
    }
    true
}

/// How many soft stops this run has had.
pub fn soft_stops() -> u64 {
    SOFT_TOTAL.load(Ordering::Relaxed)
}

/// The soft stops per call, most frequent first.
pub fn soft_stop_calls() -> Vec<(String, u64)> {
    let calls = SOFT_CALLS.lock().unwrap_or_else(|p| p.into_inner());
    let mut v: Vec<_> = calls
        .as_ref()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes() {
        assert_eq!(
            classify("function 0044fb20 not translated yet (in 0044fb20)"),
            StopClass::Untranslated
        );
        assert_eq!(
            classify("access violation reading 00000010"),
            StopClass::Memory
        );
        assert_eq!(
            classify("FindFirstFileA(x): the 8.3 short name of y is not provided"),
            StopClass::Platform
        );
        assert_eq!(classify("something else"), StopClass::Other);
        assert!(StopClass::Platform.recoverable());
        assert!(!StopClass::Untranslated.recoverable());
        assert!(!StopClass::Memory.recoverable());
        // An untranslated function that mentions a platform word stays a halt.
        assert!(
            !classify("function 00401000 not translated yet; call is not provided").recoverable()
        );
    }

    #[test]
    fn address_chain() {
        assert_eq!(
            addresses("boom (in 0044fb20 00401000, 0040a000)"),
            ["0044fb20", "00401000", "0040a000"]
        );
        assert!(addresses("no chain").is_empty());
    }

    #[test]
    fn policy_values() {
        assert_eq!(Policy::parse(Some("halt")), Policy::Halt);
        assert_eq!(Policy::parse(Some("SOFT")), Policy::Soft);
        assert_eq!(
            Policy::parse(None),
            if cfg!(windows) {
                Policy::Halt
            } else {
                Policy::Soft
            }
        );
    }

    #[test]
    fn override_wins_on_its_thread_and_is_restored() {
        let outside = policy();
        with_policy(Policy::Halt, || assert_eq!(policy(), Policy::Halt));
        with_policy(Policy::Soft, || {
            assert!(soft_stop("t", "OverrideCall", format_args!("x")));
            with_policy(Policy::Halt, || {
                assert!(!soft_stop("t", "OverrideCall", format_args!("x")));
            });
            assert_eq!(policy(), Policy::Soft);
        });
        assert_eq!(policy(), outside);
    }

    #[test]
    fn soft_stop_counts_and_halt_does_not() {
        let before = soft_stops();
        assert!(!soft_stop_with(
            Policy::Halt,
            "test",
            "TestCall",
            format_args!("x")
        ));
        assert_eq!(soft_stops(), before);
        assert!(soft_stop_with(
            Policy::Soft,
            "test",
            "TestCallSoft",
            format_args!("x {}", 1)
        ));
        assert_eq!(soft_stops(), before + 1);
        assert!(soft_stop_calls()
            .iter()
            .any(|(c, n)| c == "TestCallSoft" && *n >= 1));
    }
}
