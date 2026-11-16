//! The parsed stream, the targets it is played into and the timing loop.
//!
//! A stream is the commands before its first frame (the setup: the resources the frames use,
//! made once) and the frames, each ending at a `Present`. [`run`] plays the setup, then the frames
//! again and again: the first passes warm the backend up (pipelines, bind groups, caches, the
//! driver), the later ones are measured. A frame's time is split where the backends differ:
//! the commands up to the present (recording: decode is already done, so this is what the
//! backend costs the game's thread) and the present (the hand-over, and any wait for the frame in
//! flight before it).

use crate::stats::{process_cpu_ns, thread_cpu_ns};
use d3d9_stream::{Backend, Cmd, Data, Reply};
use std::ops::Range;
use std::time::Instant;

/// A stream parsed: every command (borrowing its payloads from the wire file's bytes), where the
/// setup ends and each frame's commands, its `Present` last.
pub struct Replay<'a> {
    pub cmds: Vec<Cmd<Data<'a>>>,
    /// The commands before the first frame.
    pub setup: usize,
    pub frames: Vec<Range<usize>>,
    /// Draw calls in each frame.
    pub draws: Vec<u32>,
}

impl<'a> Replay<'a> {
    /// The commands of `bytes` (a wire file), the first `setup` of them the setup. Commands after
    /// the last present are left out. Errors when the stream has no frame.
    pub fn parse(bytes: &'a [u8], setup: u64) -> Result<Replay<'a>, String> {
        let cmds = d3d9_stream::wire::commands(bytes)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let setup = (setup as usize).min(cmds.len());
        let mut frames = Vec::new();
        let mut draws = Vec::new();
        let (mut start, mut n) = (setup, 0u32);
        for (i, c) in cmds.iter().enumerate().skip(setup) {
            match c {
                Cmd::DrawPrimitive { .. }
                | Cmd::DrawIndexedPrimitive { .. }
                | Cmd::DrawPrimitiveUp { .. } => n += 1,
                Cmd::Present { .. } => {
                    frames.push(start..i + 1);
                    draws.push(n);
                    start = i + 1;
                    n = 0;
                }
                _ => {}
            }
        }
        if frames.is_empty() {
            return Err("the stream has no frame (no Present after its setup)".into());
        }
        let end = frames.last().map_or(0, |f| f.end);
        let mut cmds = cmds;
        cmds.truncate(end);
        Ok(Replay {
            cmds,
            setup,
            frames,
            draws,
        })
    }

    /// Draw calls in one pass over the frames.
    pub fn draws_per_pass(&self) -> u64 {
        self.draws.iter().map(|&d| u64::from(d)).sum()
    }
}

/// What a stream is played into.
pub trait Target: Backend {
    fn name(&self) -> &'static str;
    /// Before the first command of a frame.
    fn frame_begin(&mut self) {}
    /// After the frame's `Present` returned.
    fn frame_end(&mut self) {}
    /// Waits until the work given so far is done (between the setup and the frames, and at the
    /// end).
    fn drain(&mut self) {}
    /// After the last frame: the GPU time of each frame begun, in order, in nanoseconds, when the
    /// target measures it.
    fn gpu_ns(&mut self) -> Option<Vec<f64>> {
        None
    }
}

/// One frame's measurement.
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    /// The commands before the present, wall clock.
    pub cmd_ns: u64,
    /// The present, wall clock.
    pub present_ns: u64,
    /// The calling thread's CPU time over the commands and the present.
    pub thread_cpu_ns: u64,
    /// The whole process's CPU time over the same (the backend's other threads included).
    pub process_cpu_ns: u64,
    pub draws: u32,
}

/// How a run is made.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Passes over the frames before the measured ones.
    pub warmup: usize,
    /// Passes measured.
    pub passes: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            warmup: 3,
            passes: 10,
        }
    }
}

