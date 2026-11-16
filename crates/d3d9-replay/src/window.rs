//! A window of frames cut out of a whole recording, so it replays on its own.
//!
//! `NV_D3D9_RECORD` writes every command of a run from the first (the intro movie, the menus,
//! every texture the game ever made). To replay `frames` frames of one scene, [`cut`] keeps:
//!
//! - **the setup**: every command before the window that sets state or makes, fills or changes a
//!   resource, minus what no longer matters: draws, clears and presents of the frames before the
//!   window, and each resource that was destroyed before the window began, with every command
//!   that names it; an upload a later one covers whole (a dynamic buffer rewritten each frame)
//!   is left out; the state the game set (render states, constants, binds) only from the frame
//!   before the window, which sets it all again;
//! - **the resources the window itself makes**, moved to the end of the setup, so the window can
//!   be played over and over (a create inside the window would otherwise make the object twice);
//! - **the window's frames**, without the destroys (the objects stay for the next pass) and
//!   without the commands that read back from the card.
//!
//! The result is the cut's commands in order and how many of them are the setup. Handles can be
//! reused by the game after a `Destroy`; the cut follows each object's life by the order of the
//! stream.

use d3d9_stream::{Cmd, Data, Handle};
use std::collections::HashMap;

/// The commands of a cut and how many of them are its setup.
pub struct Cut<'a> {
    pub cmds: Vec<Cmd<Data<'a>>>,
    pub setup: usize,
}

fn handles(d: &Data<'_>) -> Vec<Handle> {
    d.to_words().into_owned()
}

/// The object a creation command makes, and the objects it lists inside it (a texture's levels).
fn creation(c: &Cmd<Data<'_>>) -> Option<(Handle, Vec<Handle>)> {
    Some(match c {
        Cmd::CreateTexture {
            texture, levels, ..
        } => (*texture, handles(levels)),
        Cmd::CreateCubeTexture {
            texture, surfaces, ..
        } => (*texture, handles(surfaces)),
        Cmd::CreateSurface { surface, .. } | Cmd::CreateOffscreenSurface { surface, .. } => {
            (*surface, Vec::new())
        }
        Cmd::CreateBuffer { buffer, .. } => (*buffer, Vec::new()),
        Cmd::CreateShader { shader, .. } => (*shader, Vec::new()),
        Cmd::CreateDeclaration { declaration, .. } => (*declaration, Vec::new()),
        Cmd::CreateQuery { query, .. } => (*query, Vec::new()),
        _ => return None,
    })
}

/// The object an upload or a level change is about.
fn target(c: &Cmd<Data<'_>>) -> Option<Handle> {
    match c {
        Cmd::UploadSurface { surface, .. } => Some(*surface),
        Cmd::UploadBuffer { buffer, .. } => Some(*buffer),
        Cmd::SetLod { texture, .. } => Some(*texture),
        _ => None,
    }
}

fn is_render(c: &Cmd<Data<'_>>) -> bool {
    matches!(
        c,
        Cmd::DrawPrimitive { .. }
            | Cmd::DrawIndexedPrimitive { .. }
            | Cmd::DrawPrimitiveUp { .. }
            | Cmd::Clear { .. }
            | Cmd::Present { .. }
            | Cmd::StretchRect { .. }
            | Cmd::ColorFill { .. }
            | Cmd::BeginScene { .. }
            | Cmd::EndScene { .. }
            | Cmd::QueryIssue { .. }
            | Cmd::QueryGetData { .. }
            | Cmd::ReadSurface { .. }
            | Cmd::GetRenderTargetData { .. }
    )
}

/// The commands that set what the next draw uses and nothing else: only their last values matter
/// when a window begins, and the frame before it sets them all again.
fn is_state(c: &Cmd<Data<'_>>) -> bool {
    matches!(
        c,
        Cmd::SetRenderTarget { .. }
            | Cmd::SetDepthStencilSurface { .. }
            | Cmd::SetViewport { .. }
            | Cmd::SetTransform { .. }
            | Cmd::SetScissorRect { .. }
            | Cmd::SetClipPlane { .. }
            | Cmd::SetRenderState { .. }
            | Cmd::SetTextureStageState { .. }
            | Cmd::SetSamplerState { .. }
            | Cmd::SetTexture { .. }
            | Cmd::SetVertexDeclaration { .. }
            | Cmd::SetFvf { .. }
            | Cmd::SetShader { .. }
            | Cmd::SetConstants { .. }
            | Cmd::SetStreamSource { .. }
            | Cmd::SetIndices { .. }
            | Cmd::SetMaterial { .. }
            | Cmd::SetLight { .. }
            | Cmd::LightEnable { .. }
    )
}

