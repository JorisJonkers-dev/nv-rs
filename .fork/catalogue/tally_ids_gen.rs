// SPDX-License-Identifier: GPL-3.0-only
//
// The ids generator of tally. This file uses the standard library only, so a repository can copy it next to its
// catalogue and `include!` it from build.rs without a new dependency (`tally ids --generator <path>` writes the copy,
// `tally check` fails when the copy differs from the one its tally version ships). tally itself compiles the same
// file, so `tally ids --out` and a build.rs give the same module.
//
// It reads only what the module needs: the `id` of each `[group]` and `[[item]]`, `retired`, `aliases` and
// `items_from`. Each of those must sit on one line as a plain string (or a one-line array of plain strings);
// `tally check` compares this reader with the full TOML reader and fails when they disagree.

#[allow(dead_code)]
pub mod tally_ids_gen {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Words that cannot name a module or constant.
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn",
        "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self",
        "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become",
        "box", "do", "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield", "union",
    ];

    /// One item as the generator sees it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct GenItem {
        /// The local id (the last segment).
        pub id: String,
        pub retired: bool,
        /// Earlier full ids of this item.
        pub aliases: Vec<String>,
    }

    /// One group file as the generator sees it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct GenGroup {
        pub file: PathBuf,
        pub id: String,
        pub items_from: Option<String>,
        pub items: Vec<GenItem>,
    }

    /// True when `segment` is a valid id segment: a lower-case letter, then lower-case letters, digits or `_`, and
    /// not a Rust keyword.
    #[must_use]
    pub fn valid_segment(segment: &str) -> bool {
        let mut chars = segment.chars();
        matches!(chars.next(), Some('a'..='z'))
            && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
            && !KEYWORDS.contains(&segment)
    }

    /// True when every dot-separated segment of `id` is valid.
    #[must_use]
    pub fn valid_id(id: &str) -> bool {
        !id.is_empty() && id.split('.').all(valid_segment)
    }

    /// The group files under `catalogue`: every `.toml` below a sub-directory, except under `ref/`.
    ///
    /// # Errors
    /// When a directory cannot be read.
    pub fn group_files(catalogue: &Path) -> Result<Vec<PathBuf>, String> {
        let mut out = Vec::new();
        let mut entries = read_dir_sorted(catalogue)?;
        entries.retain(|p| p.is_dir() && p.file_name().is_some_and(|n| n != "ref"));
        for dir in entries {
            walk(&dir, &mut out)?;
        }
        out.sort();
        Ok(out)
    }

    fn read_dir_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
        let rd = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        v.sort();
        Ok(v)
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for p in read_dir_sorted(dir)? {
            if p.is_dir() {
                walk(&p, out)?;
            } else if p.extension().is_some_and(|e| e == "toml") {
                out.push(p);
            }
        }
        Ok(())
    }

    /// A plain one-line string value: `"text"` with an optional `# comment` after it.
    fn plain_string(value: &str) -> Option<String> {
        let rest = value.strip_prefix('"')?;
        let end = rest.find('"')?;
        let text = &rest[..end];
        if text.contains('\\') {
            return None;
        }
        let tail = rest[end + 1..].trim();
        (tail.is_empty() || tail.starts_with('#')).then(|| text.to_string())
    }

    /// A one-line array of plain strings: `["a", "b"]`.
    fn plain_array(value: &str) -> Option<Vec<String>> {
        let rest = value.strip_prefix('[')?;
        let end = rest.find(']')?;
        let tail = rest[end + 1..].trim();
        if !(tail.is_empty() || tail.starts_with('#')) {
            return None;
        }
        let inner = rest[..end].trim().trim_end_matches(',');
        if inner.trim().is_empty() {
            return Some(Vec::new());
        }
        inner.split(',').map(|s| plain_string(s.trim())).collect()
    }

    /// Reads one group file with the line reader.
    ///
    /// # Errors
    /// When the file cannot be read, or a key the generator needs is not on one line as a plain string.
    pub fn read_group(file: &Path) -> Result<GenGroup, String> {
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        read_group_text(file, &text)
    }

    /// Reads the text of one group file.
    ///
    /// # Errors
    /// As [`read_group`].
    pub fn read_group_text(file: &Path, text: &str) -> Result<GenGroup, String> {
        #[derive(PartialEq)]
        enum Section {
            None,
            Group,
            Item,
            Other,
        }
        let mut section = Section::None;
        let mut group_id: Option<String> = None;
        let mut items_from = None;
        let mut items: Vec<GenItem> = Vec::new();
        let mut item_ids: Vec<Option<String>> = Vec::new();
        let at = |n: usize| format!("{}:{}", file.display(), n + 1);
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                let header = line.split('#').next().unwrap_or("").trim();
                section = match header {
                    "[group]" => Section::Group,
                    "[[item]]" => {
                        items.push(GenItem { id: String::new(), retired: false, aliases: Vec::new() });
                        item_ids.push(None);
                        Section::Item
                    }
                    _ => Section::Other,
                };
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            match (&section, key) {
                (Section::Group, "id") => {
                    group_id = Some(plain_string(value).ok_or_else(|| format!("{}: write `id = \"...\"` on one line", at(n)))?);
                }
                (Section::Group, "items_from") => {
                    items_from =
                        Some(plain_string(value).ok_or_else(|| format!("{}: write `items_from = \"...\"` on one line", at(n)))?);
                }
                (Section::Item, "id") => {
                    let id = plain_string(value).ok_or_else(|| format!("{}: write `id = \"...\"` on one line", at(n)))?;
                    if let (Some(item), Some(slot)) = (items.last_mut(), item_ids.last_mut()) {
                        item.id.clone_from(&id);
                        *slot = Some(id);
                    }
                }
                (Section::Item, "retired") => {
                    if let Some(item) = items.last_mut() {
                        item.retired = true;
                    }
                }
                (Section::Item, "aliases") => {
                    let list = plain_array(value)
                        .ok_or_else(|| format!("{}: write `aliases = [\"...\"]` on one line, plain strings", at(n)))?;
                    if let Some(item) = items.last_mut() {
                        item.aliases = list;
                    }
                }
                _ => {}
            }
        }
        let id = group_id.ok_or_else(|| format!("{}: no `id` in `[group]`", file.display()))?;
        if let Some(pos) = item_ids.iter().position(Option::is_none) {
            return Err(format!("{}: item {} has no `id`", file.display(), pos + 1));
        }
        Ok(GenGroup { file: file.to_path_buf(), id, items_from, items })
    }

    /// The keys of a reference list (first column, header line skipped).
    ///
    /// # Errors
    /// When the file cannot be read.
    pub fn list_keys(file: &Path) -> Result<Vec<String>, String> {
        let text = fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        Ok(text
            .lines()
            .skip(1)
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.split('\t').next().unwrap_or("").trim().to_string())
            .collect())
    }

    /// Reads every group of `catalogue`, with the items of `items_from` lists added (an explicit `[[item]]` with the
    /// same id wins).
    ///
    /// # Errors
    /// When a file cannot be read or a key is not in the one-line form.
    pub fn read_catalogue(catalogue: &Path) -> Result<Vec<GenGroup>, String> {
        let mut groups = Vec::new();
        for file in group_files(catalogue)? {
            let mut group = read_group(&file)?;
            if let Some(list) = &group.items_from {
                for key in list_keys(&catalogue.join(list))? {
                    if !group.items.iter().any(|i| i.id == key) {
                        group.items.push(GenItem { id: key, retired: false, aliases: Vec::new() });
                    }
                }
            }
            groups.push(group);
        }
        Ok(groups)
    }

    #[derive(Default)]
    struct Node {
        children: BTreeMap<String, Node>,
        consts: BTreeMap<String, (String, Option<String>)>,
    }

    fn insert(root: &mut Node, full: &str, value: &str, deprecated: Option<String>) -> Result<(), String> {
        if !valid_id(full) || !full.contains('.') {
            return Err(format!("`{full}` is not a valid id (dotted lower-case segments, no Rust keywords)"));
        }
        let mut parts: Vec<&str> = full.split('.').collect();
        let last = parts.pop().unwrap_or_default();
        let mut node = root;
        for p in parts {
            node = node.children.entry(p.to_string()).or_default();
        }
        let name = last.to_ascii_uppercase();
        if node.consts.contains_key(&name) || node.children.contains_key(last) {
            return Err(format!("`{full}` is defined twice"));
        }
        node.consts.insert(name, (value.to_string(), deprecated));
        Ok(())
    }

    fn emit(node: &Node, depth: usize, out: &mut String) {
        let indent = "    ".repeat(depth);
        let up = "super::".repeat(depth);
        for (name, (value, deprecated)) in &node.consts {
            if let Some(note) = deprecated {
                let _ = writeln!(out, "{indent}#[deprecated(note = \"{note}\")]");
            }
            let _ = writeln!(out, "{indent}pub const {name}: {up}Id = {up}Id(\"{value}\");");
        }
        for (name, child) in &node.children {
            let _ = writeln!(out, "{indent}pub mod {name} {{");
            emit(child, depth + 1, out);
            let _ = writeln!(out, "{indent}}}");
        }
    }

    /// The Rust source of the ids module for these groups.
    ///
    /// # Errors
    /// When an id is invalid or defined twice.
    pub fn module_source(groups: &[GenGroup]) -> Result<String, String> {
        let mut root = Node::default();
        for g in groups {
            for item in &g.items {
                let full = format!("{}.{}", g.id, item.id);
                // A retired item and its aliases have no constant: a test that still names one fails to compile.
                if item.retired {
                    continue;
                }
                insert(&mut root, &full, &full, None)?;
                for alias in &item.aliases {
                    insert(&mut root, alias, &full, Some(format!("renamed to {full}")))?;
                }
            }
        }
        let mut out = String::from(HEADER);
        out.push_str("#[allow(dead_code, non_snake_case, clippy::all, clippy::pedantic)]\npub mod ids {\n");
        emit(&root, 1, &mut out);
        out.push_str("}\n");
        Ok(out)
    }

    const HEADER: &str = r#"// Generated by tally from the catalogue. Do not edit and do not commit: build.rs writes it on every build.

