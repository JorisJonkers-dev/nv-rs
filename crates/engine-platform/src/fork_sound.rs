//! The sound output off Windows (fork-owned, docs/fork/LINUX.md "Sound"): the same seam as the
//! WASAPI stream in `sound_out.rs` ([`super::sound_out::Source`], [`super::sound_out::Output`]),
//! with two backends behind it.
//!
//! * **Pulse**: libpulse (the threaded main loop, context and stream API), loaded with `dlopen`
//!   at the first use (nothing is linked or needed to build; `libpulse.so.0`, or the file
//!   `NV_PULSE` names, is a run-time prerequisite: Arch `libpulse`, Debian and Ubuntu `libpulse0`,
//!   Fedora `pulseaudio-libs`). Under PipeWire the same library talks to `pipewire-pulse`, so one
//!   backend serves PipeWire and PulseAudio sessions. The default sink's channel map is probed
//!   and reported as the mix layout ([`mix`]); a stream is 32-bit float at the reference rate
//!   with an explicit channel map in WAVE order, and the server's underflow callback reaches
//!   `Source::starved`.
//! * **Null**: a thread that asks the source for frames on the wall clock and drops them. It is
//!   the fallback with no sound server (a headless machine, CI) and what `NV_SOUND_BACKEND=null`
//!   asks for; the mixer runs exactly as with a device, so the game's sound path is exercised.
//!
//! `NV_SOUND_BACKEND` is `auto` (the default: Pulse, else Null with a line on stderr), `pulse`
//! (Pulse, else no device) or `null`. The mix rate is fixed ([`MIX_RATE`] Hz; the server resamples).
//! The layout is the default sink's, or `NV_SOUND_LAYOUT` (`auto`, `stereo`, `5.1`, `7.1`).

use super::sound_out::{Layout, Output, Refused, Source};
use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The rate the mixer runs at on this target (the shared-mode rate DirectSound saw on the
/// measured machine); the sound server converts to the sink's.
pub const MIX_RATE: u32 = super::fork_tables::MIX_RATE;

/// Frames asked of the source at a time: 20 ms.
const CHUNK_FRAMES: usize = (MIX_RATE / 50) as usize;

/// Which backend to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Pulse, else Null.
    Auto,
    /// Pulse only.
    Pulse,
    /// No device.
    Null,
}

impl Backend {
    /// Reads `NV_SOUND_BACKEND` (`auto`, `pulse`, `null`); anything else is `auto`.
    pub fn from_env() -> Backend {
        Backend::parse(std::env::var("NV_SOUND_BACKEND").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> Backend {
        match value.map(str::trim) {
            Some("pulse") => Backend::Pulse,
            Some("null") => Backend::Null,
            _ => Backend::Auto,
        }
    }
}

/// `NV_SOUND_LAYOUT`: which layout the mix reports to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutChoice {
    /// The default sink's channel map.
    Auto,
    Stereo,
    FiveOne,
    SevenOne,
}

impl LayoutChoice {
    /// Reads `NV_SOUND_LAYOUT` (`auto`, `stereo`, `5.1`, `7.1`); anything else is `auto`.
    pub fn from_env() -> LayoutChoice {
        LayoutChoice::parse(std::env::var("NV_SOUND_LAYOUT").ok().as_deref())
    }

    pub fn parse(value: Option<&str>) -> LayoutChoice {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("stereo") => LayoutChoice::Stereo,
            Some("5.1") => LayoutChoice::FiveOne,
            Some("7.1") => LayoutChoice::SevenOne,
            _ => LayoutChoice::Auto,
        }
    }
}

/// `pa_channel_position_t` values (`PA_CHANNEL_POSITION_*`).
pub mod position {
    pub const MONO: i32 = 0;
    pub const FRONT_LEFT: i32 = 1;
    pub const FRONT_RIGHT: i32 = 2;
    pub const FRONT_CENTER: i32 = 3;
    pub const REAR_LEFT: i32 = 5;
    pub const REAR_RIGHT: i32 = 6;
    pub const LFE: i32 = 7;
    pub const SIDE_LEFT: i32 = 10;
    pub const SIDE_RIGHT: i32 = 11;
}

/// The WAVE speaker bit of a Pulse channel position (`None`: a position DirectSound's layouts do
/// not have), and back again for the positions the stream is opened with.
const POSITION_BITS: [(i32, u32); 8] = [
    (position::FRONT_LEFT, 0x1),
    (position::FRONT_RIGHT, 0x2),
    (position::FRONT_CENTER, 0x4),
    (position::LFE, 0x8),
    (position::REAR_LEFT, 0x10),
    (position::REAR_RIGHT, 0x20),
    (position::SIDE_LEFT, 0x200),
    (position::SIDE_RIGHT, 0x400),
];

/// The Pulse channel map of a layout in WAVE order (one position per mask bit; the mono layout
/// is `PA_CHANNEL_POSITION_MONO`).
pub fn channel_map_of(layout: Layout) -> Vec<i32> {
    if layout == Layout::MONO {
        return vec![position::MONO];
    }
    POSITION_BITS
        .iter()
        .filter(|(_, bit)| layout.mask & bit != 0)
        .map(|(p, _)| *p)
        .collect()
}