/// `c` with the objects it names renamed by `alias` (an object not in it keeps its handle).
/// A payload of handles (a texture's levels) is rebuilt and kept for the life of the process: this
/// is a tool that reads one stream and ends.
fn renamed<'a>(c: &Cmd<Data<'a>>, alias: &HashMap<Handle, Handle>) -> Cmd<Data<'a>> {
    let a = |h: &Handle| alias.get(h).copied().unwrap_or(*h);
    let list = |d: &Data<'a>| -> Data<'a> {
        let words: Vec<u8> = handles(d).iter().flat_map(|h| a(h).to_le_bytes()).collect();
        Data::Bytes(Box::leak(words.into_boxed_slice()))
    };
    match *c {
        Cmd::SetRenderTarget {
            device,
            index,
            surface,
        } => Cmd::SetRenderTarget {
            device,
            index,
            surface: a(&surface),
        },
        Cmd::SetDepthStencilSurface { device, surface } => Cmd::SetDepthStencilSurface {
            device,
            surface: a(&surface),
        },
        Cmd::SetTexture {
            device,
            sampler,
            texture,
        } => Cmd::SetTexture {
            device,
            sampler,
            texture: a(&texture),
        },
        Cmd::SetVertexDeclaration {
            device,
            declaration,
        } => Cmd::SetVertexDeclaration {
            device,
            declaration: a(&declaration),
        },
        Cmd::SetShader {
            device,
            pixel,
            shader,
        } => Cmd::SetShader {
            device,
            pixel,
            shader: a(&shader),
        },
        Cmd::SetStreamSource {
            device,
            stream,
            buffer,
            offset,
            stride,
        } => Cmd::SetStreamSource {
            device,
            stream,
            buffer: a(&buffer),
            offset,
            stride,
        },
        Cmd::SetIndices { device, buffer } => Cmd::SetIndices {
            device,
            buffer: a(&buffer),
        },
        Cmd::UpdateTexture {
            device,
            source,
            destination,
        } => Cmd::UpdateTexture {
            device,
            source: a(&source),
            destination: a(&destination),
        },
        Cmd::StretchRect {
            device,
            source,
            source_rect,
            destination,
            destination_rect,
            filter,
        } => Cmd::StretchRect {
            device,
            source: a(&source),
            source_rect,
            destination: a(&destination),
            destination_rect,
            filter,
        },
        Cmd::ColorFill {
            device,
            surface,
            rect,
            colour,
        } => Cmd::ColorFill {
            device,
            surface: a(&surface),
            rect,
            colour,
        },
        Cmd::CreateTexture {
            device,
            texture,
            levels,
            width,
            height,
            usage,
            format,
            pool,
        } => Cmd::CreateTexture {
            device,
            texture: a(&texture),
            levels: list(&levels),
            width,
            height,
            usage,
            format,
            pool,
        },
        Cmd::CreateCubeTexture {
            device,
            texture,
            surfaces,
            edge,
            usage,
            format,
            pool,
        } => Cmd::CreateCubeTexture {
            device,
            texture: a(&texture),
            surfaces: list(&surfaces),
            edge,
            usage,
            format,
            pool,
        },
        Cmd::CreateSurface {
            device,
            surface,
            depth,
            width,
            height,
            format,
            multisample,
            quality,
            flag,
        } => Cmd::CreateSurface {
            device,
            surface: a(&surface),
            depth,
            width,
            height,
            format,
            multisample,
            quality,
            flag,
        },
        Cmd::CreateOffscreenSurface {
            device,
            surface,
            width,
            height,
            format,
            pool,
        } => Cmd::CreateOffscreenSurface {
            device,
            surface: a(&surface),
            width,
            height,
            format,
            pool,
        },
        Cmd::CreateBuffer {
            device,
            buffer,
            index,
            length,
            usage,
            format,
            pool,
        } => Cmd::CreateBuffer {
            device,
            buffer: a(&buffer),
            index,
            length,
            usage,
            format,
            pool,
        },
        Cmd::CreateShader {
            device,
            shader,
            pixel,
            tokens,
        } => Cmd::CreateShader {
            device,
            shader: a(&shader),
            pixel,
            tokens,
        },
        Cmd::CreateDeclaration {
            device,
            declaration,
            elements,
        } => Cmd::CreateDeclaration {
            device,
            declaration: a(&declaration),
            elements,
        },
        Cmd::CreateQuery {
            device,
            query,
            kind,
        } => Cmd::CreateQuery {
            device,
            query: a(&query),
            kind,
        },
        Cmd::Destroy { object } => Cmd::Destroy { object: a(&object) },
        Cmd::UploadSurface {
            surface,
            row,
            rows,
            data,
        } => Cmd::UploadSurface {
            surface: a(&surface),
            row,
            rows,
            data,
        },
        Cmd::UploadBuffer {
            buffer,
            offset,
            flags,
            data,
        } => Cmd::UploadBuffer {
            buffer: a(&buffer),
            offset,
            flags,
            data,
        },
        Cmd::SetLod {
            texture,
            lod,
            model,
        } => Cmd::SetLod {
            texture: a(&texture),
            lod,
            model,
        },
        Cmd::QueryIssue { query, flags } => Cmd::QueryIssue {
            query: a(&query),
            flags,
        },
        Cmd::QueryGetData { query, flags } => Cmd::QueryGetData {
            query: a(&query),
            flags,
        },
        other => other,
    }
}

