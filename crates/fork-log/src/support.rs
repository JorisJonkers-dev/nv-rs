// SPDX-License-Identifier: GPL-3.0-only
//! What a mod needs that this engine does not have (nv-rs: mod support report).
//!
//! A mod can ship two things the engine may not answer: a script file the script runner compiles
//! (`Data/NVSE/plugins/scripts/gl_*.txt`, `gr_*`, `ln_*`, ...) that calls a command no built-in
//! extender has, and a plugin DLL (`Data/NVSE/Plugins/*.dll`) the engine has no counterpart for.
//! Without this module the log names the script file and the command but not the mod, and a
//! plugin DLL that is not loaded is not named at all.
//!
//! The pieces:
//! - [`set_data_dir`] gives the data folder; with it a file is traced to the mod that ships it
//!   (the staged data folder links each file into its mod's folder: the link's target minus the
//!   file's own path below `Data` is the mod's folder, [`mod_of`]);
//! - [`scan_plugins`] lists the plugin DLLs and what the engine has for each ([`answer_for`]);
//! - [`script_message`] takes the compiler's message for one script file (from the runner) and
//!   records the missing commands in it; [`script_skipped`] records a file that did not compile;
//! - [`finish`] (called once the run ends) names the extender plugin that owns each missing
//!   command (the command's name found in the plugin DLLs' bytes, [`owner_of`]), returns the
//!   summary lines and writes the machine-readable file ([`Report::to_json`]) next to the log.
//!
//! Only std; the data is behind one lock. Nothing here reads a file the game did not already
//! have in its data folder, and nothing is written but the one report file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The version of the machine-readable file's layout.
pub const REPORT_VERSION: u32 = 1;

/// What the engine has for a plugin DLL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Answer {
    /// Nothing: the DLL is not loaded and nothing stands in for it.
    None,
    /// Part of the plugin is built in (some commands or some fixes).
    Partial,
    /// The plugin's effect is built in, as another feature (not the DLL itself).
    Equivalent,
    /// The plugin is built in.
    BuiltIn,
}

impl Answer {
    /// The word the log and the file use.
    pub fn word(self) -> &'static str {
        match self {
            Answer::None => "none",
            Answer::Partial => "partial",
            Answer::Equivalent => "equivalent",
            Answer::BuiltIn => "built-in",
        }
    }
}

/// The plugin DLLs the engine has an answer for (lower-case file names), with what it has.
/// Anything not here has none. Kept by hand next to `docs/FIXES.md` and `.fork/capabilities.json`.
const ANSWERED: &[(&str, Answer, &str)] = &[
    (
        "jip_nvse.dll",
        Answer::BuiltIn,
        "JIP LN NVSE is built in (commands and most game patches)",
    ),
    (
        "johnnyguitar.dll",
        Answer::Partial,
        "JohnnyGuitar NVSE: only part of its commands are built in",
    ),
    (
        "johnnyguitarnvse.dll",
        Answer::Partial,
        "JohnnyGuitar NVSE: only part of its commands are built in",
    ),
    (
        "showoffnvse.dll",
        Answer::Partial,
        "ShowOff xNVSE: only the command TTW calls is built in",
    ),
    (
        "nvse_stewie_tweaks.dll",
        Answer::Partial,
        "lStewieAl's Tweaks: engine fixes built in row by row, no script commands",
    ),
    ("ttw_nvse.dll", Answer::BuiltIn, "TTW NVSE is built in"),
    (
        "anhnvse.dll",
        Answer::Partial,
        "AnhNVSE: only GetQuestCompletedAlt and Sv_PadStart are built in",
    ),
    (
        "mcm.dll",
        Answer::BuiltIn,
        "The Mod Configuration Menu is built in",
    ),
    (
        "ui_organizer.dll",
        Answer::BuiltIn,
        "UI Organizer is built in",
    ),
    (
        "nvtf.dll",
        Answer::Partial,
        "NVTF: the fixed step is built in, the rest is not",
    ),
    (
        "engineoptimizations.dll",
        Answer::Partial,
        "lStewieAl's Engine Optimizations: single rows are built in",
    ),
    (
        "actorcausesavebloatfix.dll",
        Answer::Equivalent,
        "the engine has its own fix for this",
    ),
    (
        "celloffsetgenerator.dll",
        Answer::Equivalent,
        "the engine writes cell offsets for every plugin itself",
    ),
    (
        "mlf.dll",
        Answer::Equivalent,
        "the engine reads files directly, which is what Mod Limit Fix does",
    ),
    (
        "muzzleflashfix.dll",
        Answer::Equivalent,
        "the engine has its own muzzle flash light fade",
    ),
    (
        "vatslagfix.dll",
        Answer::Equivalent,
        "the engine has its own fix for this",
    ),
];

