//! The typed form of `nv-viewer --check` ([`host::check_installation`]), behind the feature
//! `engine`: what the run will find in a game folder, or why it refuses, as values a program can
//! match on instead of a process status and text.

use std::fmt;
use std::path::Path;

/// Why the check did not accept the installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CheckKind {
    /// The game was found and accepted, but this build does not run it on this processor or
    /// system yet (for example ARM Linux); it is the process status
    /// [`host::NOT_PROVIDED_STATUS`] (69).
    NotProvidedOnThisSystem,
    /// Anything else: no `FalloutNV.exe`, not version 1.4.0.525, an unsupported setting.
    Refused,
}

/// What the run will find, one line each, for display.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckReport {
    /// The lines of the report.
    pub lines: Vec<String>,
}

/// The check's refusal: its kind and the message to show.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CheckError {
    /// Which kind of refusal this is.
    pub kind: CheckKind,
    /// The message, as the engine words it.
    pub message: String,
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CheckError {}

/// Checks the game folder `data` the way `nv-viewer --check` does. It reads files and asks the
/// sound device what it offers, so a program with a window calls it off its UI thread.
pub fn check(data: &Path) -> Result<CheckReport, CheckError> {
    match host::check_installation(data) {
        Ok(lines) => Ok(CheckReport { lines }),
        Err(message) => Err(classify(message)),
    }
}

fn classify(message: String) -> CheckError {
    let kind = if host::error_status(&message) == host::NOT_PROVIDED_STATUS {
        CheckKind::NotProvidedOnThisSystem
    } else {
        CheckKind::Refused
    };
    CheckError { kind, message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn not_provided_message_is_classified_and_kept() {
        let message = format!("nv-rs does not run the game on this system yet: {}", "x");
        let err = classify(message.clone());
        assert_eq!(err.kind, CheckKind::NotProvidedOnThisSystem);
        assert_eq!(err.message, message);
    }

    #[test]
    fn other_message_is_refused() {
        let err = classify("somewhere: not found".to_string());
        assert_eq!(err.kind, CheckKind::Refused);
        assert_eq!(err.message, "somewhere: not found");
    }

    #[test]
    fn empty_folder_is_refused() {
        let dir = std::env::temp_dir().join(format!(
            "fork-api-check-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let result = check(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        let err = result.unwrap_err();
        assert_eq!(err.kind, CheckKind::Refused);
        assert!(err.message.contains("not found"), "{}", err.message);
    }

    #[test]
    fn display_is_the_message() {
        let err = CheckError {
            kind: CheckKind::Refused,
            message: "no".to_string(),
        };
        assert_eq!(err.to_string(), "no");
    }
}
