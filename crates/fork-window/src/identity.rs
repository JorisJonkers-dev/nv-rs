// SPDX-License-Identifier: GPL-3.0-only
//! How a window names itself to the desktop: its Wayland `app_id` (X11: the `WM_CLASS`) and its
//! title.
//!
//! A run started under `gamerun` carries its label in `GAMERUN_LABEL`. The window then takes the
//! label as its application id, so the dock shows each run as an entry of its own, named by the
//! label, and the entry goes with the window. Without a label the id is `nv-rs`, which is the
//! `StartupWMClass` of `packaging/linux/nv-rs.desktop` (the dock's icon and name for a plain run).

/// The application id of a window that has no run label (matches `packaging/linux/nv-rs.desktop`).
pub const DEFAULT_APP_ID: &str = "nv-rs";

/// The label of this run: `GAMERUN_LABEL` when it is one `gamerun` would accept (letters, digits,
/// `.`, `_`, `-`; at most 100 characters; starts with a letter or digit), else none.
pub fn run_label() -> Option<String> {
    label_from(std::env::var("GAMERUN_LABEL").ok().as_deref())
}

/// [`run_label`] for a given value (pure, for tests).
pub fn label_from(value: Option<&str>) -> Option<String> {
    let v = value?;
    let mut chars = v.chars();
    let first = chars.next()?;
    let ok = first.is_ascii_alphanumeric()
        && v.len() <= 100
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    ok.then(|| v.to_owned())
}

/// The application id for a run with `label` (the label itself, else [`DEFAULT_APP_ID`]).
pub fn app_id(label: Option<&str>) -> String {
    label.unwrap_or(DEFAULT_APP_ID).to_owned()
}

/// The title for a run with `label`: the game's own title behind the label (`label: title`), so the
/// window is told apart in a switcher too. Without a label the game's title as it is.
pub fn titled(label: Option<&str>, title: &str) -> String {
    match label {
        Some(l) => format!("{l}: {title}"),
        None => title.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_names_the_window_and_none_gives_the_program_name() {
        assert_eq!(app_id(Some("gate14-mojave-ttw")), "gate14-mojave-ttw");
        assert_eq!(app_id(None), "nv-rs");
        assert_eq!(
            titled(Some("play-beautiful-2"), "Fallout"),
            "play-beautiful-2: Fallout"
        );
        assert_eq!(titled(None, "Fallout"), "Fallout");
    }

    #[test]
    fn only_a_label_gamerun_accepts_is_used() {
        assert_eq!(
            label_from(Some("bench.views_1-a")).as_deref(),
            Some("bench.views_1-a")
        );
        for bad in ["", "-x", ".x", "a b", "a/b", "ä", &"a".repeat(101)] {
            assert_eq!(label_from(Some(bad)), None, "{bad:?}");
        }
        assert_eq!(
            label_from(Some(&"a".repeat(100))).map(|s| s.len()),
            Some(100)
        );
        assert_eq!(label_from(None), None);
    }
}
