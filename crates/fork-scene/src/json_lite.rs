// SPDX-License-Identifier: GPL-3.0-only
//! Reads a few string fields of one line of the engine's JSON log (`{"level":"info",...,"message":"..."}`):
//! top-level keys with string values only, escapes `\"`, `\\`, `\n`, `\t` and `\uXXXX` understood. Not a
//! general JSON reader: the log's lines are flat enough for it, and the crate takes no dependency for it.

/// The values of `keys` (in that order) from `line`; an error when the line is not an object or lacks one.
pub fn fields(line: &str, keys: &[&str]) -> Result<Vec<String>, String> {
    keys.iter()
        .map(|k| field(line, k).ok_or_else(|| format!("no \"{k}\" in the line")))
        .collect()
}

fn field(line: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = line.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_level_and_message_with_escapes() {
        let l = r#"{"ts":"x","level":"error","target":"stop","message":"Game's main \"stopped\" in frame 5!","frame":5}"#;
        let v = fields(l, &["level", "message"]).unwrap();
        assert_eq!(v[0], "error");
        assert_eq!(v[1], "Game's main \"stopped\" in frame 5!");
    }

    #[test]
    fn a_missing_key_is_an_error() {
        assert!(fields("{\"a\":\"b\"}", &["level"]).is_err());
        assert!(fields("not json", &["level"]).is_err());
    }
}
