// SPDX-License-Identifier: GPL-3.0-only
//! The values the settings file holds, and how one is read from and written as TOML text.
//!
//! Only three kinds are read: booleans, numbers (TOML integers and floats) and strings (basic and
//! literal, one line or several). Anything else (arrays, inline tables, dates) is not a value of
//! this file; [`parse`] returns `None` for it and the file keeps the text as it is.

/// A value of the settings file.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    /// The value as TOML text: `true`, `0.7`, `1` (a whole number is written as a TOML integer, which
    /// a reader that wants a float accepts too), `"high"` (escaped).
    pub fn to_toml(&self) -> String {
        match self {
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => number_text(*n),
            Value::Text(s) => quote(s),
        }
    }

    /// A short name of the kind, for messages: `true or false`, `a number`, `a string`.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "true or false",
            Value::Number(_) => "a number",
            Value::Text(_) => "a string",
        }
    }
}

/// A number as TOML text. Not finite numbers are written as TOML's `inf`, `-inf` and `nan`.
pub fn number_text(n: f64) -> String {
    if n.is_nan() {
        return "nan".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        // Rust's shortest round-trip form; it never uses an exponent TOML cannot read.
        let s = format!("{n}");
        if s.contains(['.', 'e', 'E']) {
            s
        } else {
            format!("{s}.0")
        }
    }
}

/// `s` as a TOML basic string.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Reads the value that starts `text` (the text after `=` with leading spaces taken off). Returns the
/// value, if it is one of this file's kinds, and how many bytes the value's text takes. `Err` when the
/// text is not a complete TOML value at all (for example an unclosed string).
///
/// The length is returned for every TOML value, also those that are not this file's kinds, so the
/// caller can find the end of the line's value and the comment after it.
pub fn parse(text: &str) -> Result<(Option<Value>, usize), String> {
    let bytes = text.as_bytes();
    match bytes.first() {
        None => Err("no value after `=`".to_string()),
        Some(b'"') if text.starts_with("\"\"\"") => {
            let (s, len) = multi_line_basic(text)?;
            Ok((Some(Value::Text(s)), len))
        }
        Some(b'\'') if text.starts_with("'''") => {
            let end = text[3..]
                .find("'''")
                .ok_or("a ''' string that does not end")?;
            // Up to two more quotes right before the closing three belong to the string.
            let mut end = end + 3;
            while text[end + 3..].starts_with('\'') && end + 3 < text.len() {
                end += 1;
            }
            let body = &text[3..end];
            let body = body
                .strip_prefix('\n')
                .or_else(|| body.strip_prefix("\r\n"))
                .unwrap_or(body);
            Ok((Some(Value::Text(body.to_string())), end + 3))
        }
        Some(b'"') => {
            let (s, len) = basic(text)?;
            Ok((Some(Value::Text(s)), len))
        }
        Some(b'\'') => {
            let end = text[1..]
                .find(['\'', '\n'])
                .filter(|&i| text.as_bytes()[1 + i] == b'\'')
                .ok_or("a ' string that does not end on its line")?;
            Ok((Some(Value::Text(text[1..1 + end].to_string())), end + 2))
        }
        Some(b'[') | Some(b'{') => Ok((None, bracketed(text)?)),
        Some(_) => {
            // A bare word: a boolean, a number or a date; it ends at a space, a comment or the line's end.
            let mut len = text
                .find(|c: char| c.is_whitespace() || c == '#' || c == ',' || c == ']' || c == '}')
                .unwrap_or(text.len());
            // A date and a time with a space between them is one value.
            let after = &text.as_bytes()[len..];
            if looks_like_date(&text[..len])
                && after.len() >= 4
                && after[0] == b' '
                && after[1].is_ascii_digit()
                && after[2].is_ascii_digit()
                && after[3] == b':'
            {
                len += 1 + text[len + 1..]
                    .find(|c: char| {
                        c.is_whitespace() || c == '#' || c == ',' || c == ']' || c == '}'
                    })
                    .unwrap_or(text.len() - len - 1);
            }
            let word = &text[..len];
            let value = match word {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                w => number(w).map(Value::Number),
            };
            if value.is_none() && !looks_like_date(word) {
                return Err(format!("`{word}` is not a TOML value"));
            }
            Ok((value, word.len()))
        }
    }
}

