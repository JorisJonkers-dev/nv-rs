//! The sound output off Windows (fork-owned, docs/fork/LINUX.md "Sound"): the same seam as the
//! WASAPI stream in `sound_out.rs` ([`super::sound_out::Source`], [`super::sound_out::Output`]),
//! with two backends behind it.
//!
//! * **Pulse**: libpulse-simple, loaded with `dlopen` at the first use (nothing is linked or
//!   needed to build; `libpulse-simple.so.0`, or the file `NV_PULSE_SIMPLE` names, is a run-time prerequisite: Arch `libpulse`, Debian
//!   and Ubuntu `libpulse0`, Fedora `pulseaudio-libs`). Under PipeWire the same library talks to
//!   `pipewire-pulse`, so one backend serves PipeWire and PulseAudio sessions. A stream is 32-bit
//!   float at the reference rate, in the layout asked for (the server maps it to the sink; for six
//!   and eight channels its default channel map is the WAVE order DirectSound uses).
//! * **Null**: a thread that asks the source for frames on the wall clock and drops them. It is
//!   the fallback with no sound server (a headless machine, CI) and what `NV_SOUND_BACKEND=null`
//!   asks for; the mixer runs exactly as with a device, so the game's sound path is exercised.
//!
//! `NV_SOUND_BACKEND` is `auto` (the default: Pulse, else Null with a line on stderr), `pulse`
//! (Pulse, else no device) or `null`. The mix format is fixed: [`MIX_RATE`] Hz, stereo; the
//! server resamples and remixes to the real sink.

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

/// The mix format of this target: the rate and the layout the stream is opened in when the game
/// asks for none. `None` when [`Backend::Pulse`] is asked for and the library is not there.
pub fn mix(backend: Backend) -> Option<(u32, Layout)> {
    match backend {
        Backend::Pulse if pulse::api().is_none() => None,
        _ => Some((MIX_RATE, Layout::STEREO)),
    }
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
            eprintln!(
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
    while !stop.load(Ordering::Relaxed) {
        chunk.fill(0.0);
        if let Ok(mut s) = source.lock() {
            s.render(MIX_RATE, channels, &mut chunk);
        }
        if super::sound_out::is_muted() {
            chunk.fill(0.0);
        }
        if !sink.write(&chunk) {
            if !lost {
                lost = true;
                eprintln!(
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

/// libpulse-simple through `dlopen`: the three calls the stream needs.
mod pulse {
    use super::*;
    use libloading::Library;
    use std::sync::OnceLock;

    /// `pa_sample_spec`.
    #[repr(C)]
    struct SampleSpec {
        format: c_int,
        rate: u32,
        channels: u8,
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

    /// `PA_SAMPLE_FLOAT32LE`, `PA_STREAM_PLAYBACK`.
    const SAMPLE_FLOAT32LE: c_int = 5;
    const STREAM_PLAYBACK: c_int = 1;

    type New = unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        c_int,
        *const c_char,
        *const c_char,
        *const SampleSpec,
        *const c_void,
        *const BufferAttr,
        *mut c_int,
    ) -> *mut c_void;
    type Write = unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int;
    type Drain = unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int;
    type Free = unsafe extern "C" fn(*mut c_void);

    pub struct Api {
        new: New,
        write: Write,
        drain: Drain,
        free: Free,
        _lib: Library,
    }

    /// The library, loaded once (`None`: not installed, or without the calls).
    pub fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            // `NV_PULSE_SIMPLE` names the file (the Nix package sets it); else the library path's.
            let lib = match std::env::var_os("NV_PULSE_SIMPLE") {
                Some(path) => Library::new(path),
                None => Library::new("libpulse-simple.so.0"),
            }
            .ok()?;
            let new = *lib.get::<New>(b"pa_simple_new\0").ok()?;
            let write = *lib.get::<Write>(b"pa_simple_write\0").ok()?;
            let drain = *lib.get::<Drain>(b"pa_simple_drain\0").ok()?;
            let free = *lib.get::<Free>(b"pa_simple_free\0").ok()?;
            Some(Api {
                new,
                write,
                drain,
                free,
                _lib: lib,
            })
        })
        .as_ref()
    }

    pub struct Stream {
        api: &'static Api,
        handle: *mut c_void,
    }

    // The handle is used by the one thread that opened it.
    unsafe impl Send for Stream {}

    impl Stream {
        pub fn open(api: &'static Api, layout: Layout) -> Result<Stream, Refused> {
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
            let app = CString::new("nv-rs").expect("no nul");
            let name = CString::new("game").expect("no nul");
            let mut error: c_int = 0;
            let handle = unsafe {
                (api.new)(
                    std::ptr::null(),
                    app.as_ptr(),
                    STREAM_PLAYBACK,
                    std::ptr::null(),
                    name.as_ptr(),
                    &spec,
                    std::ptr::null(),
                    &attr,
                    &mut error,
                )
            };
            if handle.is_null() {
                return Err(Refused::NoDevice);
            }
            Ok(Stream { api, handle })
        }
    }

    impl Sink for Stream {
        fn write(&mut self, frames: &[f32]) -> bool {
            let mut error: c_int = 0;
            unsafe {
                (self.api.write)(
                    self.handle,
                    frames.as_ptr().cast(),
                    std::mem::size_of_val(frames),
                    &mut error,
                ) >= 0
            }
        }
        fn paces(&self) -> bool {
            true
        }
    }

    impl Drop for Stream {
        fn drop(&mut self) {
            let mut error: c_int = 0;
            unsafe {
                (self.api.drain)(self.handle, &mut error);
                (self.api.free)(self.handle);
            }
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
}
