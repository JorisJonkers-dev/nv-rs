//! The floor: a backend that carries nothing out, only counts what it is given.
//!
//! What a replay into it costs is the harness's own (walking the commands, the call through the
//! trait object) and, when the stream is decoded as it is played, the decoding: the least any
//! backend pays for a draw. [`Null::digest`] folds each draw's parameters and the state the game
//! set, so two replays of one stream are known to have been given the same commands and the
//! compiler cannot drop the work.

use d3d9_stream::{Backend, Cmd, Data, Reply};

/// Counts and a digest of what a [`Null`] was given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub commands: u64,
    pub draws: u64,
    pub state_sets: u64,
    pub binds: u64,
    pub uploads: u64,
    pub upload_bytes: u64,
    pub creates: u64,
    pub presents: u64,
    pub clears: u64,
    /// A hash of the draws and of the states and constants they were drawn with.
    pub digest: u64,
}

/// The backend that does nothing but note each command.
#[derive(Default)]
pub struct Null {
    pub counts: Counts,
    /// The last value of each of the first 256 render states.
    states: Vec<u32>,
}

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x0100_0000_01b3).rotate_left(5)
}

impl Null {
    pub fn new() -> Null {
        Null {
            counts: Counts {
                digest: 0xcbf2_9ce4_8422_2325,
                ..Counts::default()
            },
            states: vec![0; 256],
        }
    }

    /// What the draws and the state they saw hashed to.
    pub fn digest(&self) -> u64 {
        self.counts.digest
    }
}

impl Backend for Null {
    fn execute(&mut self, cmd: Cmd<Data<'_>>) -> Reply {
        let c = &mut self.counts;
        c.commands += 1;
        match cmd {
            Cmd::CreateDevice { .. } => {
                return Reply::Device {
                    created: true,
                    native: None,
                }
            }
            Cmd::DrawPrimitive {
                kind, start, count, ..
            } => {
                c.draws += 1;
                c.digest = mix(
                    c.digest,
                    u64::from(kind) << 40 | u64::from(start) << 20 | u64::from(count),
                );
            }
            Cmd::DrawIndexedPrimitive {
                kind,
                base_vertex,
                min_index,
                vertices,
                start_index,
                count,
                ..
            } => {
                c.draws += 1;
                let a = u64::from(kind) << 48
                    ^ u64::from(base_vertex as u32) << 16
                    ^ u64::from(min_index);
                let b = u64::from(vertices) << 40 ^ u64::from(start_index) << 20 ^ u64::from(count);
                c.digest = mix(mix(c.digest, a), b);
            }
            Cmd::DrawPrimitiveUp {
                kind,
                count,
                stride,
                vertices,
                ..
            } => {
                c.draws += 1;
                c.digest = mix(
                    c.digest,
                    u64::from(kind) << 40
                        ^ u64::from(count) << 20
                        ^ u64::from(stride)
                        ^ u64::from(vertices.len()),
                );
            }
            Cmd::SetRenderState { state, value, .. } => {
                c.state_sets += 1;
                if let Some(s) = self.states.get_mut(state as usize) {
                    *s = value;
                }
                c.digest = mix(c.digest, u64::from(state) << 32 | u64::from(value));
            }
            Cmd::SetSamplerState { .. }
            | Cmd::SetTextureStageState { .. }
            | Cmd::SetViewport { .. }
            | Cmd::SetScissorRect { .. }
            | Cmd::SetTransform { .. }
            | Cmd::SetClipPlane { .. }
            | Cmd::SetMaterial { .. }
            | Cmd::SetLight { .. }
            | Cmd::LightEnable { .. }
            | Cmd::SetFvf { .. } => c.state_sets += 1,
            Cmd::SetConstants {
                kind,
                start,
                count,
                data,
                ..
            } => {
                c.state_sets += 1;
                c.digest = mix(
                    c.digest,
                    u64::from(kind) << 48
                        ^ u64::from(start) << 24
                        ^ u64::from(count)
                        ^ u64::from(data.len()),
                );
            }
            Cmd::SetTexture {
                sampler, texture, ..
            } => {
                c.binds += 1;
                c.digest = mix(c.digest, u64::from(sampler) << 32 | u64::from(texture));
            }
            Cmd::SetStreamSource {
                stream,
                buffer,
                offset,
                stride,
                ..
            } => {
                c.binds += 1;
                c.digest = mix(
                    c.digest,
                    u64::from(buffer) << 32
                        ^ u64::from(offset) << 8
                        ^ u64::from(stream)
                        ^ u64::from(stride) << 48,
                );
            }
            Cmd::SetIndices { buffer, .. } => {
                c.binds += 1;
                c.digest = mix(c.digest, u64::from(buffer));
            }
            Cmd::SetShader { shader, pixel, .. } => {
                c.binds += 1;
                c.digest = mix(c.digest, u64::from(shader) << 1 | u64::from(pixel));
            }
            Cmd::SetVertexDeclaration { declaration, .. } => {
                c.binds += 1;
                c.digest = mix(c.digest, u64::from(declaration));
            }
            Cmd::SetRenderTarget { surface, index, .. } => {
                c.binds += 1;
                c.digest = mix(c.digest, u64::from(surface) << 8 | u64::from(index));
            }
            Cmd::SetDepthStencilSurface { surface, .. } => {
                c.binds += 1;
                c.digest = mix(c.digest, u64::from(surface));
            }
            Cmd::UploadBuffer { data, .. } => {
                c.uploads += 1;
                c.upload_bytes += u64::from(data.len());
            }
            Cmd::UploadSurface { data, .. } => {
                c.uploads += 1;
                c.upload_bytes += u64::from(data.len());
            }
            Cmd::CreateTexture { .. }
            | Cmd::CreateCubeTexture { .. }
            | Cmd::CreateSurface { .. }
            | Cmd::CreateOffscreenSurface { .. }
            | Cmd::CreateBuffer { .. }
            | Cmd::CreateShader { .. }
            | Cmd::CreateDeclaration { .. }
            | Cmd::CreateQuery { .. } => c.creates += 1,
            Cmd::Clear { .. } => c.clears += 1,
            Cmd::Present { .. } => c.presents += 1,
            Cmd::ReadSurface { .. } => return Reply::Bytes(None),
            Cmd::GetRenderTargetData { .. } => return Reply::Rows(None),
            Cmd::QueryGetData { .. } => return Reply::Query(None),
            _ => {}
        }
        Reply::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::Replay;
    use crate::synthetic::{stream, Scene};

    #[test]
    fn replays_are_deterministic_and_count_the_draws() {
        let scene = Scene::small();
        let bytes = stream(&scene);
        let run = || {
            let replay = Replay::parse(&bytes, 0).unwrap();
            let mut n = Null::new();
            for c in &replay.cmds {
                n.execute(*c);
            }
            n.counts
        };
        let (a, b) = (run(), run());
        assert_eq!(a, b);
        assert_eq!(a.draws, u64::from(scene.frames * scene.draws));
        assert_eq!(a.presents, u64::from(scene.frames));
        assert_ne!(a.digest, 0xcbf2_9ce4_8422_2325);
        // Another scene hashes to another digest.
        let other = stream(&Scene {
            draws: scene.draws + 1,
            ..scene
        });
        let replay = Replay::parse(&other, 0).unwrap();
        let mut n = Null::new();
        for c in &replay.cmds {
            n.execute(*c);
        }
        assert_ne!(n.digest(), a.digest);
    }
}
