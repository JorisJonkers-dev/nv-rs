//! component-census: the table of which component touches which component's fields.
//!
//! The declarations in `components.tsv` name each component's owner thread and region, the
//! crates whose code it owns and the type prefixes whose fields it owns. `report` scans the
//! typed field accesses in `crates/` and renders `docs/fork/COMPONENT-CENSUS.md`; the table is
//! generated, never edited by hand. `--check` is the test seam: it fails when the report in the
//! tree differs from the one the command generates.
//!
//! ```text
//! cargo run --manifest-path research/component-census/Cargo.toml [-- --check]
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const DECLARATIONS: &str = "components.tsv";

#[derive(Debug, Clone)]
struct Component {
    key: String,
    label: String,
    thread: String,
    region: String,
    crates: Vec<String>,
    types: Vec<String>,
    source: String,
}

#[derive(Debug, Default)]
struct Counts {
    reads: usize,
    writes: usize,
    examples: Vec<String>,
}

#[derive(Debug, Default)]
struct Report {
    components: Vec<Component>,
    accesses: BTreeMap<(String, String, String), Counts>,
    skipped: usize,
    skipped_by_reader: BTreeMap<String, usize>,
    files_scanned: usize,
    files_unclaimed: usize,
}

/// Parses the component declarations: one tab-separated row per component.
fn components(text: &str) -> Result<Vec<Component>, String> {
    let mut found = Vec::new();
    for (line_number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "component" && fields.len() == 7 {
            continue;
        }
        if fields.len() != 7 {
            return Err(format!(
                "line {}: expected 7 tab-separated columns",
                line_number + 1
            ));
        }
        let component = Component {
            key: fields[0].to_string(),
            label: fields[1].to_string(),
            thread: fields[2].to_string(),
            region: fields[3].to_string(),
            crates: split_rules(fields[4]),
            types: split_rules(fields[5]),
            source: fields[6].to_string(),
        };
        if component.key.is_empty()
            || component.label.is_empty()
            || component.thread.is_empty()
            || component.region.is_empty()
        {
            return Err(format!(
                "line {}: component, label, thread and region are required",
                line_number + 1
            ));
        }
        if found.iter().any(|c: &Component| c.key == component.key) {
            return Err(format!(
                "line {}: duplicate component {}",
                line_number + 1,
                component.key
            ));
        }
        found.push(component);
    }
    Ok(found)
}

fn split_rules(text: &str) -> Vec<String> {
    text.split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `# ownerless: A|B` names type prefixes that no component owns: shared layouts
/// in `engine-core`'s `types` module and the platform's own threads and requests.
/// Their accesses are skipped, not attributed to whichever broad prefix matches first.
fn ownerless(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rules) = line.strip_prefix("# ownerless:") {
            found.extend(split_rules(rules));
        }
    }
    found
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The field slot of `set(object, field, value)` is a write. A `Type::m_field`
/// merely passed as a value to `set` is a read.
fn is_write(before_field: &str, after_field: &str) -> bool {
    let before = before_field.trim_end();
    let Some(open) = before.rfind('(') else {
        return false;
    };
    let bytes = before.as_bytes();
    let mut end = open;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident(bytes[start - 1]) {
        start -= 1;
    }
    if &before[start..end] != "set" {
        return false;
    }
    // The field must be the second argument: one top-level comma before it,
    // nothing between that comma and the field, and a comma or `)` after it.
    let args = &before[open + 1..];
    let mut depth = 0usize;
    let mut commas = 0usize;
    let mut last_comma = None;
    for (i, ch) in args.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => return false,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                commas += 1;
                last_comma = Some(i);
            }
            _ => {}
        }
    }
    let Some(last) = last_comma else {
        return false;
    };
    let after = after_field.trim_start();
    commas == 1
        && args[last + 1..].trim().is_empty()
        && (after.starts_with(',') || after.starts_with(')'))
}

fn raw_string_hashes(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) != Some(&b'r') {
        return None;
    }
    let mut hashes = 0;
    let mut at = i + 1;
    while bytes.get(at) == Some(&b'#') {
        hashes += 1;
        at += 1;
    }
    (bytes.get(at) == Some(&b'"')).then_some(hashes)
}

