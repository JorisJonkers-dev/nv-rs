// SPDX-License-Identifier: GPL-3.0-only
//! Direct3D 9 on wgpu: how much of the device interface and of its render states the port
//! carries out on wgpu (Vulkan on Linux and Windows, Metal on macOS; ADR-0007), against the full
//! lists of a public `d3d9.h` / `d3d9types.h` (mingw-w64, pinned in [`crate::pins`]).
//!
//! - A device method is *answered* when the platform's device (`engine-platform`'s
//!   `d3d9device.rs`) registers its vtable slot, and *carried to wgpu* when, besides, a command
//!   of the recorded stream (`d3d9-stream`'s `commands!`) names it in its documentation and the
//!   wgpu backend (`d3d9-wgpu`'s `lib.rs`) handles that command.
//! - A render state is *mapped* when d3d9-wgpu's code (tests left out) uses its constant outside
//!   a "not provided" warning, *warned* when every use is such a warning, *ignored* when no code
//!   names it.

use std::collections::BTreeSet;

/// `IDirect3DDevice9`'s methods in vtable order (`STDMETHOD(Name)` and `STDMETHOD_(type, Name)`).
pub fn device_methods(d3d9_h: &str) -> Result<Vec<String>, String> {
    let start = d3d9_h
        .find("DECLARE_INTERFACE_IID_(IDirect3DDevice9,")
        .or_else(|| d3d9_h.find("DECLARE_INTERFACE_(IDirect3DDevice9,"))
        .ok_or("d3d9.h: no IDirect3DDevice9")?;
    let body = &d3d9_h[start..];
    let end = body
        .find("\n};")
        .ok_or("d3d9.h: IDirect3DDevice9 has no end")?;
    let mut out = Vec::new();
    for line in body[..end].lines() {
        let t = line.trim_start();
        let name = if let Some(r) = t.strip_prefix("STDMETHOD_(") {
            r.split_once(',').map(|(_, n)| n)
        } else {
            t.strip_prefix("STDMETHOD(")
        };
        if let Some(n) = name {
            let n: String = n
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !n.is_empty() {
                out.push(n);
            }
        }
    }
    Ok(out)
}

/// `D3DRENDERSTATETYPE`'s values (`D3DRS_NAME = n`), without the `FORCE_DWORD` sentinel.
pub fn render_states(d3d9types_h: &str) -> Result<Vec<(String, u32)>, String> {
    let start = d3d9types_h
        .find("_D3DRENDERSTATETYPE")
        .ok_or("d3d9types.h: no D3DRENDERSTATETYPE")?;
    let body = &d3d9types_h[start..];
    let end = body
        .find('}')
        .ok_or("d3d9types.h: D3DRENDERSTATETYPE has no end")?;
    let mut out = Vec::new();
    for line in body[..end].lines() {
        let t = line.trim();
        let Some(r) = t.strip_prefix("D3DRS_") else {
            continue;
        };
        let Some((name, value)) = r.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let v = value.trim().trim_end_matches(',').trim();
        let v = v.split_whitespace().next().unwrap_or("");
        let n = if let Some(h) = v.strip_prefix("0x") {
            u32::from_str_radix(h, 16)
        } else {
            v.parse()
        };
        if name == "FORCE_DWORD" {
            continue;
        }
        if let Ok(n) = n {
            out.push((name.to_string(), n));
        }
    }
    Ok(out)
}

/// The device slots the platform registers: the entries of the arrays of `register_code` that
/// are registered at `DEVICE_CODE + 4 * slot`. Each array's entry count is checked against its
/// declared length, so a reading that misses an entry stops the run.
pub fn answered_slots(d3d9device_rs: &str) -> Result<BTreeSet<u32>, String> {
    let start = d3d9device_rs
        .find("fn register_code(")
        .ok_or("d3d9device.rs: no register_code")?;
    let body = &d3d9device_rs[start..];
    let end = body.find("\n}\n").unwrap_or(body.len());
    let body = &body[..end];
    let mut out = BTreeSet::new();
    let decls: Vec<usize> = body.match_indices("    let ").map(|(i, _)| i).collect();
    for (k, &d) in decls.iter().enumerate() {
        let seg = &body[d..decls.get(k + 1).copied().unwrap_or(body.len())];
        if !seg.contains("DEVICE_CODE + 4 * slot") {
            continue;
        }
        let Some(len) = seg
            .split_once("crate::AbiFn); ")
            .and_then(|(_, r)| r.split(']').next())
            .and_then(|n| n.trim().parse::<usize>().ok())
        else {
            continue;
        };
        let slots = tuple_heads(seg);
        if slots.len() != len {
            return Err(format!(
                "d3d9device.rs: an array of {len} device slots read as {}",
                slots.len()
            ));
        }
        out.extend(slots);
    }
    if out.is_empty() {
        return Err("d3d9device.rs: no device slot read".into());
    }
    Ok(out)
}

