//! Fork-owned (docs/fork/LINUX.md, "Direct3D 9 off Windows"): the Direct3D 9 recorder of
//! `d3d9real.rs` off Windows. The recorder itself (the calls recorded as `d3d9_stream::Cmd`) is the
//! one the Windows build runs; what is Windows only there (the real `d3d9.dll`, the game's window,
//! the F12 report) is left out, and the stream's backend is wgpu alone, off screen
//! (`d3d9_wgpu::Output::Offscreen`): no window is made, the frames are read back by a test or
//! a tool (`NV_D3D9_RECORD` files replay with `d3d9-replay`).

use super::*;

/// The wgpu backend, off screen, on the stream `which` names: the real Direct3D 9 is not there,
/// so `real` and `real,wgpu` draw on wgpu too (a line says so). With the render thread, the
/// occlusion counts are posted for the game's side to read without waiting
/// (`d3d9_wgpu::QueryAnswers`), as the Windows build does for wgpu alone.
pub(super) fn make_stream(e: &mut Engine, which: Backends) -> Stream {
    let name = which.name();
    if which != Backends::Wgpu {
        eprintln!(
            "NV_D3D9={name}: there is no real Direct3D 9 here: wgpu draws off screen instead"
        );
    }
    let wanted_thread = real(e).render_thread;
    let wgpu_cache = real(e).wgpu_cache.clone();
    let backend = |answers: Option<std::sync::Arc<d3d9_wgpu::QueryAnswers>>| {
        let b = d3d9_wgpu::WgpuBackend::new(d3d9_wgpu::Output::Offscreen);
        let b = match wgpu_cache.clone() {
            Some(dir) => b.with_cache_dir(dir),
            None => b,
        };
        match answers {
            Some(a) => b.with_answers(a),
            None => b,
        }
    };
    let mut stream = if wanted_thread {
        let settings = d3d9_stream::Settings::from_env();
        let answers = std::sync::Arc::new(d3d9_wgpu::QueryAnswers::default());
        real(e).wgpu_answers = Some(answers.clone());
        eprintln!(
            "NV_D3D9={name}: the render thread is on (NV_RENDER_THREAD=0 turns it off; at most {} frame(s) in flight)",
            settings.frames_in_flight
        );
        Stream::threaded(Box::new(backend(Some(answers))), settings)
    } else {
        Stream::direct(Box::new(backend(None)))
    };
    if let Some(path) = std::env::var_os("NV_D3D9_RECORD") {
        let repeatable = e.repeatable;
        let recording = std::fs::File::create(&path).and_then(|file| {
            let out = Box::new(std::io::BufWriter::with_capacity(1 << 20, file));
            if repeatable {
                stream.record_repeatable(out)
            } else {
                stream.record_to(out)
            }
        });
        match recording {
            Ok(()) => eprintln!(
                "NV_D3D9_RECORD: every Direct3D 9 command goes to {}",
                std::path::Path::new(&path).display()
            ),
            Err(error) => eprintln!(
                "NV_D3D9_RECORD: {} could not be written ({error}): not recorded",
                std::path::Path::new(&path).display()
            ),
        }
    }
    stream
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::d3d9;
    use d3d9_stream::Backend;

    /// The backend of the stream, shared with the test, which reads the frame back from it.
    struct Shared(std::sync::Arc<std::sync::Mutex<d3d9_wgpu::WgpuBackend>>);

    impl Backend for Shared {
        fn execute(&mut self, cmd: Cmd<Data<'_>>) -> Reply {
            self.0.lock().unwrap().execute(cmd)
        }
    }

    /// TEST DATA, not a capture: the adapter table of `d3d9device`'s tests (a 640x480 X8R8G8B8
    /// desktop, HAL caps, a windowed X8R8G8B8 back buffer accepted).
    fn table() -> String {
        let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
        let mut mode = vec![0u8; 16];
        mode[..4].copy_from_slice(&640u32.to_le_bytes());
        mode[4..8].copy_from_slice(&480u32.to_le_bytes());
        mode[8..12].copy_from_slice(&60u32.to_le_bytes());
        mode[12..].copy_from_slice(&0x16u32.to_le_bytes());
        let mut caps = vec![0u8; 304];
        caps[..4].copy_from_slice(&1u32.to_le_bytes());
        [
            format!("GetAdapterDisplayMode\t0\t0\t{}", hex(&mode)),
            format!("GetDeviceCaps\t0 1\t0\t{}", hex(&caps)),
            "Domain\td0\t0,1".to_string(),
            "CheckDeviceType\t0 1 16 16\t0\t\td0".to_string(),
        ]
        .join("\n")
    }

    /// A guest program through the engine: `Direct3DCreate9`, a device (64x64, X8R8G8B8, no
    /// window), a clear to green, a triangle drawn with a vertex and a pixel shader, `Present`.
    /// The recorder hands the calls to wgpu off screen (lavapipe on CI) and the back buffer is
    /// read back: the cleared colour in the corner, the triangle's colour at its centre.
    #[test]
    fn a_guest_program_draws_a_frame_through_the_recorder_on_wgpu() {
        let backend = std::sync::Arc::new(std::sync::Mutex::new(d3d9_wgpu::WgpuBackend::new(
            d3d9_wgpu::Output::Offscreen,
        )));
        let mut e = Engine::new();
        d3d9::load_adapter_table(&mut e, &table());
        let create = d3d9::proc_address(&mut e, b"Direct3DCreate9").unwrap();
        let d3d = e.call(create, &[d3d9::D3D_SDK_VERSION]).u32();
        // What `NV_D3D9=wgpu` makes at the first `Direct3DCreate9`, with the test's backend.
        real(&mut e).stream = Some(Stream::direct(Box::new(Shared(backend.clone()))));
        let present = e.mem.alloc(0x38);
        e.mem.set_u32(present, 64);
        e.mem.set_u32(present + 4, 64);
        e.mem.set_u32(present + 0x1c, 0x11c);
        e.mem.set_u32(present + 0x20, 1);
        let out = e.mem.alloc(4);
        let hr = e
            .vcall(d3d, 4 * 16, &[0, 1, 0x11c, 0x40, present, out])
            .u32();
        assert_eq!(hr, 0, "CreateDevice");
        let device = e.mem.u32(out);
        if backend.lock().unwrap().device().is_none() {
            assert!(
                std::env::var_os("NV_REQUIRE_GPU").is_none(),
                "NV_REQUIRE_GPU is set and wgpu has no adapter"
            );
            eprintln!("no GPU adapter: the headless frame test is skipped");
            return;
        }
        assert!(real(&mut e).devices.iter().any(|(d, _)| *d == device));
        let words = |e: &mut Engine, w: &[u32]| {
            let at = e.mem.alloc(4 * w.len() as u32);
            for (i, x) in w.iter().enumerate() {
                e.mem.set_u32(at + 4 * i as u32, *x);
            }
            at
        };
        // vs_2_0: dcl_position v0; mov oPos, v0. ps_3_0: mov oC0, c0.
        let vs = words(
            &mut e,
            &[
                0xFFFE_0200,
                0x0200_001F,
                0x8000_0000,
                0x900F_0000,
                0x0200_0001,
                0xC00F_0000,
                0x90E4_0000,
                0x0000_FFFF,
            ],
        );
        let ps = words(
            &mut e,
            &[
                0xFFFF_0300,
                0x0200_0001,
                0x800F_0800,
                0xA0E4_0000,
                0x0000_FFFF,
            ],
        );
        let elements = e.mem.alloc(16);
        // `D3DVERTEXELEMENT9`: stream 0, offset 0, FLOAT4 (3), method 0, usage POSITION (0), index 0.
        e.mem.set_u16(elements, 0);
        e.mem.set_u16(elements + 2, 0);
        e.mem.set_u8(elements + 4, 3);
        e.mem.set_u8(elements + 5, 0);
        e.mem.set_u8(elements + 6, 0);
        e.mem.set_u8(elements + 7, 0);
        e.mem.set_u16(elements + 8, 0xFF);
        e.mem.set_u16(elements + 10, 0);
        e.mem.set_u8(elements + 12, 17);
        e.mem.set_u8(elements + 13, 0);
        e.mem.set_u8(elements + 14, 0);
        e.mem.set_u8(elements + 15, 0);
        assert_eq!(
            e.vcall(device, 4 * 91, &[vs, out]).u32(),
            0,
            "CreateVertexShader"
        );
        let vs = e.mem.u32(out);
        assert_eq!(
            e.vcall(device, 4 * 106, &[ps, out]).u32(),
            0,
            "CreatePixelShader"
        );
        let ps = e.mem.u32(out);
        assert_eq!(
            e.vcall(device, 4 * 86, &[elements, out]).u32(),
            0,
            "CreateVertexDeclaration"
        );
        let decl = e.mem.u32(out);
        assert_eq!(
            e.vcall(device, 4 * 18, &[0, 0, 0, out]).u32(),
            0,
            "GetBackBuffer"
        );
        let back = e.mem.u32(out);
        let f = |x: f32| x.to_bits();
        let triangle = words(
            &mut e,
            &[
                f(-0.8),
                f(-0.8),
                f(0.0),
                f(1.0),
                f(0.0),
                f(0.8),
                f(0.0),
                f(1.0),
                f(0.8),
                f(-0.8),
                f(0.0),
                f(1.0),
            ],
        );
        let colour = words(&mut e, &[f(1.0), f(0.0), f(0.0), f(1.0)]);
        e.vcall(device, 4 * 41, &[]);
        // D3DCLEAR_TARGET, opaque green; the triangle clockwise (Direct3D culls counter-clockwise).
        e.vcall(device, 4 * 43, &[0, 0, 1, 0xFF00_FF00, f(1.0), 0]);
        e.vcall(device, 4 * 87, &[decl]);
        e.vcall(device, 4 * 92, &[vs]);
        e.vcall(device, 4 * 107, &[ps]);
        e.vcall(device, 4 * 109, &[0, colour, 1]);
        // `DrawPrimitiveUP`: D3DPT_TRIANGLELIST, one primitive, 16 bytes a vertex.
        e.vcall(device, 4 * 83, &[4, 1, triangle, 16]);
        e.vcall(device, 4 * 42, &[]);
        assert_eq!(e.vcall(device, 4 * 17, &[0, 0, 0, 0]).u32(), 0, "Present");
        real(&mut e).stream.as_mut().expect("the stream").finish();
        let mut b = backend.lock().unwrap();
        let (w, h, px) = b
            .device()
            .and_then(|d| d.read_surface_pixels(back))
            .expect("the back buffer's pixels");
        assert_eq!((w, h), (64, 64));
        let at = |x: usize, y: usize| {
            let i = 4 * (y * 64 + x);
            [px[i], px[i + 1], px[i + 2]]
        };
        // B, G, R: green where nothing was drawn, red inside the triangle.
        assert_eq!(at(2, 2), [0x00, 0xFF, 0x00], "the cleared corner");
        assert_eq!(at(32, 40), [0x00, 0x00, 0xFF], "the triangle's centre");
    }
}