/// What the engine has for the plugin DLL named `file` (any case): the answer and a short note.
pub fn answer_for(file: &str) -> (Answer, &'static str) {
    let lower = file.to_ascii_lowercase();
    ANSWERED
        .iter()
        .find(|(name, _, _)| *name == lower)
        .map_or((Answer::None, "no built-in counterpart"), |(_, a, n)| {
            (*a, *n)
        })
}

/// One plugin DLL a mod ships.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFile {
    /// The mod that ships it, or "" when it cannot be told.
    pub mod_name: String,
    /// The file's name.
    pub dll: String,
    /// The path of the file (resolved), for reading its bytes.
    pub path: PathBuf,
    pub answer: Answer,
    pub note: String,
}

/// One problem found in a script file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The script calls a command no built-in extender has.
    MissingCommand(String),
    /// Another compile message (kept as the compiler said it, one line).
    Other(String),
}

/// One recorded problem of one script file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptProblem {
    /// The mod that ships the script file ("" when it cannot be told).
    pub mod_name: String,
    /// The file's path below `Data`, with `/` (`NVSE/plugins/scripts/gl_x.txt`).
    pub file: String,
    /// The line of the script, when the message named it.
    pub line: Option<u32>,
    pub problem: Problem,
}

/// Everything recorded in a run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub plugins: Vec<PluginFile>,
    pub scripts: Vec<ScriptProblem>,
    /// Script files that did not compile, by file path below `Data`, with their mod.
    pub skipped: BTreeMap<String, String>,
}

struct State {
    data: Option<PathBuf>,
    report: Report,
    /// The file the runner is compiling now.
    current: Option<String>,
    summary_said: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    data: None,
    report: Report {
        plugins: Vec::new(),
        scripts: Vec::new(),
        skipped: BTreeMap::new(),
    },
    current: None,
    summary_said: false,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Gives the data folder (once, at start); files are traced to their mods against it.
pub fn set_data_dir(data: &Path) {
    state().data = Some(data.to_path_buf());
}

/// The folder below `base` that `rel` (components with `/`) names, each component matched without
/// case; `None` when a component is missing.
pub fn find_nocase(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut at = base.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        let direct = at.join(part);
        if direct.symlink_metadata().is_ok() {
            at = direct;
            continue;
        }
        let found = std::fs::read_dir(&at)
            .ok()?
            .filter_map(Result::ok)
            .find(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(part))
            })?;
        at = found.path();
    }
    Some(at)
}

/// The mod that ships the file `rel` (below `data`, `/`-separated): when the file is a link, the
/// link's final target minus the file's own path is the mod's folder, and its name is the mod's
/// name. A file that is not a link (a copy, the game's own file) gives `None`.
pub fn mod_of(data: &Path, rel: &str) -> Option<String> {
    let at = find_nocase(data, rel)?;
    if at.symlink_metadata().ok()?.file_type().is_symlink() {
        let target = std::fs::canonicalize(&at).ok()?;
        let depth = rel.split('/').filter(|p| !p.is_empty()).count();
        let mut root = target.as_path();
        for _ in 0..depth {
            root = root.parent()?;
        }
        return root.file_name().map(|n| n.to_string_lossy().into_owned());
    }
    None
}

