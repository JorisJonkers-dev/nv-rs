// SPDX-License-Identifier: GPL-3.0-only
//! The run's shader overrides (`d3d9_wgpu::shader_override`): what the graphics settings ask for,
//! and the engineers' folder `NV_SHADER_OVERRIDES` (each `<shader name>.wgsl` in it, such as
//! `WATER033.pso.wgsl`, replaces that shader: for trying a program without a build; its first
//! `// nv:` lines say how it is drawn, `d3d9_wgpu::shader_override::Spec::of`). Resolved
//! against the game's shader packages in the data folder and set before the device is made.
//! Nothing is set without either: the device is then what it was.

use d3d9_wgpu::shader_override::{self, Spec};
use std::path::Path;

/// The engineers' folder of override programs.
pub const ENV: &str = "NV_SHADER_OVERRIDES";

/// The programs in `dir`: each `<shader name>.wgsl` (the name as the packages give it, its
/// extension included), reading any float constant; and a line for each file that cannot be read.
pub fn specs_in(dir: &Path) -> (Vec<Spec>, Vec<String>) {
    let mut specs = Vec::new();
    let mut lines = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        lines.push(format!(
            "nv-rs: {ENV}: {} is not a folder that can be read (no shader replaced)",
            dir.display()
        ));
        return (specs, lines);
    };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("wgsl"))
        })
        .collect();
    files.sort();
    for f in files {
        let Some(shader) = f.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        match std::fs::read_to_string(&f) {
            Ok(source) => specs.push(Spec::of(&shader, &source, ENV)),
            Err(e) => lines.push(format!("nv-rs: {ENV}: {}: {e}", f.display())),
        }
    }
    (specs, lines)
}

/// Sets the run's overrides: `specs` (the graphics settings') and the folder `env` names, a
/// folder's program for a shader winning over a setting's; resolved against the packages of
/// `data`. The log's lines.
pub fn install(
    data: &Path,
    mut specs: Vec<Spec>,
    env: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(dir) = env(ENV).filter(|d| !d.trim().is_empty()) {
        let (own, notes) = specs_in(Path::new(dir.trim()));
        lines.extend(notes);
        specs.retain(|s| !own.iter().any(|o| o.shader.eq_ignore_ascii_case(&s.shader)));
        specs.extend(own);
    }
    if specs.is_empty() {
        shader_override::set(Vec::new());
        return lines;
    }
    let (overrides, notes) = shader_override::resolve(&[data.to_path_buf()], &specs);
    lines.extend(notes);
    shader_override::set(overrides);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("nv-host-overrides-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Each `.wgsl` file is the program of the shader its name gives; other files are not read.
    #[test]
    fn a_folder_s_programs_are_named_by_their_files() {
        let d = tmp("folder");
        std::fs::write(d.join("WATER033.pso.wgsl"), "fn nv_main() {}").unwrap();
        std::fs::write(d.join("notes.txt"), "not a program").unwrap();
        let (specs, lines) = specs_in(&d);
        assert!(lines.is_empty(), "{lines:?}");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].shader, "WATER033.pso");
        assert_eq!(specs[0].source, "fn nv_main() {}");
        assert_eq!(specs[0].registers, d3d9_wgpu::sm::wgsl::FLOAT_CONSTANTS);
        let (none, lines) = specs_in(&d.join("missing"));
        assert!(
            none.is_empty() && lines[0].contains("no shader replaced"),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