/// The first number of each top-level tuple of the array literal in `seg` (`= [ (n, ...), ... ]`).
fn tuple_heads(seg: &str) -> Vec<u32> {
    let Some(open) = seg.find("= [") else {
        return Vec::new();
    };
    let b: Vec<char> = seg[open + 3..].chars().collect();
    let (mut parens, mut out) = (0i32, Vec::new());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            '(' => {
                if parens == 0 {
                    let mut j = i + 1;
                    while j < b.len() && b[j].is_whitespace() {
                        j += 1;
                    }
                    let digits: String = b[j..].iter().take_while(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = digits.parse() {
                        out.push(n);
                    }
                }
                parens += 1;
            }
            ')' => parens -= 1,
            ']' if parens == 0 => break,
            _ => {}
        }
        i += 1;
    }
    out
}

/// `{a,b}` expanded (`Set{Vertex,Pixel}Shader` is two names).
fn expand(word: &str) -> Vec<String> {
    let Some(open) = word.find('{') else {
        return vec![word.to_string()];
    };
    let Some(close) = word[open..].find('}').map(|c| c + open) else {
        return vec![word.to_string()];
    };
    let (head, alts, tail) = (&word[..open], &word[open + 1..close], &word[close + 1..]);
    alts.split(',')
        .flat_map(|a| expand(&format!("{head}{a}{tail}")))
        .collect()
}

/// The stream's commands, each with the device methods its documentation names in backticks.
pub fn stream_commands(cmd_rs: &str, methods: &[String]) -> Vec<(String, BTreeSet<String>)> {
    let known: BTreeSet<String> = methods.iter().map(|m| m.to_lowercase()).collect();
    let Some(start) = cmd_rs.find("commands! {") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut doc = String::new();
    for line in cmd_rs[start..].lines().skip(1) {
        if line.starts_with('}') {
            break;
        }
        let t = line.trim_start();
        if let Some(d) = t.strip_prefix("///") {
            doc.push_str(d);
            doc.push(' ');
            continue;
        }
        let indent = line.len() - t.len();
        let name: String = t
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if indent == 4 && !name.is_empty() && t[name.len()..].trim_start().starts_with('{') {
            let mut named = BTreeSet::new();
            for (i, part) in doc.split('`').enumerate() {
                if i % 2 == 0 {
                    continue;
                }
                let word = part.split('(').next().unwrap_or("");
                let word = word.rsplit("::").next().unwrap_or(word);
                for w in expand(word) {
                    if known.contains(&w.to_lowercase()) {
                        named.insert(w.to_lowercase());
                    }
                }
            }
            if known.contains(&name.to_lowercase()) {
                named.insert(name.to_lowercase());
            }
            out.push((name, named));
        }
        if !t.starts_with("///") && !t.is_empty() && !t.starts_with("//") && indent == 4 {
            doc.clear();
        }
    }
    out
}

/// The commands a backend's source handles (`Cmd::Name` anywhere in it).
pub fn handled(backend_rs: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, _) in backend_rs.match_indices("Cmd::") {
        let n: String = backend_rs[i + 5..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if n.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            out.insert(n);
        }
    }
    out
}

/// A source without its inline test module (`#[cfg(test)]` then `mod name {`, to the end; a
/// `#[cfg(test)] mod name;` naming another file is kept).
pub fn without_tests(text: &str) -> &str {
    let k = crate::extenders::inline_tests_at(text).unwrap_or(text.len());
    &text[..k]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateUse {
    Mapped,
    Warned,
    Ignored,
}

/// How the backend's code treats one render state, by its constant `rs::NAME` or `RS_NAME`.
pub fn state_use(code: &[&str], name: &str) -> StateUse {
    let pats = [format!("rs::{name}"), format!("RS_{name}")];
    let mut warned = false;
    for text in code {
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let used = pats.iter().any(|p| has_word(l, p));
            let defines = l.contains("const ") && l.contains(": usize =");
            if !used || defines {
                continue;
            }
            let ctx = lines[i..(i + 3).min(lines.len())].join(" ");
            if ctx.contains("not provided") {
                warned = true;
            } else {
                return StateUse::Mapped;
            }
        }
    }
    if warned {
        StateUse::Warned
    } else {
        StateUse::Ignored
    }
}

