//! The replay-bench (docs/fork/REPLAY.md): a recorded Direct3D 9 command stream
//! (`d3d9_stream::wire`) replayed into a backend, timed.
//!
//! - [`container`]: the on-disk form of a stream kept for benchmarking: versioned, compressed,
//!   with numbers about it (never the game's data in a repository: the files stay under
//!   `~/.local/share/nv-rs-research/streams/`).
//! - [`window`]: a recording cut to a window of frames that replays on its own, the resources
//!   the window needs made first.
//! - [`rescale`]: the same stream for another render target size, without recording again.
//! - [`replay`]: the parsed stream, the backends it is played into and the timing loop.
//! - [`null`]: the floor, a backend that only counts.
//! - [`synthetic`]: a made-up scene, for tests and for the harness's own check.
//! - [`stats`]: percentiles and the CPU time of the thread and of the process.

pub mod container;
pub mod null;
pub mod replay;
pub mod rescale;
pub mod stats;
pub mod synthetic;
pub mod window;