/// The mixer's layout for a sink's channel map, and whether the map is exactly one of the
/// layouts it provides. Otherwise the nearest provided layout whose speakers the sink has (the
/// server remixes the rest), else stereo.
pub fn layout_of_map(map: &[i32]) -> (Layout, bool) {
    if map == [position::MONO] {
        return (Layout::MONO, true);
    }
    let mut mask = 0u32;
    for p in map {
        if let Some((_, bit)) = POSITION_BITS.iter().find(|(q, _)| q == p) {
            mask |= bit;
        }
    }
    let exact = Layout {
        channels: map.len() as u16,
        mask,
    };
    if exact.provided() {
        return (exact, true);
    }
    let near = [0x63f_u32, 0xff, 0x60f, 0x3f, 0x33]
        .into_iter()
        .find(|c| mask & c == *c)
        .map(|c| Layout {
            channels: c.count_ones() as u16,
            mask: c,
        })
        .unwrap_or(Layout::STEREO);
    (near, false)
}

/// The layout to report: the override, else the probed sink's map (stereo when there is none).
fn choose_layout(choice: LayoutChoice, probe: impl FnOnce() -> Option<Vec<i32>>) -> Layout {
    let five_one = |map: Option<Vec<i32>>| match map.map(|m| layout_of_map(&m)) {
        // A side 5.1 sink keeps its side speakers; any other 5.1 is the back layout.
        Some((l, true)) if l.mask == 0x60f => l,
        _ => Layout::of_mix(6, Some(0x3f)),
    };
    match choice {
        LayoutChoice::Stereo => Layout::STEREO,
        LayoutChoice::SevenOne => Layout::of_mix(8, Some(0x63f)),
        LayoutChoice::FiveOne => five_one(probe()),
        LayoutChoice::Auto => match probe() {
            Some(map) => {
                let (layout, exact) = layout_of_map(&map);
                if !exact {
                    fork_log::warn!(target: "sound",
                        "sound: the sink's channel map {map:?} is not a layout the mixer provides; \
                         using {} channels (mask {:#x}) and the server remixes",
                        layout.channels, layout.mask
                    );
                }
                layout
            }
            None => Layout::STEREO,
        },
    }
}

/// The mix format of this target: the rate and the layout the stream is opened in when the game
/// asks for none. `None` when [`Backend::Pulse`] is asked for and the library is not there. The
/// layout is the default sink's (probed once), or what `NV_SOUND_LAYOUT` names.
pub fn mix(backend: Backend) -> Option<(u32, Layout)> {
    if backend == Backend::Pulse && pulse::api().is_none() {
        return None;
    }
    static LAYOUT: std::sync::OnceLock<Layout> = std::sync::OnceLock::new();
    let layout = *LAYOUT.get_or_init(|| {
        choose_layout(LayoutChoice::from_env(), || match backend {
            Backend::Null => None,
            _ => pulse::probe_channel_map(),
        })
    });
    Some((MIX_RATE, layout))
}

/// Starts playing `source` ([`super::sound_out::start`]).
pub fn start(
    backend: Backend,
    source: Arc<Mutex<dyn Source>>,
    want: Option<Layout>,
) -> Result<Output, Refused> {
    let layout = want.unwrap_or(Layout::STEREO);
    if !layout.provided() {
        return Err(Refused::Format(super::sound_out::not_provided(layout)));
    }
    match backend {
        Backend::Null => Ok(start_null(source, layout)),
        Backend::Pulse => start_pulse(source, layout),
        Backend::Auto => start_pulse(source.clone(), layout).or_else(|why| {
            fork_log::warn!(target: "sound",
                "sound: no sound server answered ({why:?}); the mix is made and dropped \
                 (NV_SOUND_BACKEND=null does this on purpose)"
            );
            Ok(start_null(source, layout))
        }),
    }
}

fn start_null(source: Arc<Mutex<dyn Source>>, layout: Layout) -> Output {
    spawn(source, layout, "nv sound null", |_| Ok(Box::new(Drop_)))
        .expect("the null sink always opens")
}

fn start_pulse(source: Arc<Mutex<dyn Source>>, layout: Layout) -> Result<Output, Refused> {
    let api = pulse::api().ok_or(Refused::NoDevice)?;
    spawn(source, layout, "nv sound pulse", move |l| {
        pulse::Stream::open(api, l).map(|s| Box::new(s) as Box<dyn Sink>)
    })
}

/// Where a chunk of frames goes; a blocking write paces the thread when it is a device.
trait Sink {
    /// Writes interleaved float frames; `false` when the sink no longer takes them.
    fn write(&mut self, frames: &[f32]) -> bool;
    /// Whether `write` blocks until the device has room (else the thread keeps the wall clock).
    fn paces(&self) -> bool;
    /// How many underruns the device has reported on this stream so far.
    fn underruns(&self) -> u64 {
        0
    }
}

/// The null sink.
struct Drop_;

impl Sink for Drop_ {
    fn write(&mut self, _frames: &[f32]) -> bool {
        true
    }
    fn paces(&self) -> bool {
        false
    }
}

type Open = dyn FnOnce(Layout) -> Result<Box<dyn Sink>, Refused> + Send;