/// How many frames (presents) `cmds` has.
pub fn frame_count(cmds: &[Cmd<Data<'_>>]) -> usize {
    cmds.iter()
        .filter(|c| matches!(c, Cmd::Present { .. }))
        .count()
}

/// Frames `from .. from + frames` of `all` as a cut that replays on its own.
pub fn cut<'a>(all: &[Cmd<Data<'a>>], from: usize, frames: usize) -> Result<Cut<'a>, String> {
    if frames == 0 {
        return Err("a window of no frames".into());
    }
    let presents: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, Cmd::Present { .. }))
        .map(|(i, _)| i)
        .collect();
    if from + frames > presents.len() {
        return Err(format!(
            "frames {from}..{} asked for, the recording has {}",
            from + frames,
            presents.len()
        ));
    }
    let start = if from == 0 { 0 } else { presents[from - 1] + 1 };
    let end = presents[from + frames - 1] + 1;
    if let Some(c) = all[start..end]
        .iter()
        .find(|c| matches!(c, Cmd::CreateDevice { .. } | Cmd::Reset { .. }))
    {
        return Err(format!(
            "the window holds a {}: choose frames after it",
            c.name()
        ));
    }

    // Each object's life: an incarnation number for every creation, by order. `live` maps a
    // handle (an object or a level inside one) to its incarnation now.
    let mut live: HashMap<Handle, usize> = HashMap::new();
    let mut owner_of: HashMap<usize, Handle> = HashMap::new();
    let mut incarnation = vec![usize::MAX; all.len()];
    let mut dead_by_start = std::collections::HashSet::new();
    let mut next = 0usize;
    for (i, c) in all[..end].iter().enumerate() {
        if let Some((h, inner)) = creation(c) {
            next += 1;
            live.insert(h, next);
            owner_of.insert(next, h);
            for l in inner {
                live.insert(l, next);
            }
            incarnation[i] = next;
        } else if let Some(h) = target(c) {
            if let Some(&n) = live.get(&h) {
                incarnation[i] = n;
            }
        } else if let Cmd::Destroy { object } = c {
            if let Some(&n) = live.get(object) {
                incarnation[i] = n;
                if owner_of.get(&n) == Some(object) {
                    live.remove(object);
                    if i < start {
                        dead_by_start.insert(n);
                    }
                }
            }
        }
    }
    let dead = |i: usize| dead_by_start.contains(&incarnation[i]);

    // Uploads a later one covers whole: a buffer range or a whole level written again.
    let mut covered = vec![false; start];
    {
        let mut ranges: HashMap<Handle, Vec<(u32, u32)>> = HashMap::new();
        let mut levels = std::collections::HashSet::new();
        for i in (0..start).rev() {
            match &all[i] {
                Cmd::UploadBuffer {
                    buffer,
                    offset,
                    data,
                    ..
                } => {
                    let r = ranges.entry(*buffer).or_default();
                    let (a, b) = (*offset, offset + data.len());
                    if r.iter().any(|&(x, y)| x <= a && b <= y) {
                        covered[i] = true;
                    } else {
                        r.push((a, b));
                    }
                }
                Cmd::UploadSurface { surface, .. } => {
                    if !levels.insert(*surface) {
                        covered[i] = true;
                    }
                }
                // A creation ends the look back for its object's uploads.
                c => {
                    if let Some((h, inner)) = creation(c) {
                        ranges.remove(&h);
                        levels.remove(&h);
                        for l in inner {
                            levels.remove(&l);
                        }
                    }
                }
            }
        }
    }

    // State is kept from the frame before the window only.
    let state_from = if from >= 2 { presents[from - 2] + 1 } else { 0 };
    let mut setup = Vec::new();
    for (i, c) in all[..start].iter().enumerate() {
        if is_render(c) || dead(i) || covered[i] || (is_state(c) && i < state_from) {
            continue;
        }
        setup.push(*c);
    }
    // What the window makes. A handle the game reuses (an address freed and made again, or one
    // alive at the window's start and made again after its destroy, which a pass over the window
    // does not repeat) names a new object each time: the later ones get handles of their own,
    // in every command after.
    let mut known: std::collections::HashSet<Handle> = live_at_start(&setup);
    let mut alias: HashMap<Handle, Handle> = HashMap::new();
    let mut fresh = 0x7000_0000u32;
    let mut body = Vec::new();
    for c in &all[start..end] {
        if let Some((h, inner)) = creation(c) {
            for x in std::iter::once(h).chain(inner) {
                if known.contains(&x) {
                    fresh += 1;
                    alias.insert(x, fresh);
                } else {
                    alias.remove(&x);
                }
                known.insert(alias.get(&x).copied().unwrap_or(x));
                known.insert(x);
            }
            setup.push(renamed(c, &alias));
            continue;
        }
        if matches!(
            c,
            Cmd::Destroy { .. } | Cmd::ReadSurface { .. } | Cmd::GetRenderTargetData { .. }
        ) {
            continue;
        }
        body.push(renamed(c, &alias));
    }
    let setup_len = setup.len();
    setup.extend(body);
    Ok(Cut {
        cmds: setup,
        setup: setup_len,
    })
}