/// A Rust char literal ends here; a lifetime (`'a`, `'static`) does not.
fn char_literal_end(bytes: &[u8], i: usize) -> Option<usize> {
    let next = *bytes.get(i + 1)?;
    if next.is_ascii_whitespace() {
        return None;
    }
    if next == b'\\' {
        let limit = (i + 16).min(bytes.len());
        for at in i + 2..limit {
            if bytes[at] == b'\'' {
                return Some(at + 1);
            }
            if bytes[at] == b'\n' {
                return None;
            }
        }
        return None;
    }
    (bytes.get(i + 2) == Some(&b'\'')).then_some(i + 3)
}

/// Blanks comments and string/character literals, keeping byte offsets and
/// newlines. Every remaining `Type::m_field` token is therefore code.
fn mask_non_code(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    out[i] = b' ';
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                out[i] = b' ';
                out[i + 1] = b' ';
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        out[i] = b' ';
                        out[i + 1] = b' ';
                        i += 2;
                        break;
                    }
                    if bytes[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
            b'r' if raw_string_hashes(bytes, i).is_some() => {
                let hashes = raw_string_hashes(bytes, i).unwrap();
                let mut at = i + 1 + hashes; // the opening quote
                for k in i..=at {
                    out[k] = b' ';
                }
                at += 1;
                while at < bytes.len() {
                    if bytes[at] == b'"'
                        && at + 1 + hashes <= bytes.len()
                        && bytes[at + 1..at + 1 + hashes].iter().all(|b| *b == b'#')
                    {
                        for k in at..at + 1 + hashes {
                            out[k] = b' ';
                        }
                        at += hashes + 1;
                        break;
                    }
                    if bytes[at] != b'\n' {
                        out[at] = b' ';
                    }
                    at += 1;
                }
                i = at;
            }
            b'"' => {
                out[i] = b' ';
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        for k in i..(i + 2).min(bytes.len()) {
                            if bytes[k] != b'\n' {
                                out[k] = b' ';
                            }
                        }
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        out[i] = b' ';
                        i += 1;
                        break;
                    }
                    if bytes[i] != b'\n' {
                        out[i] = b' ';
                    }
                    i += 1;
                }
            }
            b'\'' if char_literal_end(bytes, i).is_some() => {
                let end = char_literal_end(bytes, i).unwrap();
                for k in i..end {
                    if bytes[k] != b'\n' {
                        out[k] = b' ';
                    }
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

/// Function bodies by byte range, so an access in a multi-line signature or a
/// nested function is attributed to the function whose braces enclose it.
fn function_spans(code: &str) -> Vec<(usize, usize, String)> {
    let bytes = code.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i + 2 <= bytes.len() {
        if &bytes[i..i + 2] != b"fn"
            || (i > 0 && is_ident(bytes[i - 1]))
            || (i + 2 < bytes.len() && is_ident(bytes[i + 2]))
        {
            i += 1;
            continue;
        }
        let mut at = i + 2;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let name = at;
        while at < bytes.len() && is_ident(bytes[at]) {
            at += 1;
        }
        if at == name {
            i += 1;
            continue;
        }
        let name = code[name..at].to_string();
        let mut open = at;
        while open < bytes.len() && bytes[open] != b'{' && bytes[open] != b';' {
            open += 1;
        }
        if open >= bytes.len() || bytes[open] == b';' {
            i = at;
            continue;
        }
        let mut depth = 0usize;
        let mut end = open;
        while end < bytes.len() {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        spans.push((open, end, name));
        i = at;
    }
    spans
}

/// Field accesses are typed constants of the form `Type::m_field`.
fn field_accesses(code: &str) -> Vec<(usize, usize, String, String)> {
    let bytes = code.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_ident(bytes[i]) {
            i += 1;
        }
        let name = &code[start..i];
        let mut at = i;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at + 1 >= bytes.len() || &bytes[at..at + 2] != b"::" {
            continue;
        }
        at += 2;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let field_start = at;
        while at < bytes.len() && is_ident(bytes[at]) {
            at += 1;
        }
        if at > field_start + 2 && code[field_start..].starts_with("m_") {
            found.push((
                start,
                at,
                name.to_string(),
                code[field_start..at].to_string(),
            ));
        }
    }
    found
}

fn source_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let mut dirs = vec![root.join("crates")];
    while let Some(dir) = dirs.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
        };
        for entry in entries {
            let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().and_then(|n| n.to_str()) != Some("target") {
                    dirs.push(path);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

impl Report {
    fn skip(&mut self, reader: &str) {
        self.skipped += 1;
        *self
            .skipped_by_reader
            .entry(reader.to_string())
            .or_default() += 1;
    }
}

/// Scans `root/crates` and counts typed field accesses between declared components.
fn collect(root: &Path, components: &[Component], ownerless: &[String]) -> Result<Report, String> {
    let files = source_files(root)?;
    let mut report = Report {
        components: components.to_vec(),
        files_scanned: files.len(),
        ..Report::default()
    };
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let source_path = relative.strip_prefix("crates/").unwrap_or(&relative);
        let mut reader: Option<&Component> = None;
        let mut best = 0;
        for component in components {
            for rule in &component.crates {
                if source_path.starts_with(rule) && rule.len() > best {
                    reader = Some(component);
                    best = rule.len();
                }
            }
        }
        let Some(reader) = reader else {
            report.files_unclaimed += 1;
            continue;
        };
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let code = mask_non_code(&text);
        let functions = function_spans(&code);
        let mut base = 0usize;
        for (line_number, line) in code.split('\n').enumerate() {
            for (start, end, ty, field) in field_accesses(line) {
                let at = base + start;
                let finish = base + end;
                if ownerless.iter().any(|p| ty.starts_with(p)) {
                    report.skip(&reader.key);
                    continue;
                }
                let target = components
                    .iter()
                    .find(|c| c.types.iter().any(|p| ty.starts_with(p)));
                let Some(target) = target else {
                    report.skip(&reader.key);
                    continue;
                };
                if target.key == reader.key {
                    report.skip(&reader.key);
                    continue;
                }
                let entry = report
                    .accesses
                    .entry((
                        reader.key.clone(),
                        target.key.clone(),
                        format!("{ty}::{field}"),
                    ))
                    .or_default();
                if is_write(&code[..at], &code[finish..]) {
                    entry.writes += 1;
                } else {
                    entry.reads += 1;
                }
                let function = functions
                    .iter()
                    .filter(|(start, end, _)| *start <= at && at < *end)
                    .max_by_key(|(start, _, _)| *start)
                    .map(|(_, _, name)| name.clone());
                let key = function
                    .clone()
                    .unwrap_or_else(|| format!("{relative}:{}", line_number + 1));
                let example = function
                    .as_ref()
                    .map(|f| format!("`{f}` at {relative}:{}", line_number + 1))
                    .unwrap_or_else(|| key.clone());
                let seen = entry
                    .examples
                    .iter()
                    .any(|e| e.starts_with(&format!("`{key}`")) || e == &example);
                if entry.examples.len() < 2 && !seen {
                    entry.examples.push(example);
                }
            }
            base += line.len() + 1;
        }
    }
    Ok(report)
}

/// Renders the generated Markdown table.
fn render(report: &Report) -> String {
    let mut out = String::from(
        "# Component census\n\n\
         Generated by `research/component-census`; do not edit. Regenerate with:\n\n\
         ```sh\n\
         cargo run --manifest-path research/component-census/Cargo.toml\n\
         ```\n\n\
         The declarations in `research/component-census/components.tsv` say which thread and which\n\
         region each component owns. The thread column names the owner's thread; a row whose reader\n\
         runs on another thread is exactly the cross-component access this table exists to find.\n\
         The access rows count typed field-access sites in `crates/`, grouped by the component whose\n\
         code reads or writes the field and the component that owns it. Only `Type::m_field` sites\n\
         are counted: fields without the `m_` prefix, and mentions in comments or literals, are not\n\
         accesses. Counts include the field accesses in test code beside the translation in the same\n\
         file. Runtime access counts from `NV_TOUCH` are a separate, private measurement.\n\n\
         ## Components\n\n\
         | Component | Owner thread | Region | Source |\n\
         | --- | --- | --- | --- |\n",
    );
    let mut declared: Vec<&Component> = report.components.iter().collect();
    declared.sort_by(|a, b| a.key.cmp(&b.key));
    for component in declared {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            component.label, component.thread, component.region, component.source
        ));
    }
    out.push_str(
        "\n## Cross-component field accesses\n\n\
         | Reader | Owner | Field | Reads | Writes | Example functions |\n\
         | --- | --- | --- | ---: | ---: | --- |\n",
    );
    for ((reader, owner, field), counts) in &report.accesses {
        out.push_str(&format!(
            "| {reader} | {owner} | {field} | {} | {} | {} |\n",
            counts.reads,
            counts.writes,
            counts.examples.join("; ")
        ));
    }
    let skipped = report.skipped;
    if skipped == 1 {
        out.push_str("\n1 access skipped: not cross-component, or its type has no owner.\n");
    } else {
        out.push_str(&format!(
            "\n{skipped} accesses skipped: not cross-component, or their type has no owner.\n"
        ));
    }
    out.push_str(&format!(
        "\nScanned {} Rust source files under `crates/` ({} with no declared reader).\n",
        report.files_scanned, report.files_unclaimed
    ));
    if !report.skipped_by_reader.is_empty() {
        let mut readers: Vec<_> = report.skipped_by_reader.iter().collect();
        readers.sort_by(|a, b| a.0.cmp(b.0));
        let detail = readers
            .iter()
            .map(|(reader, count)| format!("{reader}: {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("\nSkipped accesses by reader: {detail}.\n"));
    }
    out
}

/// Fails when the report on disk is not the report the command generates.
fn check(generated: &str, path: &Path) -> Result<(), String> {
    let on_disk = fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("{} is missing: regenerate it", path.display())
        } else {
            format!("cannot read {}: {e}", path.display())
        }
    })?;
    if on_disk == generated {
        Ok(())
    } else {
        Err(format!("{} is stale: regenerate it", path.display()))
    }
}

