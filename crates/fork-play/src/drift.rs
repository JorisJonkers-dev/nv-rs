//! The drift guard: `viewer/src/main.rs` is included in this crate by `#[path]`, so it compiles
//! under this crate's dependency list, not the viewer's. When the viewer gains a dependency this
//! crate lacks, the include stops compiling on some system (or worse, a target-only dependency
//! is missed on the system that is not built). These tests read both manifests and fail first.

use std::collections::BTreeSet;
use std::path::Path;

/// `(section, name)` of every dependency in `manifest`: the section is the table header
/// (`dependencies`, `dev-dependencies`, `target.'cfg(not(windows))'.dependencies`). Build
/// dependencies are not read: `build.rs` is not run for an included module.
fn dependencies(manifest: &str) -> BTreeSet<(String, String)> {
    let mut found = BTreeSet::new();
    let mut section = String::new();
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = header.trim().to_string();
        } else if !line.starts_with('#')
            && (section.ends_with("dependencies") && !section.ends_with("build-dependencies"))
        {
            if let Some((name, _)) = line.split_once('=') {
                found.insert((section.clone(), name.trim().to_string()));
            }
        }
    }
    found
}

/// What the viewer depends on and this crate does not.
fn missing(viewer: &str, play: &str) -> Vec<(String, String)> {
    let have = dependencies(play);
    dependencies(viewer)
        .into_iter()
        .filter(|d| !have.contains(d))
        .collect()
}

fn manifest(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn the_viewers_dependencies_are_this_crates_too() {
    let gone = missing(
        &manifest("../../viewer/Cargo.toml"),
        &manifest("Cargo.toml"),
    );
    assert!(
        gone.is_empty(),
        "viewer/Cargo.toml has dependencies crates/fork-play/Cargo.toml lacks (add them under the \
         same section): {gone:?}"
    );
}

#[test]
fn a_dependency_only_the_viewer_has_is_found() {
    let viewer = "[dependencies]\nhost = { path = \"../host\" }\nextra = \"1\"\n\
                  [target.'cfg(not(windows))'.dependencies]\ngame-folders = { path = \"x\" }\n\
                  [dev-dependencies]\ntempfile = \"3\"\n[build-dependencies]\nembed = \"1\"\n";
    let play = "[dependencies]\nhost = { path = \"../host\" }\n";
    let expected: BTreeSet<(String, String)> = [
        ("dependencies", "extra"),
        ("dev-dependencies", "tempfile"),
        ("target.'cfg(not(windows))'.dependencies", "game-folders"),
    ]
    .into_iter()
    .map(|(s, n)| (s.to_string(), n.to_string()))
    .collect();
    assert_eq!(
        missing(viewer, play).into_iter().collect::<BTreeSet<_>>(),
        expected
    );
}

#[test]
fn a_target_dependency_in_the_wrong_section_is_missing() {
    let viewer = "[target.'cfg(not(windows))'.dependencies]\ngame-folders = \"1\"\n";
    let play = "[dependencies]\ngame-folders = \"1\"\n";
    assert_eq!(missing(viewer, play).len(), 1);
}

#[test]
fn extra_dependencies_of_this_crate_are_fine() {
    let viewer = "[dependencies]\nhost = \"1\"\n";
    let play = "[dependencies]\nhost = \"1\"\nmore = \"1\"\n";
    assert!(missing(viewer, play).is_empty());
}
