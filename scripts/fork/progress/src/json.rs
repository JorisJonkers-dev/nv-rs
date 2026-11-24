// SPDX-License-Identifier: GPL-3.0-only
//! A small JSON reader and writer: enough for `.fork/capabilities.json` and the summary this tool
//! writes. Objects keep their keys in the order given, so a writer that inserts keys in a fixed
//! order writes the same bytes every time.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// The number as written (never reformatted).
    Num(String),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_arr(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Num(n) => n.parse().ok(),
            _ => None,
        }
    }
    pub fn num(n: impl Into<u64>) -> Value {
        Value::Num(n.into().to_string())
    }
    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }
    pub fn strs<S: AsRef<str>>(items: &[S]) -> Value {
        Value::Arr(items.iter().map(|s| Value::str(s.as_ref())).collect())
    }
}

/// An object from (key, value) pairs, in that order.
pub fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    let v = p.value()?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("JSON: trailing text at byte {}", p.i));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("JSON: {what} at byte {}", self.i))
    }
    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            self.err(&format!("expected '{}'", c as char))
        }
    }
    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut m = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.eat(b':')?;
                    m.push((k, self.value()?));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return self.err("expected ',' or '}'"),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return self.err("expected ',' or ']'"),
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') if self.b[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Value::Bool(true))
            }
            Some(b'f') if self.b[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Value::Bool(false))
            }
            Some(b'n') if self.b[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Value::Null)
            }
            Some(c) if *c == b'-' || c.is_ascii_digit() => {
                let s = self.i;
                while self.i < self.b.len()
                    && matches!(
                        self.b[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                Ok(Value::Num(
                    String::from_utf8_lossy(&self.b[s..self.i]).into_owned(),
                ))
            }
            _ => self.err("expected a value"),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        if self.b.get(self.i) != Some(&b'"') {
            return self.err("expected a string");
        }
        self.i += 1;
        let mut out = Vec::new();
        while let Some(&c) = self.b.get(self.i) {
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else {
                        break;
                    };
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self.b.get(self.i..self.i + 4).ok_or("JSON: short \\u")?;
                            let cp = u32::from_str_radix(&String::from_utf8_lossy(hex), 16)
                                .map_err(|e| e.to_string())?;
                            self.i += 4;
                            let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                            let mut buf = [0; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
        self.err("unterminated string")
    }
}

/// The value as indented JSON (two spaces), ending in a newline.
pub fn write(v: &Value) -> String {
    let mut s = String::new();
    put(v, 0, &mut s);
    s.push('\n');
    s
}

fn put(v: &Value, depth: usize, s: &mut String) {
    let pad = |s: &mut String, d: usize| s.extend(std::iter::repeat_n(' ', d * 2));
    match v {
        Value::Null => s.push_str("null"),
        Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) => s.push_str(n),
        Value::Str(t) => quote(t, s),
        Value::Arr(a) if a.is_empty() => s.push_str("[]"),
        // Lists of plain strings or numbers stay on one line each item, compact enough to diff.
        Value::Arr(a) => {
            s.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                pad(s, depth + 1);
                put(x, depth + 1, s);
                if i + 1 < a.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            pad(s, depth);
            s.push(']');
        }
        Value::Obj(m) if m.is_empty() => s.push_str("{}"),
        Value::Obj(m) => {
            s.push_str("{\n");
            for (i, (k, x)) in m.iter().enumerate() {
                pad(s, depth + 1);
                quote(k, s);
                s.push_str(": ");
                put(x, depth + 1, s);
                if i + 1 < m.len() {
                    s.push(',');
                }
                s.push('\n');
            }
            pad(s, depth);
            s.push('}');
        }
    }
}

fn quote(t: &str, s: &mut String) {
    s.push('"');
    for c in t.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\t' => s.push_str("\\t"),
            '\r' => s.push_str("\\r"),
            c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_it_writes() {
        let v = obj([
            ("a", Value::num(3u64)),
            ("b", Value::strs(&["x\"y", "z"])),
            ("c", Value::Arr(vec![])),
            ("d", obj([("e", Value::Bool(true)), ("f", Value::Null)])),
        ]);
        let text = write(&v);
        assert_eq!(parse(&text).unwrap(), v);
        assert_eq!(write(&parse(&text).unwrap()), text);
    }

    #[test]
    fn reads_escapes_and_numbers() {
        let v = parse(r#"{"s": "aé\n", "n": -1.5e3, "k": [1, 2]}"#).unwrap();
        assert_eq!(v.get("s").unwrap().as_str(), Some("aé\n"));
        assert_eq!(v.get("n"), Some(&Value::Num("-1.5e3".into())));
        assert_eq!(v.get("k").unwrap().as_arr().len(), 2);
        assert!(parse("{\"a\": 1,}").is_err());
    }
}
