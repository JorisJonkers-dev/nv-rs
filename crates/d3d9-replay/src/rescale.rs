//! The same stream at another render target size, without recording again.
//!
//! A scene is recorded at 640x480 at most. Its draw and state counts are what the backends are
//! measured by, but how the card copes with a larger target (fill rate, bandwidth, the clears and
//! the post passes) is wanted too. [`rescale`] rewrites the commands that carry a target's size:
//! the back buffers (`CreateDevice`, `Reset`), render targets and depth surfaces and render
//! target textures whose size is the screen's or a half, quarter, eighth or sixteenth of it
//! (bloom and blur chains), and, while a surface of those is the render target, the viewport, the
//! scissor rectangle, clear rectangles and the rectangles of `StretchRect` and `ColorFill`. What
//! is not screen sized (shadow maps, lookup textures) stays as recorded. Shader constants that
//! hold the screen's size (pixel offsets of a blur, the `vPos` scale) and `DrawPrimitiveUP`
//! vertices in pixels stay as recorded, so the picture is not the game's at the new size: the
//! work is (more pixels filled, bigger targets), which is what it is for.

use d3d9_stream::{wire, Cmd, Data, Handle};
use std::collections::HashSet;

/// A target size, and the ratio from the recorded one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    from: (u32, u32),
    to: (u32, u32),
}

impl Scale {
    pub fn new(from: (u32, u32), to: (u32, u32)) -> Scale {
        Scale { from, to }
    }

    fn x(&self, v: u32) -> u32 {
        ((u64::from(v) * u64::from(self.to.0)).div_ceil(u64::from(self.from.0.max(1)))) as u32
    }

    fn y(&self, v: u32) -> u32 {
        ((u64::from(v) * u64::from(self.to.1)).div_ceil(u64::from(self.from.1.max(1)))) as u32
    }

    /// Whether a surface of this size is the screen's or a fraction of it.
    fn screen_sized(&self, w: u32, h: u32) -> bool {
        [1u32, 2, 4, 8, 16]
            .iter()
            .any(|&k| w == self.from.0.div_ceil(k) && h == self.from.1.div_ceil(k))
    }
}

