//! The counters the recorder keeps: cumulative atomics any thread adds to, turned into a frame's
//! numbers by taking the difference between two frame ends. A relaxed add is the whole hot-path
//! cost; nothing allocates.

use std::sync::atomic::{AtomicU64, Ordering};

/// What a counter measures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Ns,
    Count,
    Bytes,
}

macro_rules! counters {
    ($($variant:ident $name:literal $unit:ident,)+) => {
        /// One counter. `Main` ones are the game's main thread's own time; the rest are summed
        /// over the threads that report them.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(usize)]
        pub enum Counter { $($variant,)+ }

        /// Every counter, in index order.
        pub const ALL: &[Counter] = &[$(Counter::$variant,)+];
        /// Names, as they appear in the JSON record.
        pub const NAMES: &[&str] = &[$($name,)+];
        const UNITS: &[Unit] = &[$(Unit::$unit,)+];
    };
}

counters! {
    // The main thread's frame, split.
    PlatformNs "platform" Ns,
    PlatformCalls "platform_calls" Count,
    KernelWaitNs "kernel_wait" Ns,
    KernelWaits "kernel_waits" Count,
    DeviceWaitNs "device_wait" Ns,
    WaitNs "object_wait" Ns,
    RenderWaitNs "render_wait" Ns,
    SyncWaitNs "sync_wait" Ns,
    FileNs "file" Ns,
    SoundNs "sound" Ns,
    // Files, every thread.
    FileOtherNs "file_other_threads" Ns,
    FileReads "file_reads" Count,
    FileBytes "file_bytes" Bytes,
    FileOps "file_ops" Count,
    // The backend.
    Draws "draws" Count,
    PipelineN "pipelines" Count,
    PipelineNs "pipeline" Ns,
    ShaderN "shaders" Count,
    ShaderNs "shader" Ns,
    TextureN "textures" Count,
    TextureNs "texture" Ns,
    BufferN "buffers" Count,
    BufferNs "buffer" Ns,
    Presents "presents" Count,
    GpuWaitNs "gpu_wait" Ns,
    DeviceNs "device_thread" Ns,
    QueueNs "queue_thread" Ns,
    // Other game threads.
    OtherKernelWaitNs "kernel_wait_other_threads" Ns,
    SlowCalls "slow_calls" Count,
}

/// How many counters there are.
pub const N: usize = NAMES.len();

static VALUES: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];

impl Counter {
    pub fn name(self) -> &'static str {
        NAMES[self as usize]
    }

    pub fn unit(self) -> Unit {
        UNITS[self as usize]
    }
}

#[inline]
pub(crate) fn add(c: Counter, v: u64) {
    VALUES[c as usize].fetch_add(v, Ordering::Relaxed);
}

/// All values now.
pub(crate) fn read() -> [u64; N] {
    let mut out = [0; N];
    for (o, v) in out.iter_mut().zip(&VALUES) {
        *o = v.load(Ordering::Relaxed);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_units_and_indexes_line_up() {
        assert_eq!(ALL.len(), N);
        assert_eq!(UNITS.len(), N);
        for (i, c) in ALL.iter().enumerate() {
            assert_eq!(*c as usize, i);
        }
        assert_eq!(Counter::PlatformNs.name(), "platform");
        assert_eq!(Counter::FileBytes.unit(), Unit::Bytes);
        let mut names = NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), N, "a duplicate counter name");
    }
}
