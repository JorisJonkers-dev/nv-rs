// SPDX-License-Identifier: GPL-3.0-only
//! Just enough of a C and C++ reader to list the calls an extender's registration code makes:
//! comments removed, string literals kept, the branches of `#if` the build leaves out blanked,
//! and the opcode comment (`/*2200*/`) that starts a line kept aside. Never a compiler: what it
//! cannot decide (an `#if` it cannot read) counts as not built, and the counts' own checks
//! (`crate::extenders`) catch a reading that went wrong.

/// One line of a source file after [`read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// From 1.
    pub no: usize,
    /// The code: comments as spaces, a line the build leaves out (or a directive) empty.
    pub code: String,
    /// The hexadecimal number of a `/*XXXX*/` comment the line starts with, if any.
    pub opcode: Option<u32>,
}

/// `text` read with `defined` the macros the build sets (every other macro is 0).
pub fn read(text: &str, defined: &[&str]) -> Vec<Line> {
    let (code, opcodes) = strip_comments(text);
    let mut out = Vec::new();
    // Each level: (this branch is on, some branch of the group was on, the enclosing level is on).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let on = |stack: &Vec<(bool, bool, bool)>| stack.last().map_or(true, |l| l.0 && l.2);
    for (i, line) in code.lines().enumerate() {
        let t = line.trim_start();
        let mut keep = String::new();
        if let Some(d) = t.strip_prefix('#') {
            let d = d.trim_start();
            let word: String = d.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
            let rest = d[word.len()..].trim();
            let outer = on(&stack);
            match word.as_str() {
                "if" => {
                    let v = holds(rest, defined);
                    stack.push((v, v, outer));
                }
                "ifdef" => {
                    let v = defined.contains(&ident(rest).as_str());
                    stack.push((v, v, outer));
                }
                "ifndef" => {
                    let v = !defined.contains(&ident(rest).as_str());
                    stack.push((v, v, outer));
                }
                "elif" => {
                    if let Some(l) = stack.last_mut() {
                        let v = !l.1 && holds(rest, defined);
                        l.0 = v;
                        l.1 |= v;
                    }
                }
                "else" => {
                    if let Some(l) = stack.last_mut() {
                        l.0 = !l.1;
                        l.1 = true;
                    }
                }
                "endif" => {
                    stack.pop();
                }
                _ => {}
            }
        } else if on(&stack) {
            keep = line.to_string();
        }
        out.push(Line {
            no: i + 1,
            code: keep,
            opcode: opcodes.get(i).copied().flatten(),
        });
    }
    out
}

fn ident(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Comments replaced by spaces (newlines kept), and per line the opcode comment it starts with.
fn strip_comments(text: &str) -> (String, Vec<Option<u32>>) {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut opcodes = vec![None];
    let mut line_start = true;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && b.get(i + 1) == Some(&'*') {
            let start = i + 2;
            let mut j = start;
            while j + 1 < b.len() && !(b[j] == '*' && b[j + 1] == '/') {
                j += 1;
            }
            let body: String = b[start..j.min(b.len())].iter().collect();
            if line_start && body.len() == 4 && body.chars().all(|c| c.is_ascii_hexdigit()) {
                *opcodes.last_mut().unwrap() = u32::from_str_radix(&body, 16).ok();
            }
            for &ch in &b[i..(j + 2).min(b.len())] {
                if ch == '\n' {
                    out.push('\n');
                    opcodes.push(None);
                }
            }
            out.push(' ');
            i = j + 2;
            continue;
        }
        if c == '"' || c == '\'' {
            out.push(c);
            i += 1;
            while i < b.len() && b[i] != c && b[i] != '\n' {
                if b[i] == '\\' && i + 1 < b.len() {
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b[i]);
                i += 1;
            }
            if i < b.len() && b[i] == c {
                out.push(c);
                i += 1;
            }
            line_start = false;
            continue;
        }
        if c == '\n' {
            opcodes.push(None);
            line_start = true;
        } else if !c.is_whitespace() {
            line_start = false;
        }
        out.push(c);
        i += 1;
    }
    (out, opcodes)
}

/// An `#if` condition: numbers, macros (1 when in `defined`, else 0), `defined(X)`, `!`, `&&`,
/// `||` and parentheses. Anything else reads as 0.
pub fn holds(expr: &str, defined: &[&str]) -> bool {
    let toks = tokens(expr);
    let mut p = 0;
    let v = or(&toks, &mut p, defined);
    p == toks.len() && v
}

fn tokens(s: &str) -> Vec<String> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_alphanumeric() || c == '_' {
            let s = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(b[s..i].iter().collect());
        } else if (c == '&' || c == '|') && b.get(i + 1) == Some(&c) {
            out.push(format!("{c}{c}"));
            i += 2;
        } else {
            out.push(c.to_string());
            i += 1;
        }
    }
    out
}

fn or(t: &[String], p: &mut usize, d: &[&str]) -> bool {
    let mut v = and(t, p, d);
    while t.get(*p).map(String::as_str) == Some("||") {
        *p += 1;
        let r = and(t, p, d);
        v = v || r;
    }
    v
}

fn and(t: &[String], p: &mut usize, d: &[&str]) -> bool {
    let mut v = unary(t, p, d);
    while t.get(*p).map(String::as_str) == Some("&&") {
        *p += 1;
        let r = unary(t, p, d);
        v = v && r;
    }
    v
}

