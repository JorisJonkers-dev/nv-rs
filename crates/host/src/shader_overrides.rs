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

/// Who asks for the near water's fade, for the log.
pub const NEAR_WATER_FADE: &str = "water.near_fade";

/// The constants a water pixel shader must name for the near water's fade to fit it: those with a
/// depth map (the water's depth under its surface), whose `FogColor.w` scales the water's opacity
/// over what lies under it and whose `DepthFalloff` turns the depth into that opacity.
pub const NEAR_WATER_FADE_READS: [&str; 4] = ["EyePos", "FogColor", "DepthFalloff", "DepthMap"];

/// The near water's fade (`water.near_fade`, nv-rs#897): a patch of the game's own water shaders
/// (`d3d9_wgpu::sm::wgsl::convert_patched`). Across the ground from the eye, between 4096 and 8192
/// units (one and two cells: the loaded cells of the default `uGridsToLoad` 5 reach at least two
/// cells from the viewer), the water's opacity (`FogColor.w`) rises to 1 and the depth's falloff
/// reads every depth as deep, so near water meets the far (LOD) water, which shows nothing of what
/// lies under it, with no line where the loaded cells end. Nearer, the water is the game's.
pub const NEAR_WATER_FADE_PROGRAM: &str = "// nv: patch
// The near water's clarity fades toward the edge of the loaded cells (water.near_fade, nv-rs#897).
fn nv_patch_c(n: i32, v: vec4<f32>) -> vec4<f32> {
    let k = smoothstep(4096.0, 8192.0, length(nv_const_EyePos().xy - nv_texcoord[0].xy));
    if (n == nv_reg_FogColor) {
        return vec4<f32>(v.xyz, mix(v.w, 1.0, k));
    }
    if (n == nv_reg_DepthFalloff) {
        return vec4<f32>(mix(v.x, -1.0, k), mix(v.y, 0.0, k), v.zw);
    }
    return v;
}
";

/// The specs of the near water's fade for the shader packages of `data`: one per water pixel
/// shader that reads [`NEAR_WATER_FADE_READS`].
pub fn near_water_fade(data: &Path) -> Vec<Spec> {
    shader_override::names_naming(&[data.to_path_buf()], "WATER", &NEAR_WATER_FADE_READS)
        .into_iter()
        .map(|name| Spec::of(&name, NEAR_WATER_FADE_PROGRAM, NEAR_WATER_FADE))
        .collect()
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

    /// A water pixel shader's tokens as the game's are laid out where the fade reads them: `ps_3_0`
    /// with a constant table naming `EyePos` (c1), `FogColor` (c10), `DepthFalloff` (c11) and the
    /// `DepthMap` sampler (s4), a texture coordinate, and `add r0, c10, c11; add oC0, r0, c1`.
    fn water_tokens() -> Vec<u32> {
        let names: [(&str, u16); 4] = [
            ("EyePos", 1),
            ("FogColor", 10),
            ("DepthFalloff", 11),
            ("DepthMap", 4),
        ];
        let mut b = Vec::new();
        for v in [28u32, 0, 0xFFFF_0300, names.len() as u32, 28, 0, 0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let strings = 28 + 20 * names.len();
        let mut text = Vec::new();
        for (name, index) in names {
            b.extend_from_slice(&((strings + text.len()) as u32).to_le_bytes());
            for v in [if name.ends_with("Map") { 3u16 } else { 2 }, index, 1, 0] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&[0; 8]);
            text.extend_from_slice(name.as_bytes());
            text.push(0);
        }
        b.extend(text);
        b.resize(b.len().div_ceil(4) * 4, 0);
        let words: Vec<u32> = std::iter::once(u32::from_le_bytes(*b"CTAB"))
            .chain(
                b.chunks_exact(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
            )
            .collect();
        let mut t = vec![0xFFFF_0300, 0xFFFE | ((words.len() as u32) << 16)];
        t.extend(words);
        // dcl_texcoord0 v0; add r0, c10, c11; add oC0, r0, c1.
        t.extend([
            0x0200_001F,
            0x8000_0005,
            0x900F_0000,
            0x0300_0002,
            0x800F_0000,
            0xA0E4_000A,
            0xA0E4_000B,
            0x0300_0002,
            0x800F_0800,
            0x80E4_0000,
            0xA0E4_0001,
            0x0000_FFFF,
        ]);
        t
    }

    /// The near water's fade is a patch, and its program is valid WGSL for a water shader that
    /// reads what it needs: it patches the opacity (`FogColor`) and the depth's falloff
    /// (`DepthFalloff`) reads, and the eye's position stays the game's.
    #[test]
    fn the_near_water_fade_patches_the_water_s_opacity_and_depth_falloff() {
        use d3d9_wgpu::sm::{self, wgsl};
        let spec = Spec::of("WATER000.pso", NEAR_WATER_FADE_PROGRAM, NEAR_WATER_FADE);
        assert!(spec.patch && !spec.late && !spec.blend, "{spec:?}");
        let p = sm::parse(&water_tokens()).unwrap();
        for c in NEAR_WATER_FADE_READS {
            assert!(p.constants.iter().any(|k| k.name == c), "{c}");
        }
        let link = wgsl::Link {
            varyings: vec![wgsl::Semantic {
                usage: sm::usage::TEXCOORD,
                index: 0,
            }],
            ..wgsl::Link::default()
        };
        let s = wgsl::convert_patched(&p, &link, NEAR_WATER_FADE_PROGRAM).unwrap();
        sm::validate(&s).unwrap_or_else(|e| panic!("{e}\n{s}"));
        assert!(s.contains("const nv_reg_FogColor: i32 = 10;"), "{s}");
        assert!(s.contains("const nv_reg_DepthFalloff: i32 = 11;"), "{s}");
        assert!(
            s.contains("nv_patch_c(10, ") && s.contains("nv_patch_c(11, "),
            "{s}"
        );
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