/// Runs the output thread: opens the sink on it (so the caller learns of a refusal), then fills
/// and writes chunks until the [`Output`] is dropped.
fn spawn(
    source: Arc<Mutex<dyn Source>>,
    layout: Layout,
    name: &str,
    open: impl FnOnce(Layout) -> Result<Box<dyn Sink>, Refused> + Send + 'static,
) -> Result<Output, Refused> {
    let open: Box<Open> = Box::new(open);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name(name.into())
        .spawn(move || match open(layout) {
            Ok(sink) => {
                let _ = tx.send(Ok(()));
                run(sink, &source, layout, &flag);
            }
            Err(why) => {
                let _ = tx.send(Err(why));
            }
        })
        .map_err(|_| Refused::NoDevice)?;
    match rx.recv() {
        Ok(Ok(())) => Ok(Output::running(stop, thread, MIX_RATE, layout)),
        Ok(Err(why)) => {
            let _ = thread.join();
            Err(why)
        }
        Err(_) => Err(Refused::NoDevice),
    }
}

fn run(
    mut sink: Box<dyn Sink>,
    source: &Arc<Mutex<dyn Source>>,
    layout: Layout,
    stop: &AtomicBool,
) {
    let channels = layout.channels;
    let mut chunk = vec![0f32; CHUNK_FRAMES * usize::from(channels)];
    let period = Duration::from_micros(1_000_000 * CHUNK_FRAMES as u64 / u64::from(MIX_RATE));
    let mut due = Instant::now();
    let mut lost = false;
    let mut reported = 0u64;
    while !stop.load(Ordering::Relaxed) {
        chunk.fill(0.0);
        if let Ok(mut s) = source.lock() {
            s.render(MIX_RATE, channels, &mut chunk);
        }
        if super::sound_out::is_muted() {
            chunk.fill(0.0);
        }
        let underruns = sink.underruns();
        while reported < underruns {
            reported += 1;
            if reported == 1 {
                fork_log::warn!(target: "sound",
                    "sound: the sound server reported an underrun on this stream (later ones are \
                     counted, not logged here)"
                );
            }
            if let Ok(mut s) = source.lock() {
                s.starved(reported);
            }
        }
        if !sink.write(&chunk) {
            if !lost {
                lost = true;
                fork_log::warn!(target: "sound",
                    "sound: the sound server stopped taking frames; this stream is not heard"
                );
            }
            std::thread::sleep(period);
            continue;
        }
        if !sink.paces() {
            due += period;
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            } else if now - due > period * 25 {
                // Far behind (the machine slept): do not burst to catch up.
                due = now;
            }
        }
    }
}

/// libpulse through `dlopen`: a threaded main loop, the context, and the playback stream.
mod pulse {
    use super::*;
    use libloading::Library;
    use std::sync::atomic::AtomicU64;
    use std::sync::OnceLock;

    /// `pa_sample_spec`.
    #[repr(C)]
    struct SampleSpec {
        format: c_int,
        rate: u32,
        channels: u8,
    }

    /// `pa_channel_map`.
    #[repr(C)]
    struct ChannelMap {
        channels: u8,
        map: [c_int; 32],
    }

    /// `pa_buffer_attr`.
    #[repr(C)]
    struct BufferAttr {
        maxlength: u32,
        tlength: u32,
        prebuf: u32,
        minreq: u32,
        fragsize: u32,
    }

    /// The leading fields of `pa_server_info` and `pa_sink_info` (all that are read).
    #[repr(C)]
    struct ServerInfo {
        user_name: *const c_char,
        host_name: *const c_char,
        server_version: *const c_char,
        server_name: *const c_char,
        sample_spec: SampleSpec,
        default_sink_name: *const c_char,
    }

    #[repr(C)]
    struct SinkInfo {
        name: *const c_char,
        index: u32,
        description: *const c_char,
        sample_spec: SampleSpec,
        channel_map: ChannelMap,
    }

    const SAMPLE_FLOAT32LE: c_int = 5;
    const STREAM_ADJUST_LATENCY: c_int = 0x2000;
    const CONTEXT_READY: c_int = 4;
    const CONTEXT_FAILED: c_int = 5;
    const CONTEXT_TERMINATED: c_int = 6;
    const STREAM_READY: c_int = 2;
    const OPERATION_RUNNING: c_int = 0;

    type Ptr = *mut c_void;
    type Notify = unsafe extern "C" fn(Ptr, Ptr);
    type Request = unsafe extern "C" fn(Ptr, usize, Ptr);
    type ServerCb = unsafe extern "C" fn(Ptr, *const ServerInfo, Ptr);
    type SinkCb = unsafe extern "C" fn(Ptr, *const SinkInfo, c_int, Ptr);

    macro_rules! api {
        ($($field:ident: $sym:literal $ty:ty,)*) => {
            pub struct Api {
                $($field: $ty,)*
                _lib: Library,
            }
            fn load(lib: Library) -> Option<Api> {
                unsafe {
                    Some(Api {
                        $($field: *lib.get::<$ty>(concat!($sym, "\0").as_bytes()).ok()?,)*
                        _lib: lib,
                    })
                }
            }
        };
    }