/// The id of one catalogue item. Only the generated constants below make one, so a test cannot name an id the
/// catalogue does not have.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(&'static str);

#[allow(dead_code)]
impl Id {
    /// The dotted id.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// Marks the calling test as covering these catalogue items. It does nothing unless the environment variable
/// `TALLY_COVERS_LOG` names a file: then it appends one line per id, `<test thread name>\t<id>`.
#[allow(dead_code, clippy::all, clippy::pedantic)]
pub fn covers(ids: &[Id]) {
    use std::io::Write as _;
    if let Some(path) = std::env::var_os("TALLY_COVERS_LOG") {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let mut lines = String::new();
        for id in ids {
            lines.push_str(name);
            lines.push('\t');
            lines.push_str(id.0);
            lines.push('\n');
        }
        if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(lines.as_bytes());
        }
    }
}

"#;

    /// For build.rs: reads `catalogue` (relative to the package root), writes `$OUT_DIR/tally_ids.rs` when it
    /// changed, and asks cargo to run again when the catalogue changes. Include the module with
    /// `include!(concat!(env!("OUT_DIR"), "/tally_ids.rs"));`.
    ///
    /// # Panics
    /// When the catalogue cannot be read or has an invalid id: the build fails with the reason.
    pub fn build(catalogue: &str) {
        println!("cargo:rerun-if-changed={catalogue}");
        let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo");
        let out = Path::new(&out_dir).join("tally_ids.rs");
        let source = read_catalogue(Path::new(catalogue)).and_then(|g| module_source(&g));
        let source = match source {
            Ok(s) => s,
            Err(e) => panic!("tally ids: {e}"),
        };
        if fs::read_to_string(&out).ok().as_deref() != Some(source.as_str()) {
            fs::write(&out, source).expect("write the ids module");
        }
    }
}