/// The plugin DLLs in `data`'s `NVSE/Plugins` folder (names without case), each with its mod and
/// what the engine has for it; sorted by name.
pub fn list_plugins(data: &Path) -> Vec<PluginFile> {
    let Some(folder) = find_nocase(data, "NVSE/Plugins") else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".dll") {
            continue;
        }
        let (answer, note) = answer_for(&name);
        let rel = format!("NVSE/Plugins/{name}");
        let path = std::fs::canonicalize(e.path()).unwrap_or_else(|_| e.path());
        out.push(PluginFile {
            mod_name: mod_of(data, &rel).unwrap_or_default(),
            dll: name,
            path,
            answer,
            note: note.to_string(),
        });
    }
    out.sort_by_key(|a| a.dll.to_ascii_lowercase());
    out
}

/// One line for a plugin DLL the engine does not fully answer, or `None` for one it does.
pub fn plugin_line(p: &PluginFile) -> Option<String> {
    if matches!(p.answer, Answer::BuiltIn | Answer::Equivalent) {
        return None;
    }
    let who = if p.mod_name.is_empty() {
        "a mod".to_string()
    } else {
        format!("mod '{}'", p.mod_name)
    };
    let what = match p.answer {
        Answer::Partial => "only part of it is built in",
        _ => "the engine has no answer for it: it is not loaded",
    };
    Some(format!(
        "[MOD SUPPORT] {who} ships the plugin {}: {what} ({})",
        p.dll, p.note
    ))
}

/// Lists the plugin DLLs of the data folder, keeps them for the report and returns one line per
/// DLL that the engine does not fully answer (for the log). Call once, at start, after
/// [`set_data_dir`].
pub fn scan_plugins() -> Vec<String> {
    let Some(data) = state().data.clone() else {
        return Vec::new();
    };
    let plugins = list_plugins(&data);
    let lines = plugins.iter().filter_map(plugin_line).collect();
    state().report.plugins = plugins;
    lines
}

/// The command a compile message names as missing: `Script command "X" not found.` (the game's
/// compiler) or `Syntax Error.  Unknown command 'X'.` (the extender's expression compiler).
pub fn missing_command(message: &str) -> Option<&str> {
    if let Some(at) = message.find("Script command \"") {
        let rest = &message[at + "Script command \"".len()..];
        let end = rest.find('"')?;
        return rest[end..].contains("not found").then_some(&rest[..end]);
    }
    let at = message.find("Unknown command '")? + "Unknown command '".len();
    let rest = &message[at..];
    rest.find('\'').map(|end| &rest[..end])
}

/// The line a compile message names: `Script 'x', line 53:`.
pub fn message_line(message: &str) -> Option<u32> {
    let at = message.find(", line ")? + ", line ".len();
    let digits: String = message[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The runner starts compiling the file `rel` (below `Data`): later messages belong to it.
pub fn script_begin(rel: &str) {
    state().current = Some(rel.to_string());
}

/// The runner is done with the file.
pub fn script_end() {
    state().current = None;
}

/// A compile message for the file being compiled. Records every missing command and any other
/// message line; returns one log line per missing command not said before for this file.
pub fn script_message(message: &str) -> Vec<String> {
    let mut st = state();
    let Some(file) = st.current.clone() else {
        return Vec::new();
    };
    let data = st.data.clone();
    let mod_name = data
        .as_deref()
        .and_then(|d| mod_of(d, &file))
        .unwrap_or_default();
    let mut said = Vec::new();
    let mut line_no = message_line(message);
    for text in message.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(n) = message_line(text) {
            line_no = Some(n);
        }
        let problem = match missing_command(text) {
            Some(c) => Problem::MissingCommand(c.to_string()),
            None if text.starts_with("Script '") && text.ends_with(':') => continue,
            None => Problem::Other(text.to_string()),
        };
        let known = st
            .report
            .scripts
            .iter()
            .any(|s| s.file == file && s.problem == problem);
        if known {
            continue;
        }
        if let Problem::MissingCommand(c) = &problem {
            let who = if mod_name.is_empty() {
                "a mod".to_string()
            } else {
                format!("mod '{mod_name}'")
            };
            let at = line_no.map(|n| format!(", line {n}")).unwrap_or_default();
            said.push(format!(
                "[MOD SUPPORT] {who} ships the script {file}{at}: the command {c} is not available, the script is skipped"
            ));
        }
        st.report.scripts.push(ScriptProblem {
            mod_name: mod_name.clone(),
            file: file.clone(),
            line: line_no,
            problem,
        });
    }
    said
}

/// The file that did not compile (below `Data`): remembered with its mod.
pub fn script_skipped(rel: &str) {
    let mut st = state();
    let mod_name = st
        .data
        .as_deref()
        .and_then(|d| mod_of(d, rel))
        .unwrap_or_default();
    st.report.skipped.insert(rel.to_string(), mod_name);
}

/// Whether `bytes` holds `name` as a whole word (no letter, digit or `_` either side), without
/// case.
pub fn contains_word(bytes: &[u8], name: &str) -> bool {
    let n = name.as_bytes();
    if n.is_empty() || bytes.len() < n.len() {
        return false;
    }
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0;
    while i + n.len() <= bytes.len() {
        if bytes[i..i + n.len()].eq_ignore_ascii_case(n)
            && (i == 0 || !word(bytes[i - 1]))
            && (i + n.len() == bytes.len() || !word(bytes[i + n.len()]))
        {
            return true;
        }
        i += 1;
    }
    false
}

/// The plugin whose DLL holds the command's name (an NVSE plugin registers its commands with
/// their names as plain text in the file): the first DLL of `plugins` that does, from `bytes_of`.
pub fn owner_of<'a>(
    command: &str,
    plugins: &'a [PluginFile],
    bytes_of: &mut dyn FnMut(&PluginFile) -> Option<std::rc::Rc<Vec<u8>>>,
) -> Option<&'a PluginFile> {
    plugins
        .iter()
        .find(|p| bytes_of(p).is_some_and(|b| contains_word(&b, command)))
}