    api! {
        ml_new: "pa_threaded_mainloop_new" unsafe extern "C" fn() -> Ptr,
        ml_start: "pa_threaded_mainloop_start" unsafe extern "C" fn(Ptr) -> c_int,
        ml_stop: "pa_threaded_mainloop_stop" unsafe extern "C" fn(Ptr),
        ml_free: "pa_threaded_mainloop_free" unsafe extern "C" fn(Ptr),
        ml_lock: "pa_threaded_mainloop_lock" unsafe extern "C" fn(Ptr),
        ml_unlock: "pa_threaded_mainloop_unlock" unsafe extern "C" fn(Ptr),
        ml_wait: "pa_threaded_mainloop_wait" unsafe extern "C" fn(Ptr),
        ml_signal: "pa_threaded_mainloop_signal" unsafe extern "C" fn(Ptr, c_int),
        ml_api: "pa_threaded_mainloop_get_api" unsafe extern "C" fn(Ptr) -> Ptr,
        ctx_new: "pa_context_new" unsafe extern "C" fn(Ptr, *const c_char) -> Ptr,
        ctx_state_cb: "pa_context_set_state_callback" unsafe extern "C" fn(Ptr, Option<Notify>, Ptr),
        ctx_connect: "pa_context_connect" unsafe extern "C" fn(Ptr, *const c_char, c_int, Ptr) -> c_int,
        ctx_state: "pa_context_get_state" unsafe extern "C" fn(Ptr) -> c_int,
        ctx_disconnect: "pa_context_disconnect" unsafe extern "C" fn(Ptr),
        ctx_unref: "pa_context_unref" unsafe extern "C" fn(Ptr),
        server_info: "pa_context_get_server_info" unsafe extern "C" fn(Ptr, ServerCb, Ptr) -> Ptr,
        sink_info: "pa_context_get_sink_info_by_name" unsafe extern "C" fn(Ptr, *const c_char, SinkCb, Ptr) -> Ptr,
        op_state: "pa_operation_get_state" unsafe extern "C" fn(Ptr) -> c_int,
        op_unref: "pa_operation_unref" unsafe extern "C" fn(Ptr),
        st_new: "pa_stream_new" unsafe extern "C" fn(Ptr, *const c_char, *const SampleSpec, *const ChannelMap) -> Ptr,
        st_connect: "pa_stream_connect_playback" unsafe extern "C" fn(Ptr, *const c_char, *const BufferAttr, c_int, Ptr, Ptr) -> c_int,
        st_state: "pa_stream_get_state" unsafe extern "C" fn(Ptr) -> c_int,
        st_state_cb: "pa_stream_set_state_callback" unsafe extern "C" fn(Ptr, Option<Notify>, Ptr),
        st_write_cb: "pa_stream_set_write_callback" unsafe extern "C" fn(Ptr, Option<Request>, Ptr),
        st_underflow_cb: "pa_stream_set_underflow_callback" unsafe extern "C" fn(Ptr, Option<Notify>, Ptr),
        st_writable: "pa_stream_writable_size" unsafe extern "C" fn(Ptr) -> usize,
        st_write: "pa_stream_write" unsafe extern "C" fn(Ptr, *const c_void, usize, Option<unsafe extern "C" fn(Ptr)>, i64, c_int) -> c_int,
        st_disconnect: "pa_stream_disconnect" unsafe extern "C" fn(Ptr) -> c_int,
        st_unref: "pa_stream_unref" unsafe extern "C" fn(Ptr),
    }

    // The function pointers and the library handle are plain data.
    unsafe impl Send for Api {}
    unsafe impl Sync for Api {}