/// The back buffer's size in the stream's first `CreateDevice`.
pub fn back_buffer_size(cmds: &[Cmd<Data<'_>>]) -> Option<(u32, u32)> {
    cmds.iter().find_map(|c| match c {
        Cmd::CreateDevice { present, .. } => {
            let w = present.to_words();
            (w.len() >= 2).then(|| (w[0], w[1]))
        }
        _ => None,
    })
}

fn handles(d: &Data<'_>) -> Vec<Handle> {
    d.to_words().into_owned()
}

fn rect_scaled(s: &Scale, d: &Data<'_>) -> Vec<u8> {
    let w = d.to_words();
    let mut out = Vec::with_capacity(w.len() * 4);
    for r in w.chunks(4) {
        if r.len() == 4 {
            for v in [s.x(r[0]), s.y(r[1]), s.x(r[2]), s.y(r[3])] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

fn present_scaled(s: &Scale, d: &Data<'_>) -> Vec<u8> {
    let mut w = d.to_words().into_owned();
    if w.len() >= 2 {
        w[0] = s.to.0;
        w[1] = s.to.1;
    }
    w.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// `cmds` as a wire file with the screen-sized targets at `s.to`.
pub fn rescale(cmds: &[Cmd<Data<'_>>], s: &Scale) -> Vec<u8> {
    let mut out = wire::header().to_vec();
    // The surfaces (and the levels of textures) at the new size, and the render target now.
    let mut scaled: HashSet<Handle> = HashSet::new();
    let mut now_scaled = false;
    let mut rt: Handle = 0;
    for c in cmds {
        let mut put = |c: &Cmd<Data<'_>>| wire::encode(c, &mut out);
        match c {
            Cmd::CreateDevice {
                device,
                focus,
                window,
                behavior,
                present,
                back_buffers,
                depth_stencil,
            } => {
                scaled.extend(handles(back_buffers));
                if *depth_stencil != 0 {
                    scaled.insert(*depth_stencil);
                }
                // The device's own render target is its first back buffer.
                rt = handles(back_buffers).first().copied().unwrap_or(0);
                now_scaled = true;
                let p = present_scaled(s, present);
                put(&Cmd::CreateDevice {
                    device: *device,
                    focus: *focus,
                    window: *window,
                    behavior: *behavior,
                    present: Data::Bytes(&p),
                    back_buffers: *back_buffers,
                    depth_stencil: *depth_stencil,
                });
            }
            Cmd::Reset {
                device,
                window,
                present,
                back_buffers,
                depth_stencil,
            } => {
                scaled.extend(handles(back_buffers));
                if *depth_stencil != 0 {
                    scaled.insert(*depth_stencil);
                }
                rt = handles(back_buffers).first().copied().unwrap_or(0);
                now_scaled = true;
                let p = present_scaled(s, present);
                put(&Cmd::Reset {
                    device: *device,
                    window: *window,
                    present: Data::Bytes(&p),
                    back_buffers: *back_buffers,
                    depth_stencil: *depth_stencil,
                });
            }
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
            } if s.screen_sized(*width, *height) => {
                scaled.insert(*surface);
                put(&Cmd::CreateSurface {
                    device: *device,
                    surface: *surface,
                    depth: *depth,
                    width: s.x(*width),
                    height: s.y(*height),
                    format: *format,
                    multisample: *multisample,
                    quality: *quality,
                    flag: *flag,
                });
            }
            Cmd::CreateTexture {
                device,
                texture,
                levels,
                width,
                height,
                usage,
                format,
                pool,
            } if usage & 0x3 != 0 && s.screen_sized(*width, *height) => {
                // D3DUSAGE_RENDERTARGET (1) and D3DUSAGE_DEPTHSTENCIL (2) textures.
                scaled.extend(handles(levels));
                scaled.insert(*texture);
                put(&Cmd::CreateTexture {
                    device: *device,
                    texture: *texture,
                    levels: *levels,
                    width: s.x(*width),
                    height: s.y(*height),
                    usage: *usage,
                    format: *format,
                    pool: *pool,
                });
            }
            Cmd::SetRenderTarget { index, surface, .. } => {
                if *index == 0 {
                    rt = *surface;
                    now_scaled = scaled.contains(&rt);
                }
                put(c);
            }
            Cmd::SetViewport { device, viewport } if now_scaled => {
                let v = *viewport;
                put(&Cmd::SetViewport {
                    device: *device,
                    viewport: [s.x(v[0]), s.y(v[1]), s.x(v[2]), s.y(v[3]), v[4], v[5]],
                });
            }
            Cmd::SetScissorRect { device, rect } if now_scaled => {
                put(&Cmd::SetScissorRect {
                    device: *device,
                    rect: [
                        s.x(rect[0] as u32) as i32,
                        s.y(rect[1] as u32) as i32,
                        s.x(rect[2] as u32) as i32,
                        s.y(rect[3] as u32) as i32,
                    ],
                });
            }
            Cmd::Clear {
                device,
                count,
                rects: Some(r),
                flags,
                colour,
                z,
                stencil,
                model,
            } if now_scaled => {
                let rs = rect_scaled(s, r);
                put(&Cmd::Clear {
                    device: *device,
                    count: *count,
                    rects: Some(Data::Bytes(&rs)),
                    flags: *flags,
                    colour: *colour,
                    z: *z,
                    stencil: *stencil,
                    model: *model,
                });
            }
            Cmd::StretchRect {
                device,
                source,
                source_rect,
                destination,
                destination_rect,
                filter,
            } => {
                let sr = source_rect
                    .filter(|_| scaled.contains(source))
                    .map(|r| rect_scaled(s, &r));
                let dr = destination_rect
                    .filter(|_| scaled.contains(destination))
                    .map(|r| rect_scaled(s, &r));
                put(&Cmd::StretchRect {
                    device: *device,
                    source: *source,
                    source_rect: match (&sr, source_rect) {
                        (Some(b), _) => Some(Data::Bytes(b)),
                        (None, r) => *r,
                    },
                    destination: *destination,
                    destination_rect: match (&dr, destination_rect) {
                        (Some(b), _) => Some(Data::Bytes(b)),
                        (None, r) => *r,
                    },
                    filter: *filter,
                });
            }
            Cmd::ColorFill {
                device,
                surface,
                rect: Some(r),
                colour,
            } if scaled.contains(surface) => {
                let b = rect_scaled(s, r);
                put(&Cmd::ColorFill {
                    device: *device,
                    surface: *surface,
                    rect: Some(Data::Bytes(&b)),
                    colour: *colour,
                });
            }
            Cmd::Destroy { object } => {
                scaled.remove(object);
                if rt == *object {
                    now_scaled = false;
                }
                put(c);
            }
            other => put(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::{stream, Scene};

    fn parse(bytes: &[u8]) -> Vec<Cmd<Data<'_>>> {
        wire::commands(bytes).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn the_targets_and_the_back_buffer_change_size_and_nothing_else_does() {
        let scene = Scene::small();
        let bytes = stream(&scene);
        let all = parse(&bytes);
        assert_eq!(back_buffer_size(&all), Some((64, 64)));
        let big = rescale(&all, &Scale::new((64, 64), (256, 128)));
        let out = parse(&big);
        assert_eq!(out.len(), all.len());
        assert_eq!(back_buffer_size(&out), Some((256, 128)));
        let sizes: Vec<(u32, u32)> = out
            .iter()
            .filter_map(|c| match c {
                Cmd::CreateSurface { width, height, .. } => Some((*width, *height)),
                _ => None,
            })
            .collect();
        assert_eq!(sizes, vec![(256, 128), (256, 128)]);
        // Draws and their order are untouched.
        let draws = |v: &[Cmd<Data<'_>>]| {
            v.iter()
                .filter(|c| matches!(c, Cmd::DrawIndexedPrimitive { .. }))
                .count()
        };
        assert_eq!(draws(&out), draws(&all));
        // The same scale is the same stream, byte for byte.
        let same = rescale(&all, &Scale::new((64, 64), (64, 64)));
        assert_eq!(same, bytes);
    }

    #[test]
    fn only_screen_sized_surfaces_scale() {
        let s = Scale::new((640, 480), (1280, 960));
        assert!(s.screen_sized(640, 480) && s.screen_sized(320, 240) && s.screen_sized(40, 30));
        assert!(!s.screen_sized(1024, 1024) && !s.screen_sized(640, 640));
        assert_eq!((s.x(641), s.y(1)), (1282, 2));
    }
}