/// The summary of a report: the lines for the log and the counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Per missing command: its owner plugin DLL (if known), the mods and files that call it.
    pub commands: Vec<CommandRow>,
    /// Per plugin DLL without a full answer, and per owner of a missing command.
    pub plugins: Vec<PluginRow>,
    /// Script files that did not compile, and how many of them name a missing command.
    pub skipped_files: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRow {
    pub command: String,
    /// The plugin DLL that owns the command, when known.
    pub plugin: Option<String>,
    /// The mods whose scripts call it ("" for a mod that cannot be told is listed as "(unknown)").
    pub mods: Vec<String>,
    pub scripts: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginRow {
    pub dll: String,
    pub mod_name: String,
    pub answer: Answer,
    /// Missing commands that belong to this plugin, and the script files they stop.
    pub commands: usize,
    pub scripts: usize,
}

impl Report {
    /// The report's summary. `bytes_of` reads a plugin DLL's bytes (for the owners of commands).
    pub fn summarize(
        &self,
        bytes_of: &mut dyn FnMut(&PluginFile) -> Option<std::rc::Rc<Vec<u8>>>,
    ) -> Summary {
        let mut by_command: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> =
            BTreeMap::new();
        for s in &self.scripts {
            if let Problem::MissingCommand(c) = &s.problem {
                let e = by_command.entry(c.clone()).or_default();
                e.0.insert(if s.mod_name.is_empty() {
                    "(unknown)".into()
                } else {
                    s.mod_name.clone()
                });
                e.1.insert(s.file.clone());
            }
        }
        let mut commands = Vec::new();
        let mut owner_scripts: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> =
            BTreeMap::new();
        for (c, (mods, files)) in by_command {
            let plugin = owner_of(&c, &self.plugins, bytes_of).map(|p| p.dll.clone());
            if let Some(p) = &plugin {
                let e = owner_scripts.entry(p.clone()).or_default();
                e.0.insert(c.clone());
                e.1.extend(files.iter().cloned());
            }
            commands.push(CommandRow {
                command: c,
                plugin,
                mods: mods.into_iter().collect(),
                scripts: files.len(),
            });
        }
        commands.sort_by(|a, b| b.scripts.cmp(&a.scripts).then(a.command.cmp(&b.command)));
        let mut plugins = Vec::new();
        for p in &self.plugins {
            let (cmds, files) = owner_scripts
                .get(&p.dll)
                .map_or((0, 0), |(c, f)| (c.len(), f.len()));
            if matches!(p.answer, Answer::None | Answer::Partial) || cmds > 0 {
                plugins.push(PluginRow {
                    dll: p.dll.clone(),
                    mod_name: p.mod_name.clone(),
                    answer: p.answer,
                    commands: cmds,
                    scripts: files,
                });
            }
        }
        plugins.sort_by(|a, b| {
            b.scripts
                .cmp(&a.scripts)
                .then(a.dll.to_ascii_lowercase().cmp(&b.dll.to_ascii_lowercase()))
        });
        Summary {
            commands,
            plugins,
            skipped_files: self.skipped.len(),
        }
    }
}

impl Summary {
    /// The lines for the end of the run's log: one per plugin DLL without a full answer that owns
    /// missing commands or has none loaded, one per missing command, and a heading with the counts.
    pub fn lines(&self, report: &Report) -> Vec<String> {
        let unanswered = self
            .plugins
            .iter()
            .filter(|p| p.answer == Answer::None)
            .count();
        let files: BTreeSet<&str> = report.scripts.iter().map(|s| s.file.as_str()).collect();
        let mut out = vec![format!(
            "[MOD SUPPORT] summary: {unanswered} plugin DLL(s) without an answer; {} script file(s) did not compile ({} missing command(s); the compiler stops a file at its first)",
            files.len().max(self.skipped_files),
            self.commands.len(),
        )];
        for p in &self.plugins {
            let mut line = format!(
                "[MOD SUPPORT]   plugin {} (mod '{}'): {}",
                p.dll,
                if p.mod_name.is_empty() {
                    "(unknown)"
                } else {
                    &p.mod_name
                },
                p.answer.word()
            );
            if p.commands > 0 {
                let _ = write!(
                    line,
                    "; {} missing command(s) stop {} script file(s)",
                    p.commands, p.scripts
                );
            }
            out.push(line);
        }
        for c in &self.commands {
            out.push(format!(
                "[MOD SUPPORT]   command {}: {} script file(s) of {} (plugin {})",
                c.command,
                c.scripts,
                c.mods.join(", "),
                c.plugin.as_deref().unwrap_or("not known")
            ));
        }
        out
    }
}

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_opt(out: &mut String, s: Option<&str>) {
    match s {
        Some(s) => json_str(out, s),
        None => out.push_str("null"),
    }
}

impl Report {
    /// The machine-readable file: `version`, `plugins` (every plugin DLL with its answer),
    /// `scripts` (every problem of every script file), `missing_commands` and `missing_plugins`
    /// (the counts). `summary` is [`Report::summarize`] of this report.
    pub fn to_json(&self, summary: &Summary) -> String {
        let mut o = String::new();
        let _ = write!(o, "{{\"version\":{REPORT_VERSION},\"plugins\":[");
        for (i, p) in self.plugins.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"mod\":");
            json_str(&mut o, &p.mod_name);
            o.push_str(",\"dll\":");
            json_str(&mut o, &p.dll);
            let _ = write!(o, ",\"answer\":\"{}\",\"note\":", p.answer.word());
            json_str(&mut o, &p.note);
            o.push('}');
        }
        o.push_str("],\"scripts\":[");
        for (i, s) in self.scripts.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"mod\":");
            json_str(&mut o, &s.mod_name);
            o.push_str(",\"file\":");
            json_str(&mut o, &s.file);
            o.push_str(",\"line\":");
            match s.line {
                Some(n) => {
                    let _ = write!(o, "{n}");
                }
                None => o.push_str("null"),
            }
            match &s.problem {
                Problem::MissingCommand(c) => {
                    o.push_str(",\"kind\":\"missing_command\",\"command\":");
                    json_str(&mut o, c);
                    let plugin = summary
                        .commands
                        .iter()
                        .find(|r| &r.command == c)
                        .and_then(|r| r.plugin.as_deref());
                    o.push_str(",\"plugin\":");
                    json_opt(&mut o, plugin);
                }
                Problem::Other(m) => {
                    o.push_str(",\"kind\":\"compile_message\",\"message\":");
                    json_str(&mut o, m);
                }
            }
            o.push('}');
        }
        o.push_str("],\"missing_commands\":[");
        for (i, c) in summary.commands.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"command\":");
            json_str(&mut o, &c.command);
            o.push_str(",\"plugin\":");
            json_opt(&mut o, c.plugin.as_deref());
            let _ = write!(o, ",\"scripts\":{},\"mods\":[", c.scripts);
            for (j, m) in c.mods.iter().enumerate() {
                if j > 0 {
                    o.push(',');
                }
                json_str(&mut o, m);
            }
            o.push_str("]}");
        }
        o.push_str("],\"missing_plugins\":[");
        for (i, p) in summary.plugins.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"dll\":");
            json_str(&mut o, &p.dll);
            o.push_str(",\"mod\":");
            json_str(&mut o, &p.mod_name);
            let _ = write!(
                o,
                ",\"answer\":\"{}\",\"missing_commands\":{},\"scripts\":{}}}",
                p.answer.word(),
                p.commands,
                p.scripts
            );
        }
        let _ = writeln!(o, "],\"skipped_script_files\":{}}}", summary.skipped_files);
        o
    }
}

