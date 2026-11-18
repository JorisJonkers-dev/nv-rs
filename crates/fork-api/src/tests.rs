use super::*;
use fos::write::SaveWriter;

fn synthetic() -> Vec<u8> {
    let w = SaveWriter {
        plugins: vec!["FalloutNV.esm".into(), "Example.esp".into()],
        ..SaveWriter::default()
    };
    w.build().0
}

#[test]
fn summary_has_the_header_plugins_and_screenshot() {
    let s = summarize_save(&synthetic()).unwrap();
    assert_eq!(s.player_name, "Tester");
    assert_eq!(s.karma_title, "Drifter");
    assert_eq!(s.level, 2);
    assert_eq!(s.location, "Goodsprings");
    assert_eq!(s.play_time, "001.02.03");
    assert_eq!(s.save_number, 3);
    assert_eq!(s.language, "ENGLISH");
    assert_eq!(s.plugins, ["FalloutNV.esm", "Example.esp"]);
    assert_eq!((s.screenshot.width, s.screenshot.height), (4, 2));
    assert_eq!(s.screenshot.rgb.len(), 4 * 2 * 3);
    assert_eq!(s.screenshot.rgb[..3], [0, 1, 2]);
}

#[test]
fn summary_of_a_file_matches_the_bytes() {
    let dir = std::env::temp_dir().join(format!("fork-api-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Save 3.fos");
    std::fs::write(&path, synthetic()).unwrap();
    let from_file = save_summary(&path).unwrap();
    assert_eq!(from_file, summarize_save(&synthetic()).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_file_is_an_io_error_naming_the_path() {
    let path = std::env::temp_dir().join("fork-api-no-such-save.fos");
    match save_summary(&path) {
        Err(SaveError::Io { path: p, .. }) => assert_eq!(p, path),
        other => panic!("expected an io error, got {other:?}"),
    }
    let text = save_summary(&path).unwrap_err().to_string();
    assert!(text.contains("fork-api-no-such-save.fos"), "{text}");
}

#[test]
fn damaged_bytes_are_a_format_error() {
    let err = summarize_save(b"not a save").unwrap_err();
    assert_eq!(err.offset, 0);
    let mut cut = synthetic();
    cut.truncate(cut.len() - 3);
    assert!(summarize_save(&cut).is_err());
}

#[test]
fn format_error_displays_and_has_a_source() {
    use std::error::Error;
    let e = SaveError::Format(fos::Error::new(0, "x"));
    assert!(e.to_string().contains("not a readable save"));
    assert!(e.source().is_some());
}

#[test]
fn capabilities_is_the_extenders_file() {
    assert!(CAPABILITIES.contains("\"extenders\""));
    assert!(CAPABILITIES.contains("\"implemented\""));
}

#[test]
fn api_version_starts_at_one() {
    assert_eq!(API_VERSION, 1);
}

#[test]
fn readers_are_reachable_through_the_facade() {
    assert_eq!(&fos::MAGIC[..], b"FO3SAVEGAME");
    assert_eq!(game_folders::MAIN_MASTER, "FalloutNV.esm");
    let scan = Scan::default();
    assert!(scan.games.is_empty());
    let _: Option<Found> = None;
    let _: Option<Saves> = None;
    assert!(game_profile::settings::ini_value("[A]\nk=1\n", "A", "k").is_some());
    assert_eq!(esm::FormId(7).0, 7);
}
