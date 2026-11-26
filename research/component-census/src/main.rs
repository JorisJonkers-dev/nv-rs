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

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// A `set` call writes the field; every other typed access reads it.
fn is_write(prefix: &str) -> bool {
    let before = prefix.trim_end();
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
    &before[start..end] == "set"
}

/// The function whose body holds the access, for the example column.
fn enclosing_function(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    for start in 0..bytes.len().saturating_sub(2) {
        if &bytes[start..start + 2] != b"fn"
            || (start > 0 && is_ident(bytes[start - 1]))
            || (start + 2 < bytes.len() && is_ident(bytes[start + 2]))
        {
            continue;
        }
        let mut at = start + 2;
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let name = at;
        while at < bytes.len() && is_ident(bytes[at]) {
            at += 1;
        }
        if at > name {
            return Some(code[name..at].to_string());
        }
    }
    None
}

/// Removes a trailing line comment, leaving string literals alone.
fn code_part(line: &str) -> &str {
    let mut quoted = false;
    let bytes = line.as_bytes();
    for i in 0..bytes.len() {
        match bytes[i] {
            b'"' => quoted = !quoted,
            b'/' if !quoted && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                return &line[..i];
            }
            _ => {}
        }
    }
    line
}

/// Field accesses are typed constants of the form `Type::m_field`.
fn field_accesses(code: &str) -> Vec<(usize, String, String)> {
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
            found.push((start, name.to_string(), code[field_start..at].to_string()));
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

/// Scans `root/crates` and counts typed field accesses between declared components.
fn collect(root: &Path, components: &[Component]) -> Result<Report, String> {
    let mut report = Report {
        components: components.to_vec(),
        ..Report::default()
    };
    for path in source_files(root)? {
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
            continue;
        };
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut function = None;
        for (line_number, raw) in text.lines().enumerate() {
            let code = code_part(raw);
            if let Some(name) = enclosing_function(code) {
                function = Some(name);
            }
            for (at, ty, field) in field_accesses(code) {
                let target = components
                    .iter()
                    .find(|c| c.types.iter().any(|p| ty.starts_with(p)));
                let Some(target) = target else {
                    report.skipped += 1;
                    continue;
                };
                if target.key == reader.key {
                    report.skipped += 1;
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
                if is_write(&code[..at]) {
                    entry.writes += 1;
                } else {
                    entry.reads += 1;
                }
                let key = function
                    .clone()
                    .unwrap_or_else(|| format!("{relative}:{}", line_number + 1));
                let example = match &function {
                    Some(f) => format!("`{f}` at {relative}:{}", line_number + 1),
                    None => key.clone(),
                };
                let seen = entry
                    .examples
                    .iter()
                    .any(|e| e.starts_with(&format!("`{key}`")) || e == &example);
                if entry.examples.len() < 2 && !seen {
                    entry.examples.push(example);
                }
            }
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
         region each component owns. The access rows count typed field-access sites in `crates/`,\n\
         grouped by the component whose code reads or writes the field and the component that owns\n\
         it. Counts include the field accesses in test code beside the translation in the same file.\n\
         Runtime access counts from `NV_TOUCH` are a separate, private measurement.\n\n\
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
    out
}

/// Fails when the report on disk is not the report the command generates.
fn check(generated: &str, path: &Path) -> Result<(), String> {
    let on_disk =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
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
    let report = collect(root, &declared)?;
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
        render(&collect(dir.path(), &declared).unwrap())
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
}
