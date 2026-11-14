// Fork-only. The one gate for tests that need the player's game files (docs/fork/TESTING.md).
//
// A test crate pulls this file in with `include!` from a `#[cfg(test)]` module (no manifest changes, no
// dependency; upstream's Cargo.toml stays untouched):
//
//     #[cfg(test)]
//     #[allow(dead_code)]
//     pub(crate) mod test_gate {
//         include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/fork/test_gate.rs"));
//     }
//
// and a test that needs, say, FalloutNV.exe starts with
//
//     let Some(exe) = crate::test_gate::require_data("NV_EXE") else { return };
//
// Without the variable the test prints `skipped: <test name> (NV_EXE not set)` and returns. With
// `NV_ORACLE_REQUIRE=1` it panics instead, so a private job that must run the test cannot pass by skipping it.
// With the variable it prints `executed: <test name> (NV_EXE set)`. The path is never printed. The line is read
// by scripts/fork/gate_counts.py (run the tests with `-- --nocapture` or `--show-output`).
// scripts/fork/check_data_gates.py refuses any other read of NV_EXE, NV_DATA or NV_ORACLE_EXE.

/// What a gate decides for one variable: the path, `None` to skip, or the reason to fail.
fn decide(
    value: Option<std::ffi::OsString>,
    require: bool,
    variable: &str,
    test: &str,
) -> Result<Option<std::path::PathBuf>, String> {
    match value.filter(|v| !v.is_empty()) {
        Some(v) => Ok(Some(v.into())),
        None if require => Err(format!(
            "{test}: {variable} is not set and NV_ORACLE_REQUIRE=1 does not allow skipping it"
        )),
        None => Ok(None),
    }
}

/// The value of `variable` as a path, or `None` (printing `skipped: <test>`) when it is unset or empty.
/// Panics instead of skipping when `NV_ORACLE_REQUIRE=1`.
pub fn require_data(variable: &str) -> Option<std::path::PathBuf> {
    let current = std::thread::current();
    // libtest names the thread of each test after it (not with `--test-threads=1`, which runs on `main`).
    let test = current.name().unwrap_or("(unnamed)").to_owned();
    let require = std::env::var_os("NV_ORACLE_REQUIRE").is_some_and(|v| v == "1");
    match decide(std::env::var_os(variable), require, variable, &test) {
        Ok(Some(path)) => {
            println!("executed: {test} ({variable} set)");
            Some(path)
        }
        Ok(None) => {
            println!("skipped: {test} ({variable} not set)");
            None
        }
        Err(reason) => panic!("{reason}"),
    }
}

#[cfg(test)]
mod gate_tests {
    use super::decide;
    use std::ffi::OsString;

    #[test]
    fn a_set_variable_gives_its_path() {
        let got = decide(Some(OsString::from("/x/FalloutNV.exe")), true, "NV_EXE", "t");
        assert_eq!(got, Ok(Some("/x/FalloutNV.exe".into())));
    }

    #[test]
    fn an_unset_variable_skips_unless_required() {
        assert_eq!(decide(None, false, "NV_EXE", "t"), Ok(None));
        assert_eq!(decide(Some(OsString::new()), false, "NV_EXE", "t"), Ok(None));
    }

    #[test]
    fn an_unset_variable_fails_when_required() {
        let err = decide(None, true, "NV_EXE", "some::test").unwrap_err();
        for part in ["some::test", "NV_EXE", "NV_ORACLE_REQUIRE"] {
            assert!(err.contains(part), "{err}");
        }
        assert!(decide(Some(OsString::new()), true, "NV_EXE", "t").is_err());
    }
}
