//! The JSON-lines file: where it goes, how it is bounded, how a record is written.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where `NV_LOG_JSON` says the file goes.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Off,
    Auto,
    Path(PathBuf),
}

impl Target {
    /// `off`, `0`, `none` and `false` switch the file off; `auto` (the default) and the empty
    /// value choose a file in the state folder; anything else is the path.
    pub fn parse(value: Option<&str>) -> Target {
        match value.map(str::trim) {
            None | Some("") => Target::Auto,
            Some(v) if v.eq_ignore_ascii_case("auto") => Target::Auto,
            Some(v)
                if ["off", "0", "none", "false"]
                    .iter()
                    .any(|o| v.eq_ignore_ascii_case(o)) =>
            {
                Target::Off
            }
            Some(v) => Target::Path(PathBuf::from(v)),
        }
    }
}

/// The folder of automatic logs: `$XDG_STATE_HOME/nv-rs/logs`, else `~/.local/state/nv-rs/logs`;
/// on Windows `%LOCALAPPDATA%\nv-rs\logs`.
pub fn state_dir(var: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if cfg!(windows) {
        let base = var("LOCALAPPDATA").filter(|v| !v.is_empty())?;
        return Some(PathBuf::from(base).join("nv-rs").join("logs"));
    }
    if let Some(x) = var("XDG_STATE_HOME").filter(|v| v.starts_with('/')) {
        return Some(PathBuf::from(x).join("nv-rs").join("logs"));
    }
    let home = var("HOME").filter(|v| !v.is_empty())?;
    Some(PathBuf::from(home).join(".local/state/nv-rs/logs"))
}

/// An open log file with a size bound: past `max` bytes the file becomes `<path>.1` (the one
/// before it is dropped) and a new one starts.
pub struct JsonFile {
    path: PathBuf,
    file: File,
    written: u64,
    max: u64,
    last_flush: std::time::Instant,
    buf: Vec<u8>,
}

impl JsonFile {
    pub fn open(path: &Path, max: u64) -> std::io::Result<JsonFile> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(JsonFile {
            path: path.to_path_buf(),
            file,
            written,
            max,
            last_flush: std::time::Instant::now(),
            buf: Vec::with_capacity(16 * 1024),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes one line; `urgent` (a warning, an error, a stop) flushes at once, other lines
    /// within a second.
    pub fn line(&mut self, line: &str, urgent: bool) {
        self.buf.extend_from_slice(line.as_bytes());
        self.buf.push(b'\n');
        self.written += line.len() as u64 + 1;
        if urgent || self.buf.len() >= 16 * 1024 || self.last_flush.elapsed().as_millis() >= 1000 {
            self.flush();
        }
        if self.written >= self.max {
            self.rotate();
        }
    }

    pub fn flush(&mut self) {
        if !self.buf.is_empty() {
            let _ = self.file.write_all(&self.buf);
            self.buf.clear();
        }
        let _ = self.file.flush();
        self.last_flush = std::time::Instant::now();
    }

    fn rotate(&mut self) {
        self.flush();
        let mut old = self.path.clone().into_os_string();
        old.push(".1");
        let _ = std::fs::rename(&self.path, &old);
        if let Ok(f) = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
        {
            self.file = f;
            self.written = 0;
        }
    }
}

/// Removes all but the newest `keep` automatic logs (`nv-rs-*.jsonl*`) of `dir`.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = read
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("nv-rs-") && n.contains(".jsonl")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, p) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(p);
    }
}

/// Appends `s` as a JSON string (with the quotes).
pub fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `now` as `2026-11-16T12:00:00.123Z` (UTC).
pub fn timestamp(now: SystemTime) -> String {
    let d = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn target_values() {
        assert_eq!(Target::parse(None), Target::Auto);
        assert_eq!(Target::parse(Some("OFF")), Target::Off);
        assert_eq!(Target::parse(Some("auto")), Target::Auto);
        assert_eq!(
            Target::parse(Some("/x/y.jsonl")),
            Target::Path("/x/y.jsonl".into())
        );
    }

    #[test]
    fn state_dirs() {
        let env = |k: &str| match k {
            "HOME" => Some("/home/u".to_string()),
            _ => None,
        };
        if !cfg!(windows) {
            assert_eq!(
                state_dir(&env).unwrap(),
                PathBuf::from("/home/u/.local/state/nv-rs/logs")
            );
            let xdg = |k: &str| (k == "XDG_STATE_HOME").then(|| "/s".to_string());
            assert_eq!(state_dir(&xdg).unwrap(), PathBuf::from("/s/nv-rs/logs"));
        }
    }

    #[test]
    fn escapes_and_dates() {
        let mut s = String::new();
        json_str(&mut s, "a\"b\\c\nd\u{1}");
        assert_eq!(s, "\"a\\\"b\\\\c\\nd\\u0001\"");
        let t = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
        assert_eq!(timestamp(t), "2023-11-14T22:13:20.123Z");
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn file_rotates_at_its_bound() {
        let dir = std::env::temp_dir().join(format!("fork-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("a.jsonl");
        let mut f = JsonFile::open(&path, 64).unwrap();
        for _ in 0..10 {
            f.line("{\"k\":\"0123456789\"}", true);
        }
        assert!(dir.join("a.jsonl.1").exists());
        assert!(std::fs::metadata(&path).unwrap().len() < 64 + 20);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