/// The folder of a text or JSON log gives the report's path: `<log stem>.mod-support.json`
/// beside the JSON log, else `mod-support.json` in the log folder.
pub fn report_path(json_log: Option<&Path>, log_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = json_log {
        let stem = p.file_stem()?.to_string_lossy().into_owned();
        return Some(p.with_file_name(format!("{stem}.mod-support.json")));
    }
    log_dir.map(|d| d.join("mod-support.json"))
}

/// The end of a run: the summary lines (the first time only, so a stop and the exit that follows
/// say it once) and the file written to `path` when given. Returns the lines (empty when there
/// is nothing to say or they were said before).
pub fn finish(path: Option<&Path>) -> Vec<String> {
    let mut cache: BTreeMap<PathBuf, Option<std::rc::Rc<Vec<u8>>>> = BTreeMap::new();
    let report = state().report.clone();
    let mut read = |p: &PluginFile| {
        cache
            .entry(p.path.clone())
            .or_insert_with(|| std::fs::read(&p.path).ok().map(std::rc::Rc::new))
            .clone()
    };
    let summary = report.summarize(&mut read);
    let nothing =
        report.scripts.is_empty() && report.skipped.is_empty() && summary.plugins.is_empty();
    if nothing {
        return Vec::new();
    }
    if let Some(path) = path {
        let _ = std::fs::write(path, report.to_json(&summary));
    }
    let mut st = state();
    if st.summary_said {
        return Vec::new();
    }
    st.summary_said = true;
    summary.lines(&report)
}