fn unary(t: &[String], p: &mut usize, d: &[&str]) -> bool {
    let Some(tok) = t.get(*p).cloned() else {
        return false;
    };
    *p += 1;
    match tok.as_str() {
        "!" => !unary(t, p, d),
        "(" => {
            let v = or(t, p, d);
            if t.get(*p).map(String::as_str) == Some(")") {
                *p += 1;
            } else {
                *p = usize::MAX / 2;
            }
            v
        }
        "defined" => {
            let paren = t.get(*p).map(String::as_str) == Some("(");
            if paren {
                *p += 1;
            }
            let name = t.get(*p).cloned().unwrap_or_default();
            *p += 1;
            if paren {
                *p += 1;
            }
            d.contains(&name.as_str())
        }
        n if n.chars().all(|c| c.is_ascii_digit()) => n.parse::<u64>().map_or(false, |v| v != 0),
        name if name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_') =>
        {
            d.contains(&name)
        }
        _ => {
            *p = usize::MAX / 2;
            false
        }
    }
}

/// One call on a line: `name(first, ...` with what precedes the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    /// The first argument: an identifier, or a string literal's text with the quotes off.
    pub first: String,
    /// The first argument was a string literal.
    pub quoted: bool,
    /// The character before the name (`.` for a method call), or ' '.
    pub before: char,
}

/// Every `identifier(` on a line with its first argument (an identifier or a string literal).
pub fn calls(code: &str) -> Vec<Call> {
    let b: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '"' {
            i += 1;
            while i < b.len() && b[i] != '"' {
                i += if b[i] == '\\' { 2 } else { 1 };
            }
            i += 1;
            continue;
        }
        let word_start = b[i].is_ascii_alphabetic() || b[i] == '_';
        if !word_start || (i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == '_')) {
            i += 1;
            continue;
        }
        let s = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_') {
            i += 1;
        }
        let name: String = b[s..i].iter().collect();
        let mut j = i;
        while j < b.len() && b[j] == ' ' {
            j += 1;
        }
        if b.get(j) != Some(&'(') {
            continue;
        }
        j += 1;
        while j < b.len() && b[j].is_whitespace() {
            j += 1;
        }
        let (first, quoted) = if b.get(j) == Some(&'"') {
            let a = j + 1;
            let mut k = a;
            while k < b.len() && b[k] != '"' {
                k += 1;
            }
            (b[a..k.min(b.len())].iter().collect(), true)
        } else {
            let a = j;
            let mut k = a;
            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == '_') {
                k += 1;
            }
            (b[a..k].iter().collect(), false)
        };
        out.push(Call {
            name,
            first,
            quoted,
            before: if s > 0 { b[s - 1] } else { ' ' },
        });
    }
    out
}

/// The bodies of the functions whose definition line holds `head` (the text from the line after
/// to the closing brace at the same depth), as line ranges of `lines`.
pub fn bodies(lines: &[Line], head: impl Fn(&str) -> bool) -> Vec<&[Line]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !head(&lines[i].code) {
            i += 1;
            continue;
        }
        let mut depth = 0i32;
        let mut opened = false;
        let start = i;
        let mut end = lines.len();
        'scan: for (k, l) in lines.iter().enumerate().skip(i) {
            for c in l.code.chars() {
                match c {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => {
                        depth -= 1;
                        if opened && depth == 0 {
                            end = k + 1;
                            break 'scan;
                        }
                    }
                    _ => {}
                }
            }
        }
        out.push(&lines[start..end]);
        i = end.max(i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
#define REG_CMD(name) nvse->RegisterCommand(&kCommandInfo_##name)
void Load()
{
\t/*2200*/REG_CMD(First); // a comment REG_CMD(NotThis)
#if _DEBUG
\tREG_CMD(DebugOnly);
#elif RUNTIME && !EDITOR
\tREG_CMD(Second);
#else
\tREG_CMD(Third);
#endif
\t/* REG_CMD(InAComment)
\t   REG_CMD(StillInIt) */ REG_CMD_STR(Fourth);
\tbuilder.Create(\"Fifth // not a comment\", 1);
}
";

    fn names(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .flat_map(|l| calls(&l.code))
            .filter(|c| c.name.starts_with("REG_") || c.name == "Create")
            .map(|c| c.first)
            .collect()
    }

    #[test]
    fn reads_only_the_built_branch_and_no_comment() {
        let lines = read(SRC, &["RUNTIME"]);
        assert_eq!(
            names(&lines),
            ["First", "Second", "Fourth", "Fifth // not a comment"]
        );
        assert_eq!(lines[3].opcode, Some(0x2200));
        assert_eq!(lines[0].code, "", "a directive is no code");
        let lines = read(SRC, &[]);
        assert_eq!(
            names(&lines),
            ["First", "Third", "Fourth", "Fifth // not a comment"]
        );
    }

    #[test]
    fn conditions() {
        assert!(holds("RUNTIME", &["RUNTIME"]));
        assert!(!holds("0", &[]));
        assert!(holds("defined(A) || B", &["A"]));
        assert!(!holds("A && (B || !C)", &["A", "C"]));
        assert!(
            !holds("VERSION >= 3", &["VERSION"]),
            "what it cannot read is off"
        );
    }

    #[test]
    fn function_bodies() {
        let lines = read(
            "int a() {\n  X(1);\n}\nvoid T::AddV1()\n{\n  if (x) { Y(2); }\n  Z(3);\n}\nW(4);\n",
            &[],
        );
        let b = bodies(&lines, |c| c.contains("T::AddV"));
        assert_eq!(b.len(), 1);
        let inner: Vec<String> = b[0]
            .iter()
            .flat_map(|l| calls(&l.code))
            .map(|c| c.name)
            .collect();
        assert_eq!(inner, ["AddV1", "if", "Y", "Z"]);
    }
}