/// What a run measured.
#[derive(Debug, Default)]
pub struct Run {
    pub target: &'static str,
    /// The setup's wall time, milliseconds.
    pub setup_ms: f64,
    /// The warm-up's wall time, milliseconds.
    pub warmup_ms: f64,
    /// The measured frames, pass after pass.
    pub frames: Vec<Frame>,
    /// The GPU time of each measured frame, nanoseconds, when the target measures it.
    pub gpu_ns: Option<Vec<f64>>,
    /// Wall time of all the measured passes, milliseconds (draws a second is over this).
    pub wall_ms: f64,
    /// CPU nanoseconds each thread used over the measured passes, by thread name, most first.
    pub threads: Vec<(String, u64)>,
}

/// Plays `replay` into `target`: the setup, then `warmup + passes` passes over the frames.
pub fn run(target: &mut dyn Target, replay: &Replay<'_>, o: Options) -> Result<Run, String> {
    let mut out = Run {
        target: target.name(),
        ..Run::default()
    };
    let t = Instant::now();
    for c in &replay.cmds[..replay.setup] {
        if let Reply::Device { created: false, .. } = target.execute(*c) {
            return Err(format!("{}: the device was not made", target.name()));
        }
    }
    target.drain();
    out.setup_ms = t.elapsed().as_secs_f64() * 1e3;
    let mut begun = 0usize;
    let mut warm_frames = 0usize;
    let t = Instant::now();
    let mut t_measured = Instant::now();
    let mut threads0 = Vec::new();
    for pass in 0..o.warmup + o.passes {
        let measured = pass >= o.warmup;
        if pass == o.warmup {
            target.drain();
            out.warmup_ms = t.elapsed().as_secs_f64() * 1e3;
            warm_frames = begun;
            t_measured = Instant::now();
            threads0 = crate::stats::threads_cpu_ns();
        }
        for (f, range) in replay.frames.iter().enumerate() {
            let last = range.end - 1;
            let (p0, c0) = (process_cpu_ns(), thread_cpu_ns());
            target.frame_begin();
            begun += 1;
            let w0 = Instant::now();
            for c in &replay.cmds[range.start..last] {
                target.execute(*c);
            }
            let w1 = Instant::now();
            target.execute(replay.cmds[last]);
            let w2 = Instant::now();
            target.frame_end();
            if measured {
                let (p1, c1) = (process_cpu_ns(), thread_cpu_ns());
                out.frames.push(Frame {
                    cmd_ns: (w1 - w0).as_nanos() as u64,
                    present_ns: (w2 - w1).as_nanos() as u64,
                    thread_cpu_ns: c1.zip(c0).map_or(0, |(b, a)| b - a),
                    process_cpu_ns: p1.zip(p0).map_or(0, |(b, a)| b - a),
                    draws: replay.draws[f],
                });
            }
        }
    }
    target.drain();
    out.wall_ms = t_measured.elapsed().as_secs_f64() * 1e3;
    for (name, ns) in crate::stats::threads_cpu_ns() {
        let before = threads0.iter().find(|(n, _)| *n == name).map_or(0, |x| x.1);
        if ns > before {
            out.threads.push((name, ns - before));
        }
    }
    out.threads.sort_by_key(|t| std::cmp::Reverse(t.1));
    if let Some(all) = target.gpu_ns() {
        out.gpu_ns = Some(all.into_iter().skip(warm_frames).collect());
    }
    Ok(out)
}

/// Where the commands' time goes, by class, over one pass of `replay` (each command timed on its
/// own, so the clock's cost is taken off: see [`crate::stats::timer_overhead_ns`]).
#[derive(Debug, Default, Clone, Copy)]
pub struct Split {
    pub draw_ns: u64,
    pub draws: u64,
    pub state_ns: u64,
    pub state: u64,
    pub bind_ns: u64,
    pub binds: u64,
    pub constants_ns: u64,
    pub constants: u64,
    pub upload_ns: u64,
    pub uploads: u64,
    pub other_ns: u64,
    pub other: u64,
    pub present_ns: u64,
}