fn has_word(line: &str, word: &str) -> bool {
    line.match_indices(word).any(|(i, _)| {
        let after = line[i + word.len()..].chars().next();
        !after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            && !line[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "\
#undef INTERFACE
#define INTERFACE IDirect3DDevice9
DECLARE_INTERFACE_IID_(IDirect3DDevice9,IUnknown,\"d0223b96\")
{
    STDMETHOD(QueryInterface)(THIS_ REFIID riid, void** ppvObj) PURE;
    STDMETHOD_(ULONG,AddRef)(THIS) PURE;
    STDMETHOD(SetRenderState)(THIS_ D3DRENDERSTATETYPE State,DWORD Value) PURE;
    STDMETHOD(SetVertexShader)(THIS_ IDirect3DVertexShader9* pShader) PURE;
    STDMETHOD(SetPixelShader)(THIS_ IDirect3DPixelShader9* pShader) PURE;
};
";

    #[test]
    fn methods_and_states_from_the_headers() {
        let m = device_methods(H).unwrap();
        assert_eq!(
            m,
            [
                "QueryInterface",
                "AddRef",
                "SetRenderState",
                "SetVertexShader",
                "SetPixelShader"
            ]
        );
        let t = "typedef enum _D3DRENDERSTATETYPE {\n    D3DRS_ZENABLE = 7,\n    D3DRS_FOGENABLE = 28,\n    D3DRS_FORCE_DWORD = 0x7fffffff\n} D3DRENDERSTATETYPE;\n";
        assert_eq!(
            render_states(t).unwrap(),
            [("ZENABLE".to_string(), 7), ("FOGENABLE".to_string(), 28)]
        );
    }

    #[test]
    fn slots_are_read_and_counted() {
        let src = "fn register_code(e: &mut Engine) {
    let device: [(u32, crate::AbiFn); 3] = [
        (16, crate::entry!(0, reset(Ptr, Ptr) -> u32).1),
        (
            57,
            WRITER.render_state.0,
        ),
        (
            32,
            crate::entry!(0, get_data(Ptr, u32) -> u32).1,
        ),
    ];
    for (slot, f) in device {
        e.register(DEVICE_CODE + 4 * slot, f);
    }
    let surface: [(u32, crate::AbiFn); 1] = [(3, crate::entry!(0, s(Ptr) -> u32).1)];
    for (slot, f) in surface {
        e.register(SURFACE_CODE + 4 * slot, f);
    }
}
";
        assert_eq!(answered_slots(src).unwrap(), BTreeSet::from([16, 32, 57]));
        let short = src.replace("; 3] =", "; 4] =");
        assert!(
            answered_slots(&short).is_err(),
            "an entry not read stops the count"
        );
    }

    #[test]
    fn commands_name_their_methods() {
        let m = device_methods(H).unwrap();
        let cmd = "commands! {
    // The device.

    /// `SetRenderState(state, value)`.
    SetRenderState { device: Handle, state: u32, value: u32 },
    /// `SetVertexShader` (`pixel` false) or `SetPixelShader`; 0 unbinds.
    SetShader { device: Handle, pixel: bool, shader: Handle },
    /// A port-only command.
    Marker {
        device: Handle,
    },
}
";
        let c = stream_commands(cmd, &m);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].1, BTreeSet::from(["setrenderstate".to_string()]));
        assert_eq!(
            c[1].1,
            BTreeSet::from(["setpixelshader".to_string(), "setvertexshader".to_string()])
        );
        assert!(c[2].1.is_empty());
        assert_eq!(expand("Set{Vertex,Pixel}ShaderConstant{F,I}").len(), 4);
        let h = handled("match cmd { Cmd::SetShader { .. } => {}, Cmd::Present { frame, .. } => d.present(frame), _ => {} }");
        assert_eq!(
            h,
            BTreeSet::from(["Present".to_string(), "SetShader".to_string()])
        );
    }

    #[test]
    fn states_mapped_warned_or_ignored() {
        let code = "pub mod rs {\n    pub const ZENABLE: usize = 7;\n    pub const FOGENABLE: usize = 28;\n    pub const DITHER: usize = 26;\n}\nfn f(s: &[u32]) -> bool { s[rs::ZENABLE] != 0 }\nmatch st {\n    rs::FOGENABLE if value != 0 => {\n        self.warn(\"D3DRS_FOGENABLE is not provided\")\n    }\n}\n";
        assert_eq!(state_use(&[code], "ZENABLE"), StateUse::Mapped);
        assert_eq!(state_use(&[code], "FOGENABLE"), StateUse::Warned);
        assert_eq!(state_use(&[code], "DITHER"), StateUse::Ignored);
        assert_eq!(
            state_use(&[code], "ZENABL"),
            StateUse::Ignored,
            "whole names only"
        );
        assert_eq!(
            without_tests("a\n#[cfg(test)]\nmod other;\nb\n#[cfg(test)]\nmod tests {}\n"),
            "a\n#[cfg(test)]\nmod other;\nb\n"
        );
    }
}