/// A TOML integer or float: `7`, `-3`, `1_000`, `0x1f`, `0o17`, `0b101`, `0.7`, `1e-3`, `inf`, `nan`.
pub fn number(word: &str) -> Option<f64> {
    let w = word.replace('_', "");
    let (sign, digits) = match w.as_bytes().first() {
        Some(b'+') => (1.0, &w[1..]),
        Some(b'-') => (-1.0, &w[1..]),
        _ => (1.0, &w[..]),
    };
    for (prefix, radix) in [("0x", 16), ("0o", 8), ("0b", 2)] {
        if let Some(rest) = digits.strip_prefix(prefix) {
            return i64::from_str_radix(rest, radix)
                .ok()
                .map(|v| sign * v as f64);
        }
    }
    match digits {
        "inf" => return Some(sign * f64::INFINITY),
        "nan" => return Some(f64::NAN),
        _ => {}
    }
    // Rust also reads `infinity` and `NaN`; TOML does not.
    if digits.is_empty()
        || !digits
            .bytes()
            .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
    {
        return None;
    }
    if !digits.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    digits.parse::<f64>().ok().map(|v| sign * v)
}

fn looks_like_date(word: &str) -> bool {
    let b = word.as_bytes();
    (b.len() >= 8 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-')
        || (b.len() >= 5 && b[..2].iter().all(u8::is_ascii_digit) && b[2] == b':')
}

/// A basic string on one line: the text and its length with both quotes.
fn basic(text: &str) -> Result<(String, usize), String> {
    let mut out = String::new();
    let mut chars = text.char_indices().skip(1);
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok((out, i + 1)),
            '\n' => break,
            '\\' => escape(&mut chars, &mut out)?,
            c => out.push(c),
        }
    }
    Err("a \" string that does not end on its line".to_string())
}

/// A basic string of three quotes, over lines.
fn multi_line_basic(text: &str) -> Result<(String, usize), String> {
    let mut out = String::new();
    let mut chars = text.char_indices().skip(3).peekable();
    // A newline right after the opening quotes is not part of the string.
    if text[3..].starts_with("\r\n") {
        chars.next();
        chars.next();
    } else if text[3..].starts_with('\n') {
        chars.next();
    }
    while let Some((i, c)) = chars.next() {
        match c {
            '"' if text[i..].starts_with("\"\"\"") => {
                // Up to two more quotes before the closing three belong to the string.
                let mut end = i;
                while text[end + 3..].starts_with('"') && end + 3 < text.len() {
                    out.push('"');
                    end += 1;
                }
                return Ok((out, end + 3));
            }
            '\\' => {
                // A backslash at a line's end joins the lines, dropping the spaces after it.
                let rest = &text[i + 1..];
                let trimmed = rest.trim_start_matches([' ', '\t']);
                if trimmed.starts_with('\n') || trimmed.starts_with("\r\n") {
                    let skip = rest.len() - rest.trim_start().len();
                    let target = i + 1 + skip;
                    while chars.peek().is_some_and(|&(j, _)| j < target) {
                        chars.next();
                    }
                } else {
                    escape(&mut chars, &mut out)?;
                }
            }
            c => out.push(c),
        }
    }
    Err("a \"\"\" string that does not end".to_string())
}