/// One pass over the frames with every command timed alone.
pub fn split(target: &mut dyn Target, replay: &Replay<'_>, overhead_ns: f64) -> Split {
    let mut s = Split::default();
    let over = overhead_ns as u64;
    for range in &replay.frames {
        target.frame_begin();
        for c in &replay.cmds[range.clone()] {
            let t = Instant::now();
            target.execute(*c);
            let ns = (t.elapsed().as_nanos() as u64).saturating_sub(over);
            let (acc, n) = match c {
                Cmd::DrawPrimitive { .. }
                | Cmd::DrawIndexedPrimitive { .. }
                | Cmd::DrawPrimitiveUp { .. } => (&mut s.draw_ns, &mut s.draws),
                Cmd::SetRenderState { .. }
                | Cmd::SetSamplerState { .. }
                | Cmd::SetTextureStageState { .. }
                | Cmd::SetViewport { .. }
                | Cmd::SetScissorRect { .. }
                | Cmd::SetTransform { .. }
                | Cmd::SetClipPlane { .. }
                | Cmd::SetMaterial { .. }
                | Cmd::SetLight { .. }
                | Cmd::LightEnable { .. }
                | Cmd::SetFvf { .. } => (&mut s.state_ns, &mut s.state),
                Cmd::SetTexture { .. }
                | Cmd::SetStreamSource { .. }
                | Cmd::SetIndices { .. }
                | Cmd::SetShader { .. }
                | Cmd::SetVertexDeclaration { .. }
                | Cmd::SetRenderTarget { .. }
                | Cmd::SetDepthStencilSurface { .. } => (&mut s.bind_ns, &mut s.binds),
                Cmd::SetConstants { .. } => (&mut s.constants_ns, &mut s.constants),
                Cmd::UploadBuffer { .. } | Cmd::UploadSurface { .. } => {
                    (&mut s.upload_ns, &mut s.uploads)
                }
                Cmd::Present { .. } => {
                    s.present_ns += ns;
                    continue;
                }
                _ => (&mut s.other_ns, &mut s.other),
            };
            *acc += ns;
            *n += 1;
        }
        target.frame_end();
    }
    target.drain();
    s
}

/// The wgpu backend as a target. `Present` of an offscreen device is the backend's hand-over of
/// the frame to its queue thread.
pub struct WgpuTarget {
    inner: d3d9_wgpu::WgpuBackend,
    /// Frames to time on the GPU (`NV_REPLAY_GPU_TIMING` set before the device is made), once the
    /// device exists.
    gpu_frames: Option<usize>,
    timing: bool,
}

impl WgpuTarget {
    /// A backend with an offscreen device; `gpu_frames` the frames whose GPU time is to be
    /// measured, if any.
    pub fn new(gpu_frames: Option<usize>) -> WgpuTarget {
        let mut inner = d3d9_wgpu::WgpuBackend::new(d3d9_wgpu::Output::Offscreen);
        if let Some(dir) = std::env::var_os("NV_WGPU_CACHE") {
            inner = inner.with_cache_dir(dir.into());
        }
        WgpuTarget {
            inner,
            gpu_frames,
            timing: false,
        }
    }
}

impl Backend for WgpuTarget {
    fn execute(&mut self, cmd: Cmd<Data<'_>>) -> Reply {
        self.inner.execute(cmd)
    }
}

impl Target for WgpuTarget {
    fn name(&self) -> &'static str {
        "wgpu"
    }
    fn frame_begin(&mut self) {
        if let (Some(n), false) = (self.gpu_frames, self.timing) {
            if let Some(d) = self.inner.device() {
                self.timing = d.gpu_timing_start(n);
                self.gpu_frames = None;
            }
        }
        if self.timing {
            if let Some(d) = self.inner.device() {
                d.gpu_frame_begin();
            }
        }
    }
    fn frame_end(&mut self) {
        if self.timing {
            if let Some(d) = self.inner.device() {
                d.gpu_frame_end();
            }
        }
    }
    fn drain(&mut self) {
        if let Some(d) = self.inner.device() {
            d.wait_idle();
        }
    }
    fn gpu_ns(&mut self) -> Option<Vec<f64>> {
        self.inner.device()?.gpu_frame_times_ns()
    }
}

impl Target for crate::null::Null {
    fn name(&self) -> &'static str {
        "null"
    }
}