fn run() -> Result<(), String> {
    let tool = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut root = tool
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "cannot find the repository root".to_string())?
        .to_path_buf();
    let mut declarations = tool.join(DECLARATIONS);
    let mut out = None;
    let mut checking = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--check" => checking = true,
            "--root" => root = PathBuf::from(args.next().ok_or("--root needs a path")?),
            "--components" => {
                declarations = PathBuf::from(args.next().ok_or("--components needs a path")?);
            }
            "--out" => out = Some(PathBuf::from(args.next().ok_or("--out needs a path")?)),
            "--help" => {
                println!(
                    "usage: component-census [--root <dir>] [--components <file>] [--out <file>] [--check]"
                );
                return Ok(());
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    let out = out.unwrap_or_else(|| root.join("docs/fork/COMPONENT-CENSUS.md"));
    report_to(&root, &declarations, &out, checking)
}

fn report_to(root: &Path, declarations: &Path, out: &Path, checking: bool) -> Result<(), String> {
    let text = fs::read_to_string(declarations)
        .map_err(|e| format!("cannot read {}: {e}", declarations.display()))?;
    let declared = components(&text)?;
    let ownerless = ownerless(&text);
    let report = collect(root, &declared, &ownerless)?;
    let generated = render(&report);
    if checking {
        check(&generated, out)?;
        println!("{} is current", out.display());
    } else {
        fs::write(out, &generated).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
        println!(
            "wrote {} ({} components, {} access rows)",
            out.display(),
            report.components.len(),
            report.accesses.len()
        );
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("component-census: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("component-census-{tag}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("crates")).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const TWO: &str = "component\tlabel\tthread\tregion\tcrates\ttypes\tsource\n\
interface\tInterface\tmain\tguest/interface\tengine-fallout-interface\tMenu\tissue #916\n\
scene\tGamebryo scene graph\tmain\tguest/scene\tengine-gamebryo\tNi\tissue #916\n";

    fn census(dir: &TempDir) -> String {
        let declared = components(TWO).unwrap();
        render(&collect(dir.path(), &declared, &[]).unwrap())
    }

    #[test]
    fn components_declare_owner_thread_and_region() {
        let dir = TempDir::new("owners");
        let out = census(&dir);
        assert!(
            out.contains("| Interface | main | guest/interface |"),
            "{out}"
        );
        assert!(
            out.contains("| Gamebryo scene graph | main | guest/scene |"),
            "{out}"
        );
    }

    #[test]
    fn cross_component_field_accesses_have_counts_and_example_functions() {
        let dir = TempDir::new("accesses");
        dir.write(
            "crates/engine-fallout-interface/src/typed.rs",
            "fn read_node() {\n\
                 let _ = e.get(node, NiNode::m_usSize);\n\
                 let _ = e.get(node, NiNode::m_usSize);\n\
             }\n\
             fn write_node() {\n\
                 e.set(node, NiNode::m_usSize);\n\
             }\n",
        );
        let out = census(&dir);
        assert!(
            out.contains("| interface | scene | NiNode::m_usSize | 2 | 1 |"),
            "{out}"
        );
        assert!(out.contains("read_node"), "{out}");
        assert!(out.contains("write_node"), "{out}");
    }

    #[test]
    fn own_component_and_unmapped_fields_are_not_cross_component_accesses() {
        let dir = TempDir::new("skip");
        dir.write(
            "crates/engine-gamebryo/src/node.rs",
            "fn own() { let _ = e.get(node, NiNode::m_usSize); }\n",
        );
        dir.write(
            "crates/engine-fallout-interface/src/typed.rs",
            "fn unknown() { let _ = e.get(x, UnknownThing::m_odd); }\n",
        );
        let out = census(&dir);
        assert!(!out.contains("UnknownThing::m_odd"), "{out}");
        assert!(!out.contains("| scene | scene |"), "{out}");
        assert!(out.contains("2 accesses skipped"), "{out}");
    }

    #[test]
    fn declarations_must_name_thread_region_and_unique_keys() {
        assert!(components(
            "component\tlabel\tthread\tregion\tcrates\ttypes\tsource\n\
a\tA\t\tguest/a\tc\tT\ts\n"
        )
        .is_err());
        assert!(components(
            "component\tlabel\tthread\tregion\tcrates\ttypes\tsource\n\
a\tA\tmain\tguest/a\tc\tT\ts\n\
a\tB\tmain\tguest/b\td\tU\ts\n"
        )
        .is_err());
        assert!(components(TWO).is_ok());
    }

    #[test]
    fn check_accepts_a_current_report_and_rejects_a_stale_one() {
        let dir = TempDir::new("check");
        let path = dir.path().join("census.md");
        fs::write(&path, "# census\n").unwrap();
        assert!(check("# census\n", &path).is_ok());
        assert!(check("# other\n", &path).is_err());
    }

    #[test]
    fn check_reports_a_missing_report() {
        let dir = TempDir::new("missing");
        let path = dir.path().join("absent.md");
        let error = check("# census\n", &path).unwrap_err();
        assert!(error.contains("missing"), "{error}");
    }

    #[test]
    fn ownerless_shared_types_are_skipped_not_attributed_to_a_broad_prefix() {
        let dir = TempDir::new("ownerless");
        dir.write(
            "crates/engine-fallout-interface/src/typed.rs",
            "fn read() { let _ = e.get(x, NiTArray::m_usSize); }\n",
        );
        let text = format!("{TWO}# ownerless: NiTArray\n");
        let declared = components(&text).unwrap();
        let free = ownerless(&text);
        let out = render(&collect(dir.path(), &declared, &free).unwrap());
        assert!(!out.contains("NiTArray::m_usSize"), "{out}");
        assert!(out.contains("1 access skipped"), "{out}");
        assert!(
            out.contains("Skipped accesses by reader: interface: 1"),
            "{out}"
        );
    }

    #[test]
    fn type_rules_match_the_first_declaration_in_file_order() {
        let header = "component\tlabel\tthread\tregion\tcrates\ttypes\tsource\n";
        let broad_first = format!(
            "{header}a\tA\tmain\tguest/a\tc\tNi\ts\n\
             b\tB\tmain\tguest/b\t\tNiNode\ts\n\
             r\tR\tmain\tguest/r\td\tR\ts\n"
        );
        let specific_first = format!(
            "{header}b\tB\tmain\tguest/b\t\tNiNode\ts\n\
             a\tA\tmain\tguest/a\tc\tNi\ts\n\
             r\tR\tmain\tguest/r\td\tR\ts\n"
        );
        let dir = TempDir::new("type-order");
        dir.write(
            "crates/d/src/typed.rs",
            "fn f() { let _ = e.get(x, NiNode::m_usSize); }\n",
        );
        let first = render(&collect(dir.path(), &components(&broad_first).unwrap(), &[]).unwrap());
        assert!(
            first.contains("| r | a | NiNode::m_usSize | 1 | 0 |"),
            "{first}"
        );
        let second =
            render(&collect(dir.path(), &components(&specific_first).unwrap(), &[]).unwrap());
        assert!(
            second.contains("| r | b | NiNode::m_usSize | 1 | 0 |"),
            "{second}"
        );
    }

    #[test]
    fn a_field_passed_as_a_value_argument_to_set_is_a_read() {
        let dir = TempDir::new("writes");
        dir.write(
            "crates/engine-fallout-interface/src/typed.rs",
            "fn f() {\n\
                 e.set(node, NiNode::m_usSize, 4);\n\
                 e.set(dst, other.NiNode::m_usSize, 4);\n\
                 let _ = e.get(node, NiNode::m_usESize);\n\
             }\n",
        );
        let out = census(&dir);
        assert!(
            out.contains("| interface | scene | NiNode::m_usSize | 1 | 1 |"),
            "{out}"
        );
        assert!(
            out.contains("| interface | scene | NiNode::m_usESize | 1 | 0 |"),
            "{out}"
        );
    }

    #[test]
    fn comments_and_literals_are_not_field_accesses() {
        let dir = TempDir::new("comments");
        dir.write(
            "crates/engine-fallout-interface/src/typed.rs",
            "/* NiNode::m_usSize */\n\
             fn f() {\n\
                 // e.set(node, NiNode::m_usSize);\n\
                 let s = \"NiNode::m_usSize\";\n\
                 let c = '/'; // NiNode::m_usSize\n\
                 let _ = e.get(node, NiNode::m_usSize);\n\
             }\n",
        );
        let out = census(&dir);
        assert!(
            out.contains("| interface | scene | NiNode::m_usSize | 1 | 0 |"),
            "{out}"
        );
    }

    #[test]
    fn examples_come_from_the_function_body_that_holds_the_access() {
        let dir = TempDir::new("functions");
        dir.write(
            "crates/engine-fallout-interface/src/typed.rs",
            "pub fn\n\
                 outer(\n\
                     x: u32,\n\
                 ) {\n\
                     let _ = e.get(node, NiNode::m_usSize);\n\
                     fn inner() {\n\
                         let _ = e.get(node, NiNode::m_usESize);\n\
                     }\n\
                 }\n",
        );
        let out = census(&dir);
        let path = "crates/engine-fallout-interface/src/typed.rs";
        assert!(out.contains(&format!("`outer` at {path}:5")), "{out}");
        assert!(out.contains(&format!("`inner` at {path}:7")), "{out}");
    }

    #[test]
    fn the_longest_crate_prefix_chooses_the_reader() {
        let text = "component\tlabel\tthread\tregion\tcrates\ttypes\tsource\n\
        scene\tScene\tmain\tguest/scene\tengine-bs\tNi\ts\n\
        renderer\tRenderer\trender thread\tguest/renderer\tengine-bs/src/units/bsshader/\tNiNode\ts\n";
        let declared = components(text).unwrap();
        let dir = TempDir::new("shadow");
        dir.write(
            "crates/engine-bs/src/units/bsshader/property.rs",
            "fn f() { let _ = e.get(x, NiNode::m_usSize); }\n",
        );
        dir.write(
            "crates/engine-bs/src/units/bsmenu/menu.rs",
            "fn g() { let _ = e.get(x, NiNode::m_usSize); }\n",
        );
        let out = render(&collect(dir.path(), &declared, &[]).unwrap());
        assert!(
            out.contains("| renderer | scene | NiNode::m_usSize | 1 | 0 |"),
            "{out}"
        );
        assert!(
            out.contains("Skipped accesses by reader: scene: 1"),
            "{out}"
        );
    }

    #[test]
    fn scan_counts_are_reported() {
        let dir = TempDir::new("counts");
        dir.write(
            "crates/engine-gamebryo/src/node.rs",
            "fn own() { let _ = e.get(node, NiNode::m_usSize); }\n",
        );
        dir.write(
            "crates/host/src/main.rs",
            "fn main() { let _ = e.get(node, NiNode::m_usSize); }\n",
        );
        let out = census(&dir);
        assert!(
            out.contains("Scanned 2 Rust source files under `crates/` (1 with no declared reader)"),
            "{out}"
        );
        assert!(
            out.contains("Skipped accesses by reader: scene: 1"),
            "{out}"
        );
    }
}