fn escape(chars: &mut impl Iterator<Item = (usize, char)>, out: &mut String) -> Result<(), String> {
    let (_, e) = chars.next().ok_or("a string that ends in \\")?;
    match e {
        'n' => out.push('\n'),
        't' => out.push('\t'),
        'r' => out.push('\r'),
        'b' => out.push('\u{8}'),
        'f' => out.push('\u{c}'),
        'e' => out.push('\u{1b}'),
        '"' => out.push('"'),
        '\\' => out.push('\\'),
        'u' | 'U' => {
            let n = if e == 'u' { 4 } else { 8 };
            let hex: String = chars.take(n).map(|(_, c)| c).collect();
            let code = u32::from_str_radix(&hex, 16)
                .map_err(|_| format!("\\{e}{hex} is not an escape"))?;
            out.push(char::from_u32(code).ok_or(format!("\\{e}{hex} is not a character"))?);
        }
        other => return Err(format!("\\{other} is not an escape")),
    }
    Ok(())
}

/// The length of an array or inline table, brackets matched and strings and comments skipped.
fn bracketed(text: &str) -> Result<usize, String> {
    let mut depth = 0usize;
    let mut i = 0;
    let b = text.as_bytes();
    while i < b.len() {
        match b[i] {
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i + 1);
                }
            }
            b'"' | b'\'' => {
                let (_, len) = parse(&text[i..])?;
                i += len;
                continue;
            }
            b'#' => {
                i += text[i..].find('\n').unwrap_or(text.len() - i);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Err("an array or table that does not end".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Option<Value> {
        parse(text).unwrap().0
    }

    #[test]
    fn reads_the_three_kinds() {
        assert_eq!(value("true"), Some(Value::Bool(true)));
        assert_eq!(value("false # off"), Some(Value::Bool(false)));
        assert_eq!(value("0.7"), Some(Value::Number(0.7)));
        assert_eq!(value("-3"), Some(Value::Number(-3.0)));
        assert_eq!(value("1_000"), Some(Value::Number(1000.0)));
        assert_eq!(value("0x1f"), Some(Value::Number(31.0)));
        assert_eq!(value("1e-3"), Some(Value::Number(0.001)));
        assert_eq!(value("\"high\""), Some(Value::Text("high".into())));
        assert_eq!(value("'C:\\path'"), Some(Value::Text("C:\\path".into())));
        assert_eq!(
            value("\"a\\\"b\\u00e9\""),
            Some(Value::Text("a\"b\u{e9}".into()))
        );
    }

    #[test]
    fn measures_values_it_does_not_use() {
        assert_eq!(parse("[1, [2, 3], \"]\"] # x").unwrap(), (None, 16));
        assert_eq!(parse("{ a = 1 }").unwrap(), (None, 9));
        assert_eq!(parse("1979-05-27T07:32:00Z").unwrap(), (None, 20));
        assert_eq!(parse("1979-05-27 07:32:00 # x").unwrap(), (None, 19));
        assert_eq!(parse("2 junk").unwrap(), (Some(Value::Number(2.0)), 1));
        assert_eq!(
            parse("\"\"\"\nab\nc\"\"\"").unwrap(),
            (Some(Value::Text("ab\nc".into())), 11)
        );
        assert_eq!(
            parse("'''x'''").unwrap(),
            (Some(Value::Text("x".into())), 7)
        );
    }

    #[test]
    fn refuses_what_is_no_value() {
        assert!(parse("\"open").is_err());
        assert!(parse("[1, 2").is_err());
        assert!(parse("high").is_err());
        assert!(parse("").is_err());
        assert!(number("infinity").is_none());
        assert!(number(".5").is_none());
    }

    #[test]
    fn writes_what_it_reads() {
        for v in [
            Value::Bool(true),
            Value::Number(0.7),
            Value::Number(1.0),
            Value::Number(-12.5),
            Value::Number(1e20),
            Value::Text("a \"q\" \\ \n\t é".into()),
        ] {
            let text = v.to_toml();
            assert_eq!(value(&text), Some(v.clone()), "{text}");
        }
        assert_eq!(Value::Number(1.0).to_toml(), "1");
        assert_eq!(Value::Number(-0.0).to_toml(), "-0");
        assert_eq!(Value::Number(0.25).to_toml(), "0.25");
    }
}
