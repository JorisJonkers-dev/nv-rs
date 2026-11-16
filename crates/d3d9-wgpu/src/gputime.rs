//! Fork-owned: the GPU time of a frame, for the replay-bench (`crates/d3d9-replay`).
//!
//! With `NV_REPLAY_GPU_TIMING=1` when the device is made, the device asks for timestamp queries.
//! [`Device::gpu_frame_begin`] puts a timestamp on the queue (after the batches before it) and
//! [`Device::gpu_frame_end`] another one after the frame's last batch with the pair resolved into
//! a buffer; [`Device::gpu_frame_times_ns`] reads them all at the end. A frame's time is then the
//! span from the first timestamp to the second on the GPU, the idle time between the frame's
//! batches included: when the CPU is the slower side the span is the CPU's pace, not the card's
//! work. The game's own run never asks for this.

use crate::device::Device;

/// The features the timing needs, when `NV_REPLAY_GPU_TIMING` asks for them.
pub(crate) fn features() -> wgpu::Features {
    if std::env::var_os("NV_REPLAY_GPU_TIMING").is_some_and(|v| !v.is_empty()) {
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
    } else {
        wgpu::Features::empty()
    }
}

/// The queries and the buffers of a run.
pub(crate) struct GpuTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    /// Queries a set can hold (two a frame, reused round).
    slots: u32,
    capacity: u32,
    begun: u32,
    ended: u32,
}

/// A frame's resolved pair in the buffer: wgpu wants resolve offsets aligned to 256 bytes.
const SLOT: u64 = wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;

/// wgpu's most queries in a set.
const SET_QUERIES: u32 = 4096;

impl Device {
    /// Wait for the queue thread and the card to be idle.
    pub fn wait_idle(&mut self) {
        self.sync();
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Makes the timer for `frames` frames; false without the timestamp features.
    pub fn gpu_timing_start(&mut self, frames: usize) -> bool {
        let wanted =
            wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !self.device.features().contains(wanted) {
            return false;
        }
        let set = self.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("replay gpu time"),
            ty: wgpu::QueryType::Timestamp,
            count: SET_QUERIES,
        });
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("replay gpu time resolved"),
            size: (frames as u64 * SLOT).max(SLOT),
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        self.gpu_timer = Some(GpuTimer {
            set,
            resolve,
            slots: SET_QUERIES / 2,
            capacity: frames as u32,
            begun: 0,
            ended: 0,
        });
        true
    }

    /// A timestamp on the queue before the frame's batches.
    pub fn gpu_frame_begin(&mut self) {
        if self.gpu_timer.is_none() {
            return;
        }
        self.flush();
        let Some(t) = self.gpu_timer.as_mut() else {
            return;
        };
        if t.begun >= t.capacity {
            return;
        }
        let i = (t.begun % t.slots) * 2;
        t.begun += 1;
        let (device, set) = (self.device.clone(), t.set.clone());
        self.on_queue(move |queue| {
            let mut e = device.create_command_encoder(&Default::default());
            e.write_timestamp(&set, i);
            queue.submit([e.finish()]);
        });
    }

    /// A timestamp after the frame's last batch, the pair resolved.
    pub fn gpu_frame_end(&mut self) {
        if self.gpu_timer.is_none() {
            return;
        }
        self.flush();
        let Some(t) = self.gpu_timer.as_mut() else {
            return;
        };
        if t.ended >= t.begun {
            return;
        }
        let n = t.ended;
        let i = (n % t.slots) * 2;
        t.ended += 1;
        let (device, set, resolve) = (self.device.clone(), t.set.clone(), t.resolve.clone());
        self.on_queue(move |queue| {
            let mut e = device.create_command_encoder(&Default::default());
            e.write_timestamp(&set, i + 1);
            e.resolve_query_set(&set, i..i + 2, &resolve, u64::from(n) * SLOT);
            queue.submit([e.finish()]);
        });
    }

    /// Every frame's GPU time in nanoseconds, in the order they began; none without a timer.
    pub fn gpu_frame_times_ns(&mut self) -> Option<Vec<f64>> {
        self.wait_idle();
        let (resolve, frames) = {
            let t = self.gpu_timer.as_ref()?;
            (t.resolve.clone(), t.ended)
        };
        let size = u64::from(frames) * SLOT;
        if size == 0 {
            return Some(Vec::new());
        }
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("replay gpu time read"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut e = self.device.create_command_encoder(&Default::default());
        e.copy_buffer_to_buffer(&resolve, 0, &read, 0, size);
        self.queue.submit([e.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let period = f64::from(self.queue.get_timestamp_period());
        let data = read.slice(..).get_mapped_range().ok()?;
        let tick = |at: usize| u64::from_le_bytes(data[at..at + 8].try_into().expect("8 bytes"));
        Some(
            (0..frames as usize)
                .map(|n| {
                    let at = n * SLOT as usize;
                    tick(at + 8).saturating_sub(tick(at)) as f64 * period
                })
                .collect(),
        )
    }
}
