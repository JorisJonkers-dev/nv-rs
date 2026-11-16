//! The whole recorder in one process: frames, a hitch, the JSON log, the CSV marker and the
//! run summary file next to the log.

use fork_hitch::{Context, Counter, Screen};
use std::time::Duration;

#[test]
fn a_hitch_reaches_the_log_and_the_run_summary_is_written() {
    let dir = std::env::temp_dir().join(format!("fork-hitch-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("run.jsonl");
    std::env::set_var("NV_LOG_JSON", &log);
    std::env::set_var("NV_HITCH_SUMMARY_SECS", "0");
    fork_log::init();
    fork_hitch::init();
    fork_hitch::set_meta("gpu", "test gpu");
    fork_hitch::peak("rss_mb", 1234);
    let ctx = Context {
        cell: 0x1658b7,
        world: 0xda726,
        pos: [1.0, 2.0, 3.0],
        screen: Screen::Play,
    };
    fork_hitch::frame_end(&ctx);
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(2));
        fork_hitch::add(Counter::Draws, 100);
        fork_hitch::frame_end(&ctx);
    }
    fork_hitch::event("cell.change", 7);
    {
        let _s = fork_hitch::span(Counter::FileNs, None);
        std::thread::sleep(Duration::from_millis(90));
    }
    fork_hitch::frame_end(&ctx);
    fork_log::flush();
    let text = std::fs::read_to_string(&log).unwrap();
    let hitch = text
        .lines()
        .find(|l| l.contains("\"kind\":\"hitch\""))
        .unwrap_or_else(|| panic!("no hitch record in {text}"));
    for part in [
        "\"cause\":\"file_io\"",
        "\"trigger\":\"cell.change",
        "\"cell\":\"001658b7\"",
        "\"before\":[",
    ] {
        assert!(hitch.contains(part), "{part} in {hitch}");
    }
    fork_hitch::write_summary("exit");
    let summary = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("run-summary-"))
        .expect("a run summary next to the log");
    let s = std::fs::read_to_string(summary.path()).unwrap();
    for part in [
        "\"schema\":\"nv-rs-run-summary/1\"",
        "\"ended\":\"exit\"",
        "\"gpu\":\"test gpu\"",
        "\"rss_mb\":1234",
        "\"count\":1",
        "\"file_io\"",
        "run.jsonl",
    ] {
        assert!(s.contains(part), "{part} in {s}");
    }
    let csv = std::fs::read_to_string(dir.join("run.hitches.csv")).unwrap();
    assert!(csv.starts_with("unix_ms,run_ms,frame,wall_ms,cause"));
    assert!(csv.contains(",file_io"), "{csv}");
    let _ = std::fs::remove_dir_all(&dir);
}