/// For tests: forgets everything recorded.
pub fn reset() {
    let mut st = state();
    st.data = None;
    st.current = None;
    st.summary_said = false;
    st.report = Report::default();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::rc::Rc;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fork-support-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A made-up data folder: two mods linked in, one plain copy.
    fn made_up(name: &str) -> (PathBuf, PathBuf) {
        let root = scratch(name);
        let mods = root.join("mods");
        let data = root.join("Data");
        let a = mods.join("Pretend Notifier");
        let b = mods.join("Fake Reputation Icons");
        std::fs::create_dir_all(a.join("NVSE/plugins/scripts")).unwrap();
        std::fs::create_dir_all(a.join("NVSE/Plugins")).unwrap();
        std::fs::create_dir_all(b.join("NVSE/plugins/scripts")).unwrap();
        std::fs::create_dir_all(b.join("NVSE/Plugins")).unwrap();
        std::fs::create_dir_all(data.join("nvse/plugins/scripts")).unwrap();
        std::fs::create_dir_all(data.join("nvse/plugins")).unwrap();
        std::fs::write(a.join("NVSE/plugins/scripts/gl_notify.txt"), "x").unwrap();
        std::fs::write(b.join("NVSE/plugins/scripts/gl_icons.txt"), "x").unwrap();
        std::fs::write(
            a.join("NVSE/Plugins/PretendCorner.dll"),
            b"MZ\0SetPretendCornerMessage\0more",
        )
        .unwrap();
        std::fs::write(b.join("NVSE/Plugins/ShowOffNVSE.dll"), b"MZ").unwrap();
        std::fs::write(data.join("nvse/plugins/copy.dll"), b"MZ").unwrap();
        symlink(
            a.join("NVSE/plugins/scripts/gl_notify.txt"),
            data.join("nvse/plugins/scripts/gl_notify.txt"),
        )
        .unwrap();
        symlink(
            b.join("NVSE/plugins/scripts/gl_icons.txt"),
            data.join("nvse/plugins/scripts/gl_icons.txt"),
        )
        .unwrap();
        symlink(
            a.join("NVSE/Plugins/PretendCorner.dll"),
            data.join("nvse/plugins/PretendCorner.dll"),
        )
        .unwrap();
        symlink(
            b.join("NVSE/Plugins/ShowOffNVSE.dll"),
            data.join("nvse/plugins/ShowOffNVSE.dll"),
        )
        .unwrap();
        (root, data)
    }

    #[test]
    fn a_link_names_its_mod_and_a_copy_names_none() {
        let (root, data) = made_up("mods");
        assert_eq!(
            mod_of(&data, "NVSE/plugins/scripts/gl_notify.txt").as_deref(),
            Some("Pretend Notifier")
        );
        // The folder names are matched without case: the data folder has `nvse/plugins`.
        assert_eq!(
            mod_of(&data, "NVSE/Plugins/ShowOffNVSE.dll").as_deref(),
            Some("Fake Reputation Icons")
        );
        assert_eq!(mod_of(&data, "NVSE/Plugins/copy.dll"), None);
        assert_eq!(mod_of(&data, "NVSE/Plugins/nothing.dll"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_dlls_are_listed_with_their_answers() {
        let (root, data) = made_up("plugins");
        let list = list_plugins(&data);
        let names: Vec<_> = list.iter().map(|p| p.dll.as_str()).collect();
        assert_eq!(names, ["copy.dll", "PretendCorner.dll", "ShowOffNVSE.dll"]);
        assert_eq!(list[0].answer, Answer::None);
        assert_eq!(list[0].mod_name, "");
        assert_eq!(list[2].answer, Answer::Partial);
        assert_eq!(list[2].mod_name, "Fake Reputation Icons");
        let line = plugin_line(&list[1]).unwrap();
        assert!(
            line.contains("mod 'Pretend Notifier'") && line.contains("PretendCorner.dll"),
            "{line}"
        );
        assert!(
            line.contains("not loaded") && line.contains("no built-in counterpart"),
            "{line}"
        );
        assert!(plugin_line(&list[2]).unwrap().contains("only part"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn messages_are_parsed() {
        let m = "Script 'gl_b42notify.txt', line 53:\nScript command \"SetShowOffOnCornerMessageEventHandler\" not found.";
        assert_eq!(
            missing_command(m),
            Some("SetShowOffOnCornerMessageEventHandler")
        );
        assert_eq!(message_line(m), Some(53));
        assert_eq!(
            missing_command("xNVSE: Error on line 1: Invalid expression"),
            None
        );
        assert_eq!(missing_command("Script command \"x\" is fine"), None);
    }

    #[test]
    fn words_are_matched_whole_and_without_case() {
        assert!(contains_word(b"\0setfoo\0", "SetFoo"));
        assert!(!contains_word(b"\0SetFooBar\0", "SetFoo"));
        assert!(!contains_word(b"x_SetFoo\0", "SetFoo"));
        assert!(contains_word(b"SetFoo", "SetFoo"));
        assert!(!contains_word(b"", "SetFoo"));
    }

    #[test]
    fn a_run_is_recorded_summarised_and_written() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let (root, data) = made_up("run");
        set_data_dir(&data);
        let lines = scan_plugins();
        assert_eq!(lines.len(), 3, "{lines:?}");
        script_begin("NVSE/plugins/scripts/gl_notify.txt");
        let said = script_message("Script 'gl_notify.txt', line 7:\nScript command \"SetPretendCornerMessage\" not found.");
        assert_eq!(said.len(), 1);
        assert!(
            said[0].contains("mod 'Pretend Notifier'")
                && said[0].contains("gl_notify.txt, line 7")
                && said[0].contains("SetPretendCornerMessage"),
            "{}",
            said[0]
        );
        // The same message again says nothing new.
        assert!(script_message("Script command \"SetPretendCornerMessage\" not found.").is_empty());
        script_skipped("NVSE/plugins/scripts/gl_notify.txt");
        script_end();
        script_begin("NVSE/plugins/scripts/gl_icons.txt");
        script_message("Script command \"SetPretendCornerMessage\" not found.");
        script_message("Script command \"NobodyKnowsThis\" not found.");
        script_message("xNVSE: Error on line 1: Invalid expression for parameter 2.");
        script_skipped("NVSE/plugins/scripts/gl_icons.txt");
        script_end();
        // A message with no file being compiled is not recorded.
        assert!(script_message("Script command \"Stray\" not found.").is_empty());

        let path = root.join("run.mod-support.json");
        let lines = finish(Some(&path));
        let all = lines.join("\n");
        assert!(
            lines[0].contains(
                "2 plugin DLL(s) without an answer; 2 script file(s) did not compile (2 missing command(s)"
            ),
            "{all}"
        );
        assert!(
            all.contains("plugin copy.dll (mod '(unknown)'): none"),
            "{all}"
        );
        assert!(
            all.contains("plugin PretendCorner.dll (mod 'Pretend Notifier'): none; 1 missing command(s) stop 2 script file(s)"),
            "{all}"
        );
        assert!(
            all.contains("command SetPretendCornerMessage: 2 script file(s) of Fake Reputation Icons, Pretend Notifier (plugin PretendCorner.dll)"),
            "{all}"
        );
        assert!(all.contains("command NobodyKnowsThis: 1 script file(s) of Fake Reputation Icons (plugin not known)"), "{all}");
        // The second call (the exit after a stop) says nothing again but writes the file again.
        assert!(finish(Some(&path)).is_empty());

        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.starts_with("{\"version\":1,"), "{json}");
        for want in [
            "\"kind\":\"missing_command\",\"command\":\"SetPretendCornerMessage\",\"plugin\":\"PretendCorner.dll\"",
            "\"command\":\"NobodyKnowsThis\",\"plugin\":null",
            "\"kind\":\"compile_message\",\"message\":\"xNVSE: Error on line 1: Invalid expression for parameter 2.\"",
            "\"dll\":\"PretendCorner.dll\",\"mod\":\"Pretend Notifier\",\"answer\":\"none\",\"missing_commands\":1,\"scripts\":2",
            "\"skipped_script_files\":2",
        ] {
            assert!(json.contains(want), "missing {want} in {json}");
        }
        reset();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn nothing_recorded_says_nothing() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        assert!(finish(None).is_empty());
    }

    #[test]
    fn the_report_path_sits_beside_the_log() {
        assert_eq!(
            report_path(Some(Path::new("/l/run-1.jsonl")), None),
            Some(PathBuf::from("/l/run-1.mod-support.json"))
        );
        assert_eq!(
            report_path(None, Some(Path::new("/l"))),
            Some(PathBuf::from("/l/mod-support.json"))
        );
        assert_eq!(report_path(None, None), None);
    }

    #[test]
    fn an_owner_is_found_in_the_plugin_bytes() {
        let p = |n: &str| PluginFile {
            mod_name: String::new(),
            dll: n.into(),
            path: PathBuf::from(n),
            answer: Answer::None,
            note: String::new(),
        };
        let plugins = [p("a.dll"), p("b.dll")];
        let mut read = |f: &PluginFile| {
            Some(Rc::new(if f.dll == "b.dll" {
                b"..CmdX\0".to_vec()
            } else {
                b"nothing".to_vec()
            }))
        };
        assert_eq!(
            owner_of("CmdX", &plugins, &mut read).map(|p| p.dll.as_str()),
            Some("b.dll")
        );
        assert_eq!(owner_of("CmdY", &plugins, &mut read), None);
    }

    static TEST_LOCK: Mutex<()> = Mutex::new(());
}