/// The handles of the objects the setup made so far.
fn live_at_start(setup: &[Cmd<Data<'_>>]) -> std::collections::HashSet<Handle> {
    let mut v = std::collections::HashSet::new();
    for c in setup {
        if let Some((h, inner)) = creation(c) {
            v.insert(h);
            v.extend(inner);
        } else if let Cmd::CreateDevice { back_buffers, .. } = c {
            v.extend(handles(back_buffers));
        }
    }
    v
}

/// The wire file of `cmds`.
pub fn encode(cmds: &[Cmd<Data<'_>>]) -> Vec<u8> {
    let mut out = d3d9_stream::wire::header().to_vec();
    for c in cmds {
        d3d9_stream::wire::encode(c, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::{stream, Scene};

    fn parse(bytes: &[u8]) -> Vec<Cmd<Data<'_>>> {
        d3d9_stream::wire::commands(bytes)
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_window_keeps_its_frames_and_the_setup_and_replays_alone() {
        let s = Scene {
            frames: 6,
            ..Scene::small()
        };
        let bytes = stream(&s);
        let all = parse(&bytes);
        assert_eq!(frame_count(&all), 6);
        let c = cut(&all, 2, 3).unwrap();
        // Three presents, and none in the setup; draws only in the frames.
        assert_eq!(frame_count(&c.cmds), 3);
        assert!(!c.cmds[..c.setup].iter().any(is_render));
        let draws = c.cmds[c.setup..]
            .iter()
            .filter(|c| matches!(c, Cmd::DrawIndexedPrimitive { .. }))
            .count();
        assert_eq!(draws as u32, 3 * s.draws);
        // The setup's resources are the synthetic scene's.
        assert!(c.cmds[..c.setup]
            .iter()
            .any(|c| matches!(c, Cmd::CreateDevice { .. })));
        // The window is a stream: it encodes, reads back and counts the same.
        let out = encode(&c.cmds);
        let back = crate::replay::Replay::parse(&out, c.setup as u64).unwrap();
        assert_eq!(back.frames.len(), 3);
    }

    #[test]
    fn destroyed_resources_and_covered_uploads_are_left_out() {
        let s = Scene::small();
        let bytes = stream(&s);
        let mut all = parse(&bytes);
        // A buffer made, filled twice and destroyed before the window; another filled twice and
        // kept.
        let junk = [0u8; 16];
        let at = all
            .iter()
            .position(|c| matches!(c, Cmd::Clear { .. }))
            .unwrap();
        let mk = |b: u32| Cmd::CreateBuffer {
            device: 0x1000,
            buffer: b,
            index: false,
            length: 16,
            usage: 0,
            format: 0,
            pool: 1,
        };
        let up = |b: u32| Cmd::UploadBuffer {
            buffer: b,
            offset: 0,
            flags: 0,
            data: Data::Bytes(&junk),
        };
        all.splice(
            at..at,
            [
                mk(0x9000),
                up(0x9000),
                mk(0x9001),
                up(0x9001),
                up(0x9001),
                Cmd::Destroy { object: 0x9000 },
            ],
        );
        let c = cut(&all, 1, 2).unwrap();
        let named = |h: u32| {
            c.cmds
                .iter()
                .filter(|c| match c {
                    Cmd::CreateBuffer { buffer, .. } | Cmd::UploadBuffer { buffer, .. } => {
                        *buffer == h
                    }
                    Cmd::Destroy { object } => *object == h,
                    _ => false,
                })
                .count()
        };
        assert_eq!(
            named(0x9000),
            0,
            "the destroyed buffer is gone with its commands"
        );
        assert_eq!(named(0x9001), 2, "one creation and the last upload");
    }

    #[test]
    fn a_handle_made_again_inside_the_window_names_a_new_object() {
        let s = Scene {
            frames: 5,
            ..Scene::small()
        };
        let bytes = stream(&s);
        let mut all = parse(&bytes);
        // In frame 3 (the window's second): the first texture is destroyed, made again at the same
        // address, and bound.
        let present = all
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Cmd::Present { .. }))
            .map(|(i, _)| i)
            .nth(1)
            .unwrap();
        let level = 0x3000u32.to_le_bytes();
        let level: &[u8] = Box::leak(Box::new(level));
        all.splice(
            present + 2..present + 2,
            [
                Cmd::Destroy { object: 0x2000 },
                Cmd::CreateTexture {
                    device: 0x1000,
                    texture: 0x2000,
                    levels: Data::Bytes(level),
                    width: 8,
                    height: 8,
                    usage: 0,
                    format: 21,
                    pool: 1,
                },
                Cmd::SetTexture {
                    device: 0x1000,
                    sampler: 0,
                    texture: 0x2000,
                },
            ],
        );
        let c = cut(&all, 2, 2).unwrap();
        let made: Vec<u32> = c.cmds[..c.setup]
            .iter()
            .filter_map(|c| match c {
                Cmd::CreateTexture { texture, .. } => Some(*texture),
                _ => None,
            })
            .collect();
        // The original (alive at the start) and the new one under a handle of its own.
        assert_eq!(made.iter().filter(|&&h| h == 0x2000).count(), 1);
        assert_eq!(made.iter().filter(|&&h| h >= 0x7000_0000).count(), 1);
        // The binding after the new creation names the new one; no destroy is left in the frames.
        let bound: Vec<u32> = c.cmds[c.setup..]
            .iter()
            .filter_map(|c| match c {
                Cmd::SetTexture { texture, .. } if *texture >= 0x7000_0000 => Some(*texture),
                _ => None,
            })
            .collect();
        assert!(!bound.is_empty());
        assert!(!c.cmds[c.setup..]
            .iter()
            .any(|c| matches!(c, Cmd::Destroy { .. })));
    }

    #[test]
    fn a_window_past_the_end_or_over_a_reset_is_refused() {
        let bytes = stream(&Scene::small());
        let all = parse(&bytes);
        assert!(cut(&all, 3, 2).is_err());
        assert!(cut(&all, 0, 0).is_err());
        assert!(
            cut(&all, 0, 1).is_err(),
            "the first frame holds the device's creation"
        );
    }
}
