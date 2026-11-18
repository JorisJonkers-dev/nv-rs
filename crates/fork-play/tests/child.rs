//! `play_main` never returns, so it is tested as a child process: this test program (no harness)
//! runs it when asked to and otherwise starts itself that way and checks the exit codes. No game
//! data is used; the folders are empty temporary ones.

use std::path::PathBuf;
use std::process::{Command, Output};

const AS_CHILD: &str = "FORK_PLAY_TEST_AS_CHILD";

fn main() {
    if std::env::var_os(AS_CHILD).is_some() {
        fork_play::play_main();
    }
    let dir = std::env::temp_dir().join(format!("fork-play-child-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temporary folder");
    let empty = dir.join("empty");
    std::fs::create_dir_all(&empty).expect("empty folder");

    // --help is a normal end of the viewer's code: status 0 and the usage on stdout.
    let out = child(&dir, &["--play-mode", "--help"]);
    check("help ends with 0", out.status.code() == Some(0), &out);
    check(
        "help prints the usage",
        String::from_utf8_lossy(&out.stdout).contains("USAGE"),
        &out,
    );

    // The reserved word alone: no data folder is an argument error.
    let out = child(&dir, &["--play-mode"]);
    check(
        "no data folder ends with 2",
        out.status.code() == Some(2),
        &out,
    );

    // A folder without the game: a start-up error, never a crash.
    let out = child_os(&[
        "--play-mode".as_ref(),
        empty.as_os_str(),
        "--check".as_ref(),
    ]);
    check(
        "a folder without the game ends with 1",
        out.status.code() == Some(1),
        &out,
    );

    // The reserved word may follow the data folder and the stop file variable is accepted.
    let out = Command::new(exe())
        .env(AS_CHILD, "1")
        .env("NV_STOP_FILE", dir.join("stop"))
        .env("NV_LOG_JSON", "off")
        .env("NV_LOG_TEXT", "off")
        .args([
            empty.as_os_str(),
            "--play-mode".as_ref(),
            "--check".as_ref(),
        ])
        .output()
        .expect("start the child");
    check(
        "the stop file variable does not change an end",
        out.status.code() == Some(1),
        &out,
    );

    std::fs::remove_dir_all(&dir).ok();
    println!("fork-play child tests: ok");
}

fn exe() -> PathBuf {
    std::env::current_exe().expect("this program")
}

fn child_os(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(exe())
        .env(AS_CHILD, "1")
        .env("NV_LOG_JSON", "off")
        .env("NV_LOG_TEXT", "off")
        .env_remove("NV_STOP_FILE")
        .args(args)
        .output()
        .expect("start the child")
}

fn child(_dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(exe())
        .env(AS_CHILD, "1")
        .env("NV_LOG_JSON", "off")
        .env("NV_LOG_TEXT", "off")
        .env_remove("NV_STOP_FILE")
        .args(args)
        .output()
        .expect("start the child")
}

fn check(what: &str, ok: bool, out: &Output) {
    if !ok {
        eprintln!(
            "FAILED: {what}: status {:?}\nstdout: {}\nstderr: {}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        std::process::exit(1);
    }
}
