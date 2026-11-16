//! The terminal policy end to end: a child process (this test binary, run again) logs a fixed
//! set of messages and the parent reads what reached its stderr and the two files.

use std::process::Command;

/// The child: run only when asked by the parent.
#[test]
fn child_probe() {
    if std::env::var_os("FORK_LOG_PROBE").is_none() {
        return;
    }
    fork_log::init();
    fork_log::fact("version", "9.9.9");
    fork_log::fact("gpu", "Test GPU");
    fork_log::info!(target: "d3d9", "a one-time fact");
    fork_log::debug!(target: "d3d9", "per present stats {}", 1);
    for i in 0..10 {
        fork_log::warn!(target: "window", "the surface was lost ({i})");
    }
    fork_log::warn!(target: "sound", "a different warning");
    fork_log::fatal!(target: "window", "no window can be made");
    fork_log::print_header();
    fork_log::final_summary(&["OK: 143 fps, p99 7.0 ms, 0 hitches".to_string()]);
    fork_log::finish();
}

fn probe(term: Option<&str>) -> (String, String, String) {
    let dir = std::env::temp_dir().join(format!(
        "fork-log-term-{}-{}",
        std::process::id(),
        term.unwrap_or("default")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let json = dir.join("run.jsonl");
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["child_probe", "--exact", "--nocapture", "--test-threads=1"])
        .env("FORK_LOG_PROBE", "1")
        .env("NV_LOG_JSON", &json)
        .env_remove("NV_LOG")
        .env_remove("RUST_LOG")
        .env_remove("NV_LOG_TERM")
        .env_remove("NV_LOG_JSON_FILTER");
    if let Some(t) = term {
        cmd.env("NV_LOG_TERM", t);
    }
    let out = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let jsonl = std::fs::read_to_string(&json).unwrap_or_default();
    let text = std::fs::read_to_string(dir.join("run.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    (stderr, jsonl, text)
}

#[test]
fn quiet_shows_the_header_limited_warnings_the_fatal_and_the_summary() {
    let (err, jsonl, text) = probe(None);
    assert!(err.contains("nv-rs 9.9.9"), "{err}");
    assert!(err.contains("gpu: Test GPU"), "{err}");
    assert_eq!(err.matches("the surface was lost").count(), 4, "{err}");
    assert!(err.contains("(x repeated 7 times)"), "{err}");
    assert!(err.contains("warning: a different warning"), "{err}");
    assert!(err.contains("error: no window can be made"), "{err}");
    assert!(err.contains("OK: 143 fps"), "{err}");
    assert!(!err.contains("a one-time fact"), "{err}");
    assert!(!err.contains("per present stats"), "{err}");
    // Nothing is lost: the files hold every line, the repeats included.
    assert_eq!(
        jsonl.matches("\"message\":\"the surface was lost").count(),
        10
    );
    assert!(jsonl.contains("per present stats 1"));
    assert_eq!(text.matches("the surface was lost").count(), 10);
    assert!(
        text.contains("DEBUG d3d9 f-: per present stats 1"),
        "{text}"
    );
    assert!(text.contains("INFO  d3d9 f-: a one-time fact"));
}

#[test]
fn verbose_shows_debug_and_every_repeat() {
    let (err, _, _) = probe(Some("verbose"));
    assert!(err.contains("per present stats 1"), "{err}");
    assert_eq!(err.matches("the surface was lost").count(), 10, "{err}");
    assert!(!err.contains("repeated"), "{err}");
}

#[test]
fn off_still_prints_the_fatal_failure() {
    let (err, jsonl, _) = probe(Some("off"));
    assert!(err.contains("error: no window can be made"), "{err}");
    assert!(!err.contains("surface was lost"), "{err}");
    assert!(jsonl.contains("no window can be made"));
}

#[test]
fn a_filter_in_nv_log_term_picks_what_shows() {
    let (err, _, _) = probe(Some("error,d3d9=debug"));
    assert!(err.contains("per present stats 1"), "{err}");
    assert!(!err.contains("surface was lost"), "{err}");
}