    /// The library, loaded once (`None`: not installed, or without the calls).
    pub fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            // `NV_PULSE` names libpulse; the Nix package sets `NV_PULSE_SIMPLE` (the file of
            // libpulse-simple), whose sibling `libpulse.so.0` is used when `NV_PULSE` is not set.
            let lib = match std::env::var_os("NV_PULSE") {
                Some(path) => Library::new(path),
                None => match std::env::var("NV_PULSE_SIMPLE") {
                    Ok(p) if p.contains("libpulse-simple") => {
                        Library::new(p.replace("libpulse-simple", "libpulse"))
                    }
                    _ => Library::new("libpulse.so.0"),
                },
            }
            .ok()?;
            load(lib)
        })
        .as_ref()
    }

    /// State shared with the callbacks, which run on the library's thread.
    struct Shared {
        api: &'static Api,
        ml: Ptr,
        underruns: AtomicU64,
        // Probe results.
        sink: Option<CString>,
        map: Option<Vec<i32>>,
        done: bool,
    }

    unsafe extern "C" fn on_state(_: Ptr, ud: Ptr) {
        let s = &*(ud as *const Shared);
        (s.api.ml_signal)(s.ml, 0);
    }
    unsafe extern "C" fn on_write(_: Ptr, _: usize, ud: Ptr) {
        let s = &*(ud as *const Shared);
        (s.api.ml_signal)(s.ml, 0);
    }
    unsafe extern "C" fn on_underflow(_: Ptr, ud: Ptr) {
        let s = &*(ud as *const Shared);
        s.underruns.fetch_add(1, Ordering::Relaxed);
    }
    unsafe extern "C" fn on_server(_: Ptr, info: *const ServerInfo, ud: Ptr) {
        let s = &mut *(ud as *mut Shared);
        if !info.is_null() && !(*info).default_sink_name.is_null() {
            s.sink = Some(std::ffi::CStr::from_ptr((*info).default_sink_name).to_owned());
        }
        s.done = true;
        (s.api.ml_signal)(s.ml, 0);
    }
    unsafe extern "C" fn on_sink(_: Ptr, info: *const SinkInfo, eol: c_int, ud: Ptr) {
        let s = &mut *(ud as *mut Shared);
        if eol == 0 && !info.is_null() {
            let m = &(*info).channel_map;
            let n = usize::from(m.channels).min(32);
            s.map = Some(m.map[..n].to_vec());
        } else {
            s.done = true;
            (s.api.ml_signal)(s.ml, 0);
        }
    }

    /// A connected context on a running threaded main loop.
    struct Conn {
        shared: Box<Shared>,
        ctx: Ptr,
    }

    impl Conn {
        fn open(api: &'static Api) -> Option<Conn> {
            unsafe {
                let ml = (api.ml_new)();
                if ml.is_null() {
                    return None;
                }
                let mut shared = Box::new(Shared {
                    api,
                    ml,
                    underruns: AtomicU64::new(0),
                    sink: None,
                    map: None,
                    done: false,
                });
                let ud = &mut *shared as *mut Shared as Ptr;
                let name = CString::new("nv-rs").expect("no nul");
                let ctx = (api.ctx_new)((api.ml_api)(ml), name.as_ptr());
                if ctx.is_null() {
                    (api.ml_free)(ml);
                    return None;
                }
                (api.ctx_state_cb)(ctx, Some(on_state), ud);
                (api.ml_lock)(ml);
                let ok = (api.ml_start)(ml) >= 0
                    && (api.ctx_connect)(ctx, std::ptr::null(), 0, std::ptr::null_mut()) >= 0;
                let mut ready = false;
                if ok {
                    loop {
                        match (api.ctx_state)(ctx) {
                            CONTEXT_READY => {
                                ready = true;
                                break;
                            }
                            CONTEXT_FAILED | CONTEXT_TERMINATED => break,
                            _ => (api.ml_wait)(ml),
                        }
                    }
                }
                (api.ml_unlock)(ml);
                let conn = Conn { shared, ctx };
                // Dropping a failed connection tears it down.
                ready.then_some(conn)
            }
        }

        fn api(&self) -> &'static Api {
            self.shared.api
        }
    }

    impl Drop for Conn {
        fn drop(&mut self) {
            let api = self.api();
            let ml = self.shared.ml;
            unsafe {
                (api.ml_lock)(ml);
                (api.ctx_state_cb)(self.ctx, None, std::ptr::null_mut());
                (api.ctx_disconnect)(self.ctx);
                (api.ctx_unref)(self.ctx);
                (api.ml_unlock)(ml);
                (api.ml_stop)(ml);
                (api.ml_free)(ml);
            }
        }
    }

    /// The default sink's channel map (`pa_context_get_server_info`, then
    /// `pa_context_get_sink_info_by_name`); `None` with no library, no server or no sink.
    pub fn probe_channel_map() -> Option<Vec<i32>> {
        let api = api()?;
        let mut conn = Conn::open(api)?;
        let ml = conn.shared.ml;
        let ctx = conn.ctx;
        let ud = &mut *conn.shared as *mut Shared as Ptr;
        unsafe {
            (api.ml_lock)(ml);
            conn.shared.done = false;
            let op = (api.server_info)(ctx, on_server, ud);
            while !conn.shared.done && !op.is_null() && (api.op_state)(op) == OPERATION_RUNNING {
                (api.ml_wait)(ml);
            }
            if !op.is_null() {
                (api.op_unref)(op);
            }
            if let Some(sink) = conn.shared.sink.clone() {
                conn.shared.done = false;
                let op = (api.sink_info)(ctx, sink.as_ptr(), on_sink, ud);
                while !conn.shared.done && !op.is_null() && (api.op_state)(op) == OPERATION_RUNNING
                {
                    (api.ml_wait)(ml);
                }
                if !op.is_null() {
                    (api.op_unref)(op);
                }
            }
            (api.ml_unlock)(ml);
        }
        conn.shared.map.take()
    }

    pub struct Stream {
        conn: Conn,
        stream: Ptr,
    }

    // The handles are used under the main loop's lock only.
    unsafe impl Send for Stream {}

    impl Stream {
        /// Opens a float stream in `layout`'s channel map (WAVE order) at [`MIX_RATE`].
        pub fn open(api: &'static Api, layout: Layout) -> Result<Stream, Refused> {
            let conn = Conn::open(api).ok_or(Refused::NoDevice)?;
            let positions = channel_map_of(layout);
            let mut map = ChannelMap {
                channels: positions.len() as u8,
                map: [0; 32],
            };
            map.map[..positions.len()].copy_from_slice(&positions);
            let spec = SampleSpec {
                format: SAMPLE_FLOAT32LE,
                rate: MIX_RATE,
                channels: layout.channels as u8,
            };
            let frame = 4 * u32::from(layout.channels);
            // 80 ms of latency; the rest is the server's choice (-1).
            let attr = BufferAttr {
                maxlength: u32::MAX,
                tlength: MIX_RATE / 1000 * 80 * frame,
                prebuf: u32::MAX,
                minreq: u32::MAX,
                fragsize: u32::MAX,
            };
            let name = CString::new("game").expect("no nul");
            let ml = conn.shared.ml;
            let ud = &*conn.shared as *const Shared as Ptr;
            unsafe {
                (api.ml_lock)(ml);
                let stream = (api.st_new)(conn.ctx, name.as_ptr(), &spec, &map);
                if stream.is_null() {
                    (api.ml_unlock)(ml);
                    return Err(Refused::NoDevice);
                }
                (api.st_state_cb)(stream, Some(on_state), ud);
                (api.st_write_cb)(stream, Some(on_write), ud);
                (api.st_underflow_cb)(stream, Some(on_underflow), ud);
                let ok = (api.st_connect)(
                    stream,
                    std::ptr::null(),
                    &attr,
                    STREAM_ADJUST_LATENCY,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                ) >= 0;
                let mut ready = false;
                if ok {
                    loop {
                        match (api.st_state)(stream) {
                            STREAM_READY => {
                                ready = true;
                                break;
                            }
                            // Failed or terminated.
                            3 | 4 => break,
                            _ => (api.ml_wait)(ml),
                        }
                    }
                }
                (api.ml_unlock)(ml);
                let s = Stream { conn, stream };
                if ready {
                    Ok(s)
                } else {
                    Err(Refused::NoDevice)
                }
            }
        }
    }

    impl Sink for Stream {
        fn write(&mut self, frames: &[f32]) -> bool {
            let api = self.conn.api();
            let ml = self.conn.shared.ml;
            let bytes = std::mem::size_of_val(frames);
            unsafe {
                (api.ml_lock)(ml);
                let ok = loop {
                    if (api.st_state)(self.stream) != STREAM_READY {
                        break false;
                    }
                    if (api.st_writable)(self.stream) >= bytes {
                        break (api.st_write)(
                            self.stream,
                            frames.as_ptr().cast(),
                            bytes,
                            None,
                            0,
                            0,
                        ) >= 0;
                    }
                    (api.ml_wait)(ml);
                };
                (api.ml_unlock)(ml);
                ok
            }
        }
        fn paces(&self) -> bool {
            true
        }
        fn underruns(&self) -> u64 {
            self.conn.shared.underruns.load(Ordering::Relaxed)
        }
    }

    impl Drop for Stream {
        fn drop(&mut self) {
            let api = self.conn.api();
            let ml = self.conn.shared.ml;
            unsafe {
                (api.ml_lock)(ml);
                (api.st_state_cb)(self.stream, None, std::ptr::null_mut());
                (api.st_write_cb)(self.stream, None, std::ptr::null_mut());
                (api.st_underflow_cb)(self.stream, None, std::ptr::null_mut());
                (api.st_disconnect)(self.stream);
                (api.st_unref)(self.stream);
                (api.ml_unlock)(ml);
            }
            // `conn` drops after this and closes the context and the loop.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that counts what it was asked for and fills a constant.
    struct Counting {
        frames: Arc<Mutex<(u64, u16)>>,
    }

    impl Source for Counting {
        fn render(&mut self, rate: u32, channels: u16, out: &mut [f32]) {
            assert_eq!(rate, MIX_RATE);
            out.fill(0.25);
            let mut f = self.frames.lock().unwrap();
            f.0 += (out.len() / usize::from(channels)) as u64;
            f.1 = channels;
        }
    }

    #[test]
    fn the_backend_is_read_from_its_name() {
        assert_eq!(Backend::parse(None), Backend::Auto);
        assert_eq!(Backend::parse(Some("pulse")), Backend::Pulse);
        assert_eq!(Backend::parse(Some("null")), Backend::Null);
        assert_eq!(Backend::parse(Some("jack")), Backend::Auto);
    }

    #[test]
    fn the_layout_override_is_read_from_its_name() {
        assert_eq!(LayoutChoice::parse(None), LayoutChoice::Auto);
        assert_eq!(LayoutChoice::parse(Some("stereo")), LayoutChoice::Stereo);
        assert_eq!(LayoutChoice::parse(Some("5.1")), LayoutChoice::FiveOne);
        assert_eq!(LayoutChoice::parse(Some(" 7.1 ")), LayoutChoice::SevenOne);
        assert_eq!(LayoutChoice::parse(Some("auto")), LayoutChoice::Auto);
        assert_eq!(LayoutChoice::parse(Some("9.2")), LayoutChoice::Auto);
    }

    use super::position::*;
    const STEREO_MAP: [i32; 2] = [FRONT_LEFT, FRONT_RIGHT];
    // The order Pulse reports for 5.1 and 7.1 is not the WAVE order.
    const FIVE_ONE: [i32; 6] = [
        FRONT_LEFT,
        FRONT_RIGHT,
        FRONT_CENTER,
        LFE,
        REAR_LEFT,
        REAR_RIGHT,
    ];
    const FIVE_ONE_SIDE: [i32; 6] = [
        FRONT_LEFT,
        FRONT_RIGHT,
        FRONT_CENTER,
        LFE,
        SIDE_LEFT,
        SIDE_RIGHT,
    ];
    const SEVEN_ONE: [i32; 8] = [
        FRONT_LEFT,
        FRONT_RIGHT,
        FRONT_CENTER,
        LFE,
        REAR_LEFT,
        REAR_RIGHT,
        SIDE_LEFT,
        SIDE_RIGHT,
    ];

    #[test]
    fn a_channel_map_becomes_a_wave_mask() {
        let l = |m: &[i32]| layout_of_map(m);
        assert_eq!(l(&[MONO]), (Layout::MONO, true));
        assert_eq!(l(&STEREO_MAP), (Layout::STEREO, true));
        let quad = [FRONT_LEFT, FRONT_RIGHT, REAR_LEFT, REAR_RIGHT];
        assert_eq!(l(&quad), (Layout::of_mix(4, Some(0x33)), true));
        assert_eq!(l(&FIVE_ONE), (Layout::of_mix(6, Some(0x3f)), true));
        assert_eq!(l(&FIVE_ONE_SIDE), (Layout::of_mix(6, Some(0x60f)), true));
        assert_eq!(l(&SEVEN_ONE), (Layout::of_mix(8, Some(0x63f)), true));
        // Order does not matter, only the set.
        let shuffled = [
            LFE,
            FRONT_CENTER,
            FRONT_RIGHT,
            FRONT_LEFT,
            REAR_RIGHT,
            REAR_LEFT,
        ];
        assert_eq!(l(&shuffled), (Layout::of_mix(6, Some(0x3f)), true));
    }

    #[test]
    fn an_unsupported_map_falls_back_to_the_nearest_layout() {
        // 3.0: no provided layout is a subset but stereo.
        let three = [FRONT_LEFT, FRONT_RIGHT, FRONT_CENTER];
        assert_eq!(layout_of_map(&three), (Layout::STEREO, false));
        // 6.1 (rear centre, position 4, is not a bit here): the 7.1 speakers it has are 5.1.
        let six_one = [
            FRONT_LEFT,
            FRONT_RIGHT,
            FRONT_CENTER,
            LFE,
            4,
            SIDE_LEFT,
            SIDE_RIGHT,
        ];
        assert_eq!(
            layout_of_map(&six_one),
            (Layout::of_mix(6, Some(0x60f)), false)
        );
        assert_eq!(layout_of_map(&[]), (Layout::STEREO, false));
    }

    #[test]
    fn the_stream_is_opened_in_wave_order() {
        assert_eq!(channel_map_of(Layout::STEREO), STEREO_MAP);
        assert_eq!(channel_map_of(Layout::MONO), [MONO]);
        assert_eq!(channel_map_of(Layout::of_mix(6, Some(0x3f))), FIVE_ONE);
        assert_eq!(
            channel_map_of(Layout::of_mix(6, Some(0x60f))),
            FIVE_ONE_SIDE
        );
        assert_eq!(channel_map_of(Layout::of_mix(8, Some(0x63f))), SEVEN_ONE);
        // And it reads back to the same layout.
        for mask in [0x3u32, 0x33, 0x3f, 0x60f, 0x63f] {
            let l = Layout::of_mix(mask.count_ones() as u16, Some(mask));
            assert_eq!(layout_of_map(&channel_map_of(l)), (l, true));
        }
    }

    #[test]
    fn the_probe_and_the_override_choose_the_layout() {
        let probe = |m: &'static [i32]| move || Some(m.to_vec());
        let five = Layout::of_mix(6, Some(0x3f));
        let seven = Layout::of_mix(8, Some(0x63f));
        // Fake probe: the engine's device mix and `GetSpeakerConfig` follow it.
        let got = choose_layout(LayoutChoice::Auto, probe(&FIVE_ONE));
        assert_eq!((got.channels, got.mask), (6, 0x3f));
        assert_eq!(crate::speakers::speaker_config(got.mask), Some(6));
        let got = choose_layout(LayoutChoice::Auto, probe(&SEVEN_ONE));
        assert_eq!((got.channels, got.mask), (8, 0x63f));
        assert_eq!(crate::speakers::speaker_config(got.mask), Some(8));
        // No server: stereo.
        assert_eq!(choose_layout(LayoutChoice::Auto, || None), Layout::STEREO);
        // The override wins over the probe.
        assert_eq!(
            choose_layout(LayoutChoice::Stereo, probe(&SEVEN_ONE)),
            Layout::STEREO
        );
        assert_eq!(
            choose_layout(LayoutChoice::SevenOne, probe(&STEREO_MAP)),
            seven
        );
        assert_eq!(choose_layout(LayoutChoice::FiveOne, || None), five);
        // 5.1 keeps a side sink's side speakers.
        assert_eq!(
            choose_layout(LayoutChoice::FiveOne, probe(&FIVE_ONE_SIDE)).mask,
            0x60f
        );
    }

    #[test]
    fn the_null_sink_takes_six_and_eight_channels() {
        for (layout, n) in [
            (Layout::of_mix(6, Some(0x3f)), 6),
            (Layout::of_mix(8, Some(0x63f)), 8),
        ] {
            let seen = Arc::new(Mutex::new((0, 0)));
            let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Counting {
                frames: seen.clone(),
            }));
            let out = start(Backend::Null, source, Some(layout)).expect("opens");
            assert_eq!(out.layout, layout);
            std::thread::sleep(Duration::from_millis(60));
            drop(out);
            assert_eq!(seen.lock().unwrap().1, n);
        }
    }

    #[test]
    fn a_sink_that_reports_underruns_reaches_the_source() {
        struct Starving(Arc<Mutex<Vec<u64>>>);
        impl Source for Starving {
            fn render(&mut self, _: u32, _: u16, out: &mut [f32]) {
                out.fill(0.0);
            }
            fn starved(&mut self, count: u64) {
                self.0.lock().unwrap().push(count);
            }
        }
        struct Late(Arc<AtomicBool>);
        impl Sink for Late {
            fn write(&mut self, _: &[f32]) -> bool {
                true
            }
            fn paces(&self) -> bool {
                false
            }
            fn underruns(&self) -> u64 {
                if self.0.load(Ordering::Relaxed) {
                    3
                } else {
                    0
                }
            }
        }
        let counts = Arc::new(Mutex::new(Vec::new()));
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Starving(counts.clone())));
        let out = spawn(source, Layout::STEREO, "test", move |_| {
            Ok(Box::new(Late(f)))
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(60));
        flag.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));
        drop(out);
        assert_eq!(*counts.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn the_null_sink_asks_for_frames_on_the_wall_clock() {
        let seen = Arc::new(Mutex::new((0, 0)));
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Counting {
            frames: seen.clone(),
        }));
        let out = start(Backend::Null, source, None).expect("the null sink opens");
        assert_eq!((out.rate, out.layout), (MIX_RATE, Layout::STEREO));
        std::thread::sleep(Duration::from_millis(400));
        drop(out);
        let (frames, channels) = *seen.lock().unwrap();
        assert_eq!(channels, 2);
        // About 19200 frames in 400 ms: the thread keeps the clock, neither stalling nor racing.
        assert!(
            (9_600..=28_800).contains(&frames),
            "{frames} frames in 400 ms"
        );
    }

    #[test]
    fn the_null_sink_takes_the_layout_asked_for() {
        let seen = Arc::new(Mutex::new((0, 0)));
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Counting {
            frames: seen.clone(),
        }));
        let five_one = Layout::of_mix(6, Some(0x3f));
        let out = start(Backend::Null, source, Some(five_one)).expect("opens");
        std::thread::sleep(Duration::from_millis(100));
        drop(out);
        assert_eq!(seen.lock().unwrap().1, 6);
    }

    #[test]
    fn a_layout_directsound_does_not_place_is_refused() {
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Counting {
            frames: Arc::new(Mutex::new((0, 0))),
        }));
        let odd = Layout::of_mix(3, Some(0x7));
        assert!(!odd.provided());
        assert!(matches!(
            start(Backend::Null, source, Some(odd)),
            Err(Refused::Format(_))
        ));
    }

    #[test]
    fn the_sink_gets_what_the_source_rendered() {
        // The sink sees what `run` hands it: record it through a sink of our own.
        struct Peak(Arc<Mutex<f32>>);
        impl Sink for Peak {
            fn write(&mut self, frames: &[f32]) -> bool {
                let p = frames.iter().fold(0f32, |m, v| m.max(v.abs()));
                let mut g = self.0.lock().unwrap();
                *g = g.max(p);
                true
            }
            fn paces(&self) -> bool {
                false
            }
        }
        let seen = Arc::new(Mutex::new((0, 0)));
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Counting {
            frames: seen.clone(),
        }));
        let peak = Arc::new(Mutex::new(0f32));
        let p = peak.clone();
        let out = spawn(source, Layout::STEREO, "test", move |_| {
            Ok(Box::new(Peak(p)))
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        drop(out);
        assert_eq!(*peak.lock().unwrap(), 0.25);
        assert!(seen.lock().unwrap().0 > 0);
    }

    #[test]
    fn an_unreachable_server_is_no_device_and_auto_falls_back() {
        // With the library absent or no server the pulse path must refuse, not panic; with a
        // server it opens. Either way `auto` ends with a running stream.
        // A silent source: on a machine with a sound server this opens its real stream.
        struct Silent;
        impl Source for Silent {
            fn render(&mut self, _rate: u32, _channels: u16, out: &mut [f32]) {
                out.fill(0.0);
            }
        }
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Silent));
        assert!(start(Backend::Auto, source, None).is_ok());
    }

    /// Needs a sound server: `cargo test -p engine-platform fork_sound -- --ignored`. Plays 0.4 s
    /// of a quiet 440 Hz tone on the default sink and checks the pulse stream opened.
    #[test]
    #[ignore = "needs a PipeWire or PulseAudio session and plays a quiet tone"]
    fn a_tone_reaches_the_default_sink() {
        struct Tone(f32);
        impl Source for Tone {
            fn render(&mut self, rate: u32, channels: u16, out: &mut [f32]) {
                for frame in out.chunks_mut(usize::from(channels)) {
                    let v = 0.05 * (self.0 * std::f32::consts::TAU).sin();
                    self.0 = (self.0 + 440.0 / rate as f32).fract();
                    frame.fill(v);
                }
            }
        }
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(Tone(0.0)));
        let out = start(Backend::Pulse, source, None).expect("a sound server answers");
        std::thread::sleep(Duration::from_millis(400));
        drop(out);
    }
    /// Needs a sound server: plays 0.4 s of a quiet tone on each channel of the default sink's own
    /// layout in turn (nothing needs listening to) and logs the map the sink reports.
    #[test]
    #[ignore = "needs a PipeWire or PulseAudio session and plays quiet tones"]
    fn a_tone_per_channel_reaches_the_default_sink() {
        let map = pulse::probe_channel_map();
        eprintln!("sink channel map: {map:?}");
        let layout = choose_layout(LayoutChoice::from_env(), || map.clone());
        struct PerChannel {
            channel: Arc<Mutex<usize>>,
            phase: f32,
        }
        impl Source for PerChannel {
            fn render(&mut self, rate: u32, channels: u16, out: &mut [f32]) {
                let on = *self.channel.lock().unwrap();
                for frame in out.chunks_mut(usize::from(channels)) {
                    frame.fill(0.0);
                    frame[on % usize::from(channels)] =
                        0.05 * (self.phase * std::f32::consts::TAU).sin();
                    self.phase = (self.phase + 440.0 / rate as f32).fract();
                }
            }
        }
        let channel = Arc::new(Mutex::new(0usize));
        let source: Arc<Mutex<dyn Source>> = Arc::new(Mutex::new(PerChannel {
            channel: channel.clone(),
            phase: 0.0,
        }));
        let out = start(Backend::Pulse, source, Some(layout)).expect("a sound server answers");
        for c in 0..usize::from(layout.channels) {
            *channel.lock().unwrap() = c;
            std::thread::sleep(Duration::from_millis(400));
        }
        drop(out);
    }
}
