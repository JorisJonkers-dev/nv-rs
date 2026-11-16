//! SIGTERM, SIGINT and SIGHUP end the run with its summary written. The handler only sets a flag
//! (nothing else is safe there); the hitch thread sees it, writes the summary, restores the
//! default action and raises the signal again, so the process ends as it would have. A signal
//! somebody else already handles or ignores is left alone. Linux only.

use std::sync::atomic::{AtomicI32, Ordering};

static PENDING: AtomicI32 = AtomicI32::new(0);

/// The signal that came, 0 for none.
pub fn pending() -> i32 {
    PENDING.load(Ordering::Relaxed)
}

pub fn name(sig: i32) -> &'static str {
    match sig {
        1 => "HUP",
        2 => "INT",
        15 => "TERM",
        _ => "other",
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
        fn raise(signum: i32) -> i32;
    }

    extern "C" fn handler(sig: i32) {
        PENDING.store(sig, Ordering::Relaxed);
    }

    pub fn install() {
        for sig in [1, 2, 15] {
            // SAFETY: `signal` installs a handler that only stores to an atomic. A previous
            // handler other than the default (0) is put back at once.
            unsafe {
                let prev = signal(sig, handler as *const () as usize);
                if prev != 0 {
                    signal(sig, prev);
                }
            }
        }
    }

    /// Ends the process the way the signal would have.
    pub fn die(sig: i32) {
        // SAFETY: restores the default action and raises the signal on this thread.
        unsafe {
            signal(sig, 0);
            raise(sig);
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn install() {}
    pub fn die(_sig: i32) {}
}

pub use imp::{die, install};

#[cfg(test)]
mod tests {
    #[test]
    fn names() {
        assert_eq!(super::name(15), "TERM");
        assert_eq!(super::pending(), 0);
    }
}
