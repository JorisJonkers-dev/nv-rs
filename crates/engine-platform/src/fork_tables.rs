//! The machine tables of a Linux machine (fork-owned; `docs/fork/LINUX.md`, "Machine tables").
//!
//! On Windows the player captures the adapter table with `scripts/d3d9-capture`; nothing there
//! runs on Linux. This module writes the same table (the format `super::d3d9::Adapter::parse`
//! reads) from what a Linux machine can say: the card's identity as wgpu reports it
//! ([`d3d9_wgpu::identity`]), a list of display modes and the folders under the XDG user data
//! folder. The rest of the machine (processor, time zone, code pages, Windows' fixed answers) is
//! upstream's machine table (`super::machine_table`, slaterain/nv-rs#645), which the host puts
//! before this one.
//!
//! The answers are a shared reference profile, not a capture. The default is the owner's card (an
//! AMD Radeon RX 7900 XTX on RADV, [`Profile::radv_reference`]); the identity wgpu reports
//! replaces the name, the vendor and device ids, and a profile file ([`Profile::parse`], found by
//! `NV_LINUX_PROFILE`, else `<data_dir>/linux_profile.txt`) overrides anything for another
//! machine. The capabilities and format answers are one Shader Model 3 reference set written from
//! the Direct3D 9 specification's structures, the same for every vendor; they are not copied from
//! any machine. The sound table is [`no_device_sound_table`]: the game runs silent.
//!
//! Everything is built on every target so the tests run everywhere; the hooks that use it
//! (`host`'s loader, `dsound`'s table) are `cfg(not(windows))`.

use std::fmt::Write;

const D3D_OK: u32 = 0;
const NOT_AVAILABLE: u32 = 0x8876_086a;
/// The rate the mixer runs at off Windows (`fork_sound::MIX_RATE`).
pub const MIX_RATE: u32 = 48_000;
const DSERR_NODRIVER: u32 = 0x8878_0078;

/// The sound table that makes the game run silent: `DirectSoundCreate8` fails with
/// `DSERR_NODRIVER`, as on a machine with no sound device, and nothing else is asked.
pub fn no_device_sound_table() -> String {
    format!("DsCreate\t\t{DSERR_NODRIVER:x}\nDsEnd\n")
}

/// The sound table of a machine with a sound server (`fork_sound.rs`): one default device that
/// takes `DSSCL_PRIORITY`, written from the DirectSound headers, not captured from any machine. A
/// `dsound.tsv` in the data folder still overrides it. `DSCAPS` is a software-only device (no
/// hardware voices or memory, the continuous rate between 100 and 200000 Hz, mono and stereo, 8
/// and 16 bit); the primary buffer is 16-bit stereo at the mix rate; the listener is
/// `DS3DLISTENER`'s documented default (origin, front +z, top +y, every factor 1).
pub fn reference_sound_table() -> String {
    fn hex(words: &[u32]) -> String {
        words.iter().fold(String::new(), |mut s, w| {
            for b in w.to_le_bytes() {
                let _ = write!(s, "{b:02x}");
            }
            s
        })
    }
    let one = 1.0f32.to_bits();
    let mut caps = [0u32; 24];
    caps[0] = 0x60;
    // MONO | STEREO | 8BIT | 16BIT (primary), CONTINUOUSRATE, then the same for secondary.
    caps[1] = 0x1 | 0x2 | 0x4 | 0x8 | 0x10 | 0x100 | 0x200 | 0x400 | 0x800;
    caps[2] = 100;
    caps[3] = 200_000;
    caps[4] = 1;
    let rate = MIX_RATE;
    let primary = [1u32 | 2 << 16, rate, rate * 4, 4 | 16 << 16, 0];
    let primary_bytes = &hex(&primary)[..36];
    // DS3DLISTENER: size, position, velocity, front, top (3 words each), then three factors.
    let mut l = [0u32; 16];
    l[0] = 0x40;
    l[9] = one; // front z
    l[11] = one; // top y
    l[13] = one; // distance factor
    l[14] = one; // rolloff
    l[15] = one; // doppler
    format!(
        "DsCreate\t\t0\t\nDsCooperativeLevel\t1\t0\t\nDsCooperativeLevel\t2\t0\t\n\
         DsCaps\t\t0\t{}\nDsPrimary\t91\t0\t{} 12\nDsListener\t\t0 0\t{}\nDsEnd\n",
        hex(&caps),
        primary_bytes,
        hex(&l)
    )
}

/// What the table says about the machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    /// The adapter's description, the vendor and device ids.
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// The Direct3D 9 user-mode driver's file name (`GetAdapterIdentifier`'s `Driver`): the game
    /// picks its vendor path from it.
    pub driver: String,
    /// Display modes (width, height, refresh rate); the first is the current one.
    pub modes: Vec<(u32, u32, u32)>,
    /// The user data folder the game's folders are under (`<root>/Documents`,
    /// `<root>/AppData/Roaming`, `<root>/AppData/Local`).
    pub userdata: String,
}

/// The D3D9 driver file the game keys its vendor path on, by PCI vendor id.
pub fn driver_for_vendor(vendor: u32) -> &'static str {
    match vendor {
        0x1002 => "aticfx32.dll",
        0x10de => "nvd3dum.dll",
        0x8086 => "igdumd32.dll",
        _ => "d3d9-wgpu.dll",
    }
}

impl Profile {
    /// The owner's machine: an RX 7900 XTX (`0x1002:0x744c`) on RADV at 1920x1080 and 60 Hz.
    pub fn radv_reference(userdata: &str) -> Profile {
        Profile {
            name: "AMD Radeon RX 7900 XTX".into(),
            vendor_id: 0x1002,
            device_id: 0x744c,
            driver: driver_for_vendor(0x1002).into(),
            modes: vec![
                (1920, 1080, 60),
                (2560, 1440, 60),
                (3840, 2160, 60),
                (1680, 1050, 60),
                (1600, 900, 60),
                (1366, 768, 60),
                (1280, 1024, 60),
                (1280, 720, 60),
                (1024, 768, 60),
                (800, 600, 60),
                (640, 480, 60),
            ],
            userdata: userdata.into(),
        }
    }

    /// The reference profile with the card wgpu reports in place of the reference card's name and
    /// ids; a software adapter keeps the reference card (the game then sees the owner's identity).
    pub fn with_gpu(mut self, gpu: &d3d9_wgpu::identity::GpuIdentity) -> Profile {
        if !gpu.software {
            // wgpu names RADV cards `AMD Radeon RX 7900 XTX (RADV NAVI31)`: the part in
            // parentheses is the Vulkan driver's, not the adapter's description.
            self.name = gpu.name.split(" (").next().unwrap_or(&gpu.name).to_string();
            self.vendor_id = gpu.vendor_id;
            self.device_id = gpu.device_id;
            self.driver = driver_for_vendor(gpu.vendor_id).into();
        }
        self
    }

    /// Applies a profile file: `key = value` lines, `#` comments. Keys: `name`, `vendor_id` and
    /// `device_id` (hex), `driver`, `userdata`, and `mode = WxH@Hz` (each one adds a mode; the
    /// first listed is current and the reference modes are dropped). `bias_minutes` is taken and
    /// ignored (the time zone is the system's).
    pub fn parse(mut self, text: &str) -> Result<Profile, String> {
        let mut modes = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (key, value) = line
                .split_once('=')
                .map(|(k, v)| (k.trim(), v.trim()))
                .ok_or_else(|| format!("not a key = value line: {line}"))?;
            let hex = |v: &str| {
                u32::from_str_radix(v.trim_start_matches("0x"), 16)
                    .map_err(|_| format!("not a hex number: {line}"))
            };
            match key {
                "name" => self.name = value.into(),
                "vendor_id" => self.vendor_id = hex(value)?,
                "device_id" => self.device_id = hex(value)?,
                "driver" => self.driver = value.into(),
                "userdata" => self.userdata = value.into(),
                // Read before the machine table had the system's time zone (slaterain/nv-rs#645):
                // still taken, so an older file is not refused, and no longer used.
                "bias_minutes" => {
                    value
                        .parse::<i32>()
                        .map_err(|_| format!("not a number: {line}"))?;
                }
                "mode" => {
                    let bad = || format!("a mode is WxH@Hz: {line}");
                    let (size, hz) = value.split_once('@').ok_or_else(bad)?;
                    let (w, h) = size.split_once('x').ok_or_else(bad)?;
                    modes.push((
                        w.parse().map_err(|_| bad())?,
                        h.parse().map_err(|_| bad())?,
                        hz.parse().map_err(|_| bad())?,
                    ));
                }
                other => return Err(format!("unknown key {other}: {line}")),
            }
        }
        if !modes.is_empty() {
            self.modes = modes;
        }
        Ok(self)
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `text` as the game's ANSI bytes in a zeroed field of `len` bytes.
fn field(out: &mut [u8], at: usize, len: usize, text: &str) {
    let b = text.as_bytes();
    let n = b.len().min(len - 1);
    out[at..at + n].copy_from_slice(&b[..n]);
}

fn put(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_f(out: &mut [u8], at: usize, v: f32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// `D3DADAPTER_IDENTIFIER9` (1100 bytes): driver, description, driver version, ids, GUID, WHQL.
fn identifier(p: &Profile) -> Vec<u8> {
    let mut b = vec![0u8; 1100];
    field(&mut b, 0, 512, &p.driver);
    field(&mut b, 512, 512, &p.name);
    put(&mut b, 1024, 0x0001_5a3c); // DriverVersion low part, reference
    put(&mut b, 1028, 0x0001_001f);
    put(&mut b, 1032, p.vendor_id);
    put(&mut b, 1036, p.device_id);
    put(&mut b, 1044, 0xc8); // Revision
                             // DeviceIdentifier: a GUID derived from the ids so two cards differ.
    put(
        &mut b,
        1048,
        p.vendor_id.wrapping_mul(0x0001_0001) ^ p.device_id,
    );
    put(&mut b, 1052, p.device_id);
    put(&mut b, 1056, p.vendor_id);
    put(&mut b, 1064, 1); // WHQLLevel
    b
}

/// `D3DCAPS9::TextureCaps` of a Shader Model 3 card: PERSPECTIVE, ALPHA, TEXREPEATNOTSCALEDBYSIZE,
/// NONPOW2CONDITIONAL, PROJECTED, CUBEMAP, VOLUMEMAP, MIPMAP, MIPVOLUMEMAP, MIPCUBEMAP; not POW2
/// (0x2) and not SQUAREONLY (0x20), which would make the game round every target up to a power of
/// two square.
const TEXTURE_CAPS: u32 =
    0x1 | 0x4 | 0x40 | 0x100 | 0x400 | 0x800 | 0x2000 | 0x4000 | 0x8000 | 0x1_0000;

/// `D3DCAPS9` (304 bytes) of the reference Shader Model 3 hardware device.
fn caps(devtype: u32) -> Vec<u8> {
    let mut b = vec![0u8; 304];
    let w = |b: &mut Vec<u8>, at: usize, v: u32| put(b, at, v);
    w(&mut b, 0, devtype);
    w(&mut b, 8, 0x0002_0000); // Caps: READ_SCANLINE
    w(&mut b, 12, 0x2000_0000 | 0x0002_0000); // Caps2: CANAUTOGENMIPMAP, DYNAMICTEXTURES
    w(&mut b, 16, 0x0000_0320); // Caps3: ALPHA_FULLSCREEN_FLIP_OR_DISCARD, LINEAR_TO_SRGB, COPY_TO_VIDMEM
    w(&mut b, 20, 0x8000_0001 | 0x2 | 0x4 | 0x8); // PresentationIntervals
    w(&mut b, 24, 0x3); // CursorCaps
    w(
        &mut b,
        28,
        0x0002_0000
            | 0x0010_0000
            | 0x0000_0010
            | 0x0000_0040
            | 0x0000_0080
            | 0x0000_0100
            | 0x0000_0200
            | 0x0000_0400
            | 0x0000_2000
            | 0x0001_0000,
    );
    w(&mut b, 32, 0x0003_ffff & !0x0001_0000); // PrimitiveMiscCaps
    w(
        &mut b,
        36,
        0x0001_0000
            | 0x0000_0080
            | 0x0000_0020
            | 0x0000_4000
            | 0x0000_0008
            | 0x0000_0010
            | 0x0800_0000
            | 0x0400_0000,
    ); // RasterCaps
    w(&mut b, 40, 0xff); // ZCmpCaps
    w(&mut b, 44, 0x3fff); // SrcBlendCaps
    w(&mut b, 48, 0x3fff); // DestBlendCaps
    w(&mut b, 52, 0xff); // AlphaCmpCaps
    w(
        &mut b,
        56,
        0x0008_0000 | 0x0000_0200 | 0x0000_2000 | 0x0000_4000,
    ); // ShadeCaps
       // TextureCaps: no POW2 and no SQUAREONLY (a Shader Model 3 card takes any size; with them set
       // the game made its world target 1024 by 1024 and drew the world into a corner of it).
    w(&mut b, 60, TEXTURE_CAPS);
    w(
        &mut b,
        64,
        0x0300_0000
            | 0x0000_0100
            | 0x0000_0200
            | 0x0000_0400
            | 0x0100_0000
            | 0x0200_0000
            | 0x0400_0000
            | 0x0000_0001
            | 0x0000_0002,
    );
    w(
        &mut b,
        68,
        0x0300_0000
            | 0x0000_0100
            | 0x0000_0200
            | 0x0000_0400
            | 0x0100_0000
            | 0x0200_0000
            | 0x0400_0000
            | 0x0000_0001,
    );
    w(
        &mut b,
        72,
        0x0300_0000 | 0x0000_0100 | 0x0000_0200 | 0x0100_0000 | 0x0200_0000,
    );
    w(&mut b, 76, 0x3f); // TextureAddressCaps
    w(&mut b, 80, 0x3f);
    w(&mut b, 84, 0x7f); // LineCaps
    for at in [88, 92] {
        w(&mut b, at, 16384); // MaxTextureWidth/Height
    }
    w(&mut b, 96, 2048); // MaxVolumeExtent
    w(&mut b, 100, 32768); // MaxTextureRepeat
    w(&mut b, 104, 16384); // MaxTextureAspectRatio
    w(&mut b, 108, 16); // MaxAnisotropy
    put_f(&mut b, 112, 1e10); // MaxVertexW
    for (i, v) in [-32768.0f32, -32768.0, 32768.0, 32768.0]
        .into_iter()
        .enumerate()
    {
        put_f(&mut b, 116 + 4 * i, v); // GuardBand
    }
    w(&mut b, 136, 0x1ff); // StencilCaps
    w(&mut b, 140, 0x0008_0000 | 0x8 | 0x0010_0000 | 0xffff); // FVFCaps: TEXCOORDCOUNTMASK etc
    w(&mut b, 144, 0x03ff_ffff); // TextureOpCaps
    w(&mut b, 148, 8); // MaxTextureBlendStages
    w(&mut b, 152, 8); // MaxSimultaneousTextures
    w(&mut b, 156, 0x0000_ffff & !0x0000_0200); // VertexProcessingCaps
    w(&mut b, 160, 8); // MaxActiveLights
    w(&mut b, 164, 8); // MaxUserClipPlanes
    w(&mut b, 168, 4); // MaxVertexBlendMatrices
    w(&mut b, 172, 0);
    put_f(&mut b, 176, 8192.0); // MaxPointSize
    w(&mut b, 180, 0x00ff_ffff); // MaxPrimitiveCount
    w(&mut b, 184, 0x00ff_ffff); // MaxVertexIndex
    w(&mut b, 188, 16); // MaxStreams
    w(&mut b, 192, 508); // MaxStreamStride
    w(&mut b, 196, 0xfffe_0300); // VertexShaderVersion 3.0
    w(&mut b, 200, 256); // MaxVertexShaderConst
    w(&mut b, 204, 0xffff_0300); // PixelShaderVersion 3.0
    put_f(&mut b, 208, 1e10); // PixelShader1xMaxValue
    w(&mut b, 212, 0x3); // DevCaps2: STREAMOFFSET, VERTEXELEMENTSCANSHARESTREAMOFFSET
    put_f(&mut b, 216, 0.0);
    w(&mut b, 232, 1); // NumberOfAdaptersInGroup
    w(&mut b, 236, 0x0000_03ff); // DeclTypes
    w(&mut b, 240, 4); // NumSimultaneousRTs
    w(
        &mut b,
        244,
        0x0300_0000 | 0x0000_0100 | 0x0000_0200 | 0x0100_0000 | 0x0200_0000,
    );
    for (i, v) in [0x1fu32, 24, 32, 4].into_iter().enumerate() {
        w(&mut b, 248 + 4 * i, v); // VS20Caps
    }
    for (i, v) in [0x1fu32, 24, 32, 4, 512].into_iter().enumerate() {
        w(&mut b, 264 + 4 * i, v); // PS20Caps
    }
    w(
        &mut b,
        284,
        0x0300_0000 | 0x0000_0100 | 0x0000_0200 | 0x0100_0000 | 0x0200_0000,
    );
    w(&mut b, 288, 0x0000_ffff);
    w(&mut b, 292, 0x0000_ffff);
    w(&mut b, 296, 32768);
    w(&mut b, 300, 32768);
    b
}

fn mode_bytes(m: (u32, u32, u32), format: u32) -> Vec<u8> {
    let mut b = vec![0u8; 16];
    put(&mut b, 0, m.0);
    put(&mut b, 4, m.1);
    put(&mut b, 8, m.2);
    put(&mut b, 12, format);
    b
}

/// D3DFORMAT values the checks ask about (the same lists `scripts/d3d9-capture` asks).
const FORMATS: &[u32] = &[
    20,
    21,
    22,
    23,
    24,
    25,
    26,
    27,
    28,
    29,
    30,
    31,
    32,
    33,
    34,
    35,
    36,
    40,
    41,
    50,
    51,
    52,
    60,
    61,
    62,
    63,
    64,
    67,
    70,
    71,
    73,
    75,
    77,
    79,
    80,
    81,
    82,
    83,
    84,
    85,
    100,
    101,
    102,
    110,
    111,
    112,
    113,
    114,
    115,
    116,
    117,
    118,
    119,
    199,
    0x3154_5844,
    0x3254_5844,
    0x3354_5844,
    0x3454_5844,
    0x3554_5844,
    0x5956_5955,
    0x3259_5559,
    0x4742_5247,
    0x4247_5247,
    0x3154_454d,
    0x5a54_4e49,
    0x5a57_4152,
    0x3631_4644,
    0x3432_4644,
    0x4c4c_554e,
    0x3149_5441,
    0x3249_5441,
    0x5453_4e49,
    0x5a53_4552,
    0x434f_5441,
    0x4141_5353,
    0x4244_564e,
    0x4256_3252,
    0x3631_4c41,
];
const DISPLAY_FORMATS: &[u32] = &[21, 22, 23, 24, 25, 35];
const DEPTH_FORMATS: &[u32] = &[
    70,
    71,
    73,
    75,
    77,
    79,
    80,
    82,
    83,
    84,
    0x5a54_4e49,
    0x5a57_4152,
    0x3631_4644,
    0x3432_4644,
];
const USAGES: &[u32] = &[
    0, 1, 2, 0x200, 0x400, 0x2000, 0x8000, 0x10000, 0x20000, 0x40000, 0x80000, 0x100000, 0x200000,
];

/// Formats the reference device makes a texture of: the colour formats, the DXT and the float ones.
fn texture_format(f: u32) -> bool {
    matches!(
        f,
        21 | 22
            | 23
            | 24
            | 25
            | 26
            | 28
            | 31
            | 32
            | 35
            | 36
            | 50
            | 51
            | 60
            | 61
            | 62
            | 63
            | 64
            | 67
            | 70
            | 71
            | 73
            | 75
            | 77
            | 79
            | 80
            | 81
            | 82
            | 83
            | 84
            | 85
            | 100
            | 101
            | 102
            | 110
            | 111
            | 112
            | 113
            | 114
            | 115
            | 116
            | 117
            | 118
            | 119
    ) || matches!(
        f,
        0x3154_5844
            | 0x3254_5844
            | 0x3354_5844
            | 0x3454_5844
            | 0x3554_5844
            | 0x3631_4644
            | 0x3432_4644
            | 0x4c4c_554e
            | 0x3149_5441
            | 0x3249_5441
    )
}

fn render_target_format(f: u32) -> bool {
    matches!(
        f,
        21 | 22
            | 23
            | 24
            | 25
            | 31
            | 35
            | 36
            | 113
            | 114
            | 115
            | 116
            | 117
            | 118
            | 119
            | 50
            | 51
            | 0x4c4c_554e
    )
}

fn depth_format(f: u32) -> bool {
    matches!(
        f,
        70 | 71 | 73 | 75 | 77 | 80 | 82 | 83 | 84 | 0x5a54_4e49 | 0x3631_4644
    )
}

fn answer(ok: bool) -> String {
    format!("{:x}", if ok { D3D_OK } else { NOT_AVAILABLE })
}

/// Writes the `Check*` rows folded as `scripts/d3d9-capture`'s `fold` does.
fn fold(o: &mut String, checks: &[(&str, String, String)]) {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut rows: std::collections::HashMap<(String, String), Vec<(String, String)>> =
        std::collections::HashMap::new();
    for (kind, words, result) in checks {
        let (lead, last) = words.rsplit_once(' ').expect("words");
        let key = (kind.to_string(), lead.to_string());
        if !rows.contains_key(&key) {
            order.push(key.clone());
        }
        rows.entry(key)
            .or_default()
            .push((last.to_string(), result.clone()));
    }
    let mut domains: Vec<String> = Vec::new();
    for key in order {
        let row = &rows[&key];
        let mut counts: Vec<(String, usize)> = Vec::new();
        for (_, result) in row {
            match counts.iter_mut().find(|(r, _)| r == result) {
                Some(c) => c.1 += 1,
                None => counts.push((result.clone(), 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let default = &counts[0].0;
        let exceptions: Vec<String> = row
            .iter()
            .filter(|(_, r)| r != default)
            .map(|(w, r)| format!("{w}={r}"))
            .collect();
        let domain = row
            .iter()
            .map(|(w, _)| w.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let index = match domains.iter().position(|d| *d == domain) {
            Some(i) => i,
            None => {
                writeln!(o, "Domain\td{}\t{domain}", domains.len()).unwrap();
                domains.push(domain);
                domains.len() - 1
            }
        };
        writeln!(
            o,
            "{}\t{}\t{default}\t{}\td{index}",
            key.0,
            key.1,
            exceptions.join(",")
        )
        .unwrap();
    }
}

/// An ANSI path as the table's hex.
fn path_hex(path: &str) -> String {
    hex(path.as_bytes())
}

/// `SM_*` values of a default desktop for the metrics the game and the CRT read; the screen size
/// comes from the current mode, the rest are the system's defaults (zero where none applies).
fn metric(index: u32, current: (u32, u32, u32)) -> u32 {
    match index {
        0 | 16 | 61 => current.0,
        1 | 17 | 62 => current.1,
        2 | 3 => 17, // scroll bar arrows
        4 => 23,     // caption
        5 | 6 => 1,  // border
        7 | 8 => 3,  // dialog frame
        11 | 12 | 13 | 14 | 30 | 31 => 32,
        15 => 20,       // menu
        32 | 33 => 4,   // sizing frame
        34 | 35 => 136, // minimum track
        36 | 37 => 4,   // double click
        43 => 3,        // mouse buttons
        49 | 50 => 16,  // small icons
        57 => 3,
        75 => 1,
        76 | 77 => 0,
        78 => current.0,
        79 => current.1,
        80 => 1, // monitors
        81 => 1,
        _ => 0,
    }
}

/// `D3DQUERYTYPE` event and occlusion, the two the game makes (the retail HAL takes both).
const QUERY_EVENT: u32 = 8;
const QUERY_OCCLUSION: u32 = 9;

/// The `CreateQuery(type, NULL)` lines of `d3d9-capture device queries` for a real GPU: event
/// and occlusion are taken; every other type up to 0x14 (the timestamps, the timings, the
/// driver-private ones) is `D3DERR_NOTAVAILABLE`, as a driver without them answers.
fn query_lines(o: &mut String) {
    for kind in 0..=0x14u32 {
        let hr: u32 = if kind == QUERY_EVENT || kind == QUERY_OCCLUSION {
            0
        } else {
            0x8876_086a
        };
        writeln!(o, "CreateQuery\t{kind:x}\t{hr:x}").unwrap();
    }
}

/// The adapter table of the machine `p` describes, as `Adapter::parse` reads it: the graphics card,
/// its Direct3D answers, the display (modes, devices, metrics), the queries and the game's folders.
/// What the rest of the machine answers (the processor, the time zone, the code pages, the Windows
/// directory, window styles and borders, no NVIDIA library) is upstream's machine table
/// (`super::machine_table`), which the host puts before this one.
pub fn adapter_table(p: &Profile) -> String {
    let cur = p.modes.first().copied().unwrap_or((1920, 1080, 60));
    let mut o = String::new();
    for index in (0..=0x5fu32).chain(0x1000..=0x1003).chain(0x2000..=0x2003) {
        writeln!(o, "GetSystemMetrics\t{index:x}\t{:x}", metric(index, cur)).unwrap();
    }
    query_lines(&mut o);
    // Folders: CSIDL personal, appdata, local appdata, common appdata, common documents, under the
    // user data folder (where the native build's saves are; they replace the machine table's).
    // The game's paths are Windows paths: `Z:` is the host's root (`files::host_path`, as Wine's),
    // so the folders are `Z:\home\...`; a path with `/` is no full path to the game's file layer.
    let u = format!("Z:{}", p.userdata.trim_end_matches('/').replace('/', "\\"));
    for (csidl, path) in [
        (0x05u32, format!("{u}\\Documents")),
        (0x1a, format!("{u}\\AppData\\Roaming")),
        (0x1c, format!("{u}\\AppData\\Local")),
        (0x23, format!("{u}\\AppData\\Common")),
        (0x2e, format!("{u}\\Documents\\Common")),
    ] {
        writeln!(o, "SHGetFolderPathA\t{csidl:x} 0\t0\t{}", path_hex(&path)).unwrap();
    }
    // Display devices: one attached primary device named after the card.
    for index in 0..2u32 {
        let mut b = vec![0u8; 0x1a8];
        put(&mut b, 0, 0x1a8);
        if index == 0 {
            field(&mut b, 4, 32, "\\\\.\\DISPLAY1");
            field(&mut b, 36, 128, &p.name);
            put(&mut b, 164, 5);
            field(
                &mut b,
                168,
                128,
                &format!("PCI\\VEN_{:04X}&DEV_{:04X}", p.vendor_id, p.device_id),
            );
        }
        writeln!(
            o,
            "EnumDisplayDevicesA\t{index:x} 0\t{:x}\t{}",
            1 - index,
            hex(&b)
        )
        .unwrap();
    }
    // The adapter.
    writeln!(o, "GetAdapterCount\t\t1").unwrap();
    for flags in [0u32, 2] {
        writeln!(
            o,
            "GetAdapterIdentifier\t0 {flags:x}\t0\t{}",
            hex(&identifier(p))
        )
        .unwrap();
    }
    writeln!(
        o,
        "GetAdapterDisplayMode\t0\t0\t{}",
        hex(&mode_bytes(cur, 22))
    )
    .unwrap();
    for &format in FORMATS {
        let listed = matches!(format, 22 | 23 | 35);
        let n = if listed { p.modes.len() as u32 } else { 0 };
        writeln!(o, "GetAdapterModeCount\t0 {format:x}\t{n:x}").unwrap();
        for (i, m) in p.modes.iter().enumerate().take(n as usize) {
            writeln!(
                o,
                "EnumAdapterModes\t0 {format:x} {i:x}\t0\t{}",
                hex(&mode_bytes(*m, format))
            )
            .unwrap();
        }
    }
    for devtype in [1u32, 2, 3] {
        writeln!(
            o,
            "GetDeviceCaps\t0 {devtype:x}\t{}\t{}",
            if devtype == 1 { "0" } else { "8876086a" },
            hex(&caps(devtype))
        )
        .unwrap();
    }
    let mut checks: Vec<(&str, String, String)> = Vec::new();
    for devtype in [1u32, 2] {
        let hw = devtype == 1;
        for &display in DISPLAY_FORMATS {
            for &back in DISPLAY_FORMATS.iter().chain([0u32].iter()) {
                for windowed in [0u32, 1] {
                    let ok = hw
                        && matches!(display, 22 | 23 | 35)
                        && (back == 0 || matches!(back, 21 | 22 | 23 | 35))
                        && (windowed == 1 || display == back || back == 21 && display == 22);
                    checks.push((
                        "CheckDeviceType",
                        format!("0 {devtype:x} {display:x} {back:x} {windowed:x}"),
                        answer(ok),
                    ));
                }
            }
        }
    }
    for devtype in [1u32, 2] {
        for &display in DISPLAY_FORMATS {
            for &usage in USAGES {
                for rtype in 1u32..=7 {
                    for &format in FORMATS {
                        let hw = devtype == 1 && matches!(display, 22 | 23 | 35);
                        let ok = hw
                            && match usage & 3 {
                                1 => matches!(rtype, 1 | 3 | 5) && render_target_format(format),
                                2 => matches!(rtype, 1 | 3) && depth_format(format),
                                _ => matches!(rtype, 1 | 3 | 4 | 5) && texture_format(format),
                            };
                        checks.push((
                            "CheckDeviceFormat",
                            format!("0 {devtype:x} {display:x} {usage:x} {rtype:x} {format:x}"),
                            answer(ok),
                        ));
                    }
                }
            }
        }
    }
    for devtype in [1u32, 2] {
        for &format in FORMATS {
            for windowed in [0u32, 1] {
                for samples in 0u32..=16 {
                    let ok = devtype == 1
                        && render_target_format(format)
                        && matches!(samples, 0 | 2 | 4 | 8);
                    let r = if ok {
                        "0/0".to_string()
                    } else {
                        format!("{NOT_AVAILABLE:x}/0")
                    };
                    checks.push((
                        "CheckDeviceMultiSampleType",
                        format!("0 {devtype:x} {format:x} {windowed:x} {samples:x}"),
                        r,
                    ));
                }
            }
        }
    }
    for devtype in [1u32, 2] {
        for &display in DISPLAY_FORMATS {
            for &target in FORMATS {
                for &depth in DEPTH_FORMATS {
                    let ok = devtype == 1 && render_target_format(target) && depth_format(depth);
                    checks.push((
                        "CheckDepthStencilMatch",
                        format!("0 {devtype:x} {display:x} {target:x} {depth:x}"),
                        answer(ok),
                    ));
                }
            }
        }
    }
    for devtype in [1u32, 2] {
        for &source in FORMATS {
            for &target in DISPLAY_FORMATS {
                let ok = devtype == 1
                    && matches!(source, 21 | 22 | 23 | 35)
                    && matches!(target, 21 | 22 | 23 | 35);
                checks.push((
                    "CheckDeviceFormatConversion",
                    format!("0 {devtype:x} {source:x} {target:x}"),
                    answer(ok),
                ));
            }
        }
    }
    writeln!(o, "GetAdapterMonitor\t0\t1").unwrap();
    fold(&mut o, &checks);
    o
}

/// The profile of this Linux machine: the reference profile (userdata under the data folder), the
/// card wgpu reports, then the profile file's overrides.
#[cfg(not(windows))]
pub fn machine_profile() -> Result<Profile, String> {
    let userdata = crate::fork_platform::userdata_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "/tmp/nv-rs/userdata".into());
    let mut p = Profile::radv_reference(&userdata);
    // The display the process started on is the current mode, so the game reads its real size
    // and refresh (`GetAdapterDisplayMode`); a profile file's `mode` lines still replace it.
    if let Some(d) = fork_window::display() {
        p.modes = with_current_mode(
            &p.modes,
            (d.monitor.width, d.monitor.height, d.refresh_hz()),
        );
    }
    if let Some(gpu) = d3d9_wgpu::identity::gpu_identity() {
        p = p.with_gpu(&gpu);
    }
    let file = std::env::var_os("NV_LINUX_PROFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| crate::fork_platform::data_dir().map(|d| d.join("linux_profile.txt")));
    match file.and_then(|f| std::fs::read_to_string(f).ok()) {
        Some(text) => p.parse(&text),
        None => Ok(p),
    }
}

/// `modes` with `current` first, and any other mode of its size dropped (the same size at
/// another rate would be a second answer for one size).
pub fn with_current_mode(
    modes: &[(u32, u32, u32)],
    current: (u32, u32, u32),
) -> Vec<(u32, u32, u32)> {
    let mut out = vec![current];
    out.extend(
        modes
            .iter()
            .copied()
            .filter(|m| (m.0, m.1) != (current.0, current.1)),
    );
    out
}

/// What `NV_RESOLUTION` asks of the game's size: `auto` (the default) or `WxH`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Auto,
    Fixed(u32, u32),
}

impl Resolution {
    /// `NV_RESOLUTION`'s value; anything unreadable is `Auto`.
    pub fn parse(value: Option<&str>) -> Resolution {
        let fixed = value.and_then(|v| {
            let (w, h) = v.trim().split_once(['x', 'X'])?;
            let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
            (w > 0 && h > 0).then_some((w, h))
        });
        fixed.map_or(Resolution::Auto, |(w, h)| Resolution::Fixed(w, h))
    }
}

/// The game's stock size, which `auto` treats as never chosen by the player.
const STOCK_SIZE: (u32, u32) = (640, 480);

/// `prefs` with the display keys settled: `iSize W`, `iSize H` and `bFull Screen` (borderless full
/// screen over the display) are added under `[Display]` where missing. A value already there is
/// the player's and is kept, with two exceptions: `auto` replaces exactly the stock 640x480 with
/// the display's size (a file made by an earlier run, or by the launcher, that never had a
/// choice), and `NV_RESOLUTION=WxH` replaces the size with what was asked. `None` when nothing
/// changed. The second answer says what was decided, for the log.
pub fn prefs_with_display(
    prefs: &str,
    display: (u32, u32),
    resolution: Resolution,
) -> Option<(String, &'static str)> {
    let value_of = |key: &str| {
        prefs.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            k.trim()
                .eq_ignore_ascii_case(key)
                .then(|| v.trim().to_string())
        })
    };
    let current = |key| value_of(key).and_then(|v| v.parse::<u32>().ok());
    let existing = (current("iSize W"), current("iSize H"));
    let (target, replace, why) = match resolution {
        Resolution::Fixed(w, h) => ((w, h), existing != (Some(w), Some(h)), "NV_RESOLUTION"),
        Resolution::Auto => (
            display,
            existing == (Some(STOCK_SIZE.0), Some(STOCK_SIZE.1)),
            "stock 640x480 replaced",
        ),
    };
    let mut out = prefs.to_string();
    if replace {
        out = out
            .split_inclusive('\n')
            .map(|line| {
                let key = line
                    .split_once('=')
                    .map(|(k, _)| k.trim().to_ascii_lowercase());
                let end = &line[line.trim_end_matches(['\r', '\n']).len()..];
                match key.as_deref() {
                    Some("isize w") => format!("iSize W={}{end}", target.0),
                    Some("isize h") => format!("iSize H={}{end}", target.1),
                    _ => line.to_string(),
                }
            })
            .collect();
    }
    let has = |text: &str, key: &str| {
        text.lines().any(|l| {
            l.split_once('=')
                .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        })
    };
    let mut add = String::new();
    for (key, value) in [
        ("iSize W", target.0.to_string()),
        ("iSize H", target.1.to_string()),
        ("bFull Screen", "1".to_string()),
    ] {
        if !has(&out, key) {
            add.push_str(&format!("{key}={value}\r\n"));
        }
    }
    if add.is_empty() {
        return (out != prefs).then_some((out, why));
    }
    let mut with = String::new();
    let mut done = false;
    for line in out.split_inclusive('\n') {
        with.push_str(line);
        if !done && line.trim().eq_ignore_ascii_case("[Display]") {
            if !line.ends_with('\n') {
                with.push_str("\r\n");
            }
            with.push_str(&add);
            done = true;
        }
    }
    if !done {
        if !with.is_empty() && !with.ends_with('\n') {
            with.push_str("\r\n");
        }
        with.push_str("[Display]\r\n");
        with.push_str(&add);
    }
    Some((with, if replace { why } else { "added" }))
}

/// The settings folder the game reads on Windows, `Documents\My Games\FalloutNV`, under the
/// profile's user data folder.
fn settings_folder(userdata: &str) -> std::path::PathBuf {
    std::path::Path::new(userdata)
        .join("Documents")
        .join("My Games")
        .join("FalloutNV")
}

/// Makes the settings a first run needs, where the launcher makes them on Windows
/// (`<userdata>/Documents/My Games/FalloutNV`): a `Fallout.ini` taken from the installation's own
/// `Fallout_default.ini` (it names the archives, so without it no menu or texture file is found)
/// and a `FalloutPrefs.ini` whose `[Display] sD3DDevice` is the adapter's name (`004da570` hands
/// over to the launcher when it differs or is missing). A file that exists is never touched, and
/// nothing is written to the installation. Answers the files it made.
///
/// With a `display` size, the display keys the preferences lack (`iSize W`, `iSize H`,
/// `bFull Screen`) are added, in a new file and in an existing one that has none of them; values
/// already there are the player's and stay.
pub fn seed_settings(
    userdata: &str,
    install: &std::path::Path,
    adapter_name: &str,
    display: Option<(u32, u32)>,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    let folder = settings_folder(userdata);
    let mut made = Vec::new();
    let ini = folder.join("Fallout.ini");
    let prefs = folder.join("FalloutPrefs.ini");
    let resolution = Resolution::parse(std::env::var("NV_RESOLUTION").ok().as_deref());
    if let (Some(size), true) = (display, prefs.exists()) {
        let text = std::fs::read_to_string(&prefs).unwrap_or_default();
        if let Some((text, why)) = prefs_with_display(&text, size, resolution) {
            std::fs::write(&prefs, text)?;
            eprintln!("settings: the game's size set from the display ({why})");
        }
    }
    if ini.exists() && prefs.exists() {
        return Ok(made);
    }
    std::fs::create_dir_all(&folder)?;
    if !ini.exists() {
        let defaults = std::fs::read(install.join("Fallout_default.ini"))?;
        std::fs::write(&ini, defaults)?;
        made.push(ini);
    }
    if !prefs.exists() {
        let base = format!("[Display]\r\nsD3DDevice={adapter_name}\r\n");
        let text = display
            .and_then(|size| prefs_with_display(&base, size, resolution))
            .map_or(base, |(text, _)| text);
        std::fs::write(&prefs, text)?;
        made.push(prefs);
    }
    Ok(made)
}

/// [`seed_settings`] for this machine's profile ([`machine_profile`]); a failure is named on
/// stderr and the run goes on (the game then asks for its launcher).
#[cfg(not(windows))]
pub fn seed_machine_settings(install: &std::path::Path) {
    let profile = match machine_profile() {
        Ok(p) => p,
        Err(why) => return eprintln!("the settings are not made: {why}"),
    };
    let display = fork_window::display().map(|d| (d.monitor.width, d.monitor.height));
    match seed_settings(&profile.userdata, install, &profile.name, display) {
        Ok(made) => {
            for f in made {
                eprintln!("settings made: {}", f.display());
            }
        }
        Err(why) => eprintln!("the settings are not made: {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;

    fn reference() -> Profile {
        Profile::radv_reference("/home/p/.local/share/nv-rs/userdata")
    }

    /// The layer's own loader reads the generated table, and the answers the game's start-up asks
    /// first come out as the RADV reference identity.
    #[test]
    fn the_layer_loads_the_generated_adapter_table() {
        let table = adapter_table(&reference());
        let a = crate::d3d9::Adapter::parse(&table);
        let _ = a; // parse stops on any line it cannot read
        let mut e = Engine::new();
        crate::d3d9::load_adapter_table(&mut e, &table);
        assert!(crate::d3d9::has_adapter_table(&mut e));
        assert_eq!(
            crate::d3d9::captured_format_check(&mut e, &[0, 1, 22, 1, 3, 22]),
            Some(0)
        );
    }

    /// The run's table off Windows (`host::adapter_table_text`): upstream's machine table, then this
    /// one. This table leaves the processor, time zone, code pages, Windows directory, window
    /// styles and NVIDIA lines to the machine table, and its folders come last, so the game's
    /// Documents is the fork's user data folder, where the native build's saves are.
    #[test]
    fn the_machine_table_answers_the_system_and_the_adapter_table_the_card_and_folders() {
        let fork = adapter_table(&reference());
        for question in [
            "Cpuid\t",
            "CpuIdFlagTest\t",
            "GetTimeZoneInformation\t",
            "GetTimeZoneInformationForYear\t",
            "GetACP\t",
            "IsValidCodePage\t",
            "GetWindowsDirectoryA\t",
            "AdjustWindowRectEx\t",
            "CreateWindowStyle\t",
            "NvApiLoad\t",
        ] {
            assert!(!fork.lines().any(|l| l.starts_with(question)), "{question}");
        }
        let inputs = crate::machine_table::Inputs {
            zone: crate::machine_table::parse_posix_tz("UTC0")
                .map(crate::machine_table::Zone::from_rule),
            now: 1_800_000_000,
            processor: crate::machine_table::fixed_processor(),
            own_folder: Ok(std::path::PathBuf::from("/upstream/nv-rs")),
        };
        let (machine, problems) = crate::machine_table::write_table(&inputs);
        assert!(problems.is_empty(), "{problems:?}");
        let table = format!("{machine}{fork}");
        let documents = table
            .lines()
            .rev()
            .find(|l| l.starts_with("SHGetFolderPathA\t5 0\t"))
            .and_then(|l| l.rsplit('\t').next())
            .expect("a Documents line");
        let userdata = format!("Z:{}\\Documents", reference().userdata.replace('/', "\\"));
        assert_eq!(documents, hex(userdata.as_bytes()));
        let mut e = Engine::new();
        crate::d3d9::load_adapter_table(&mut e, &table);
        assert!(crate::d3d9::has_adapter_table(&mut e));
    }

    /// The table says event and occlusion queries are taken and the rest are not available, so the
    /// first outdoor frame's `CreateQuery(9)` no longer stops the run.
    #[test]
    fn the_table_takes_event_and_occlusion_queries() {
        let table = adapter_table(&reference());
        let mut e = Engine::new();
        crate::d3d9::load_adapter_table(&mut e, &table);
        assert_eq!(crate::d3d9::query_support(&mut e, 8), Some(0));
        assert_eq!(crate::d3d9::query_support(&mut e, 9), Some(0));
        for kind in [0, 4, 10, 11, 12, 0x14] {
            assert_eq!(
                crate::d3d9::query_support(&mut e, kind),
                Some(0x8876_086a),
                "type {kind}"
            );
        }
    }

    /// The caps the table gives say a card takes any texture size: with POW2 or SQUAREONLY the
    /// game made its world target a 1024 by 1024 square and drew the world into a corner of it.
    #[test]
    fn the_device_caps_take_textures_of_any_size() {
        let table = adapter_table(&reference());
        let line = table
            .lines()
            .find(|l| l.starts_with("GetDeviceCaps\t0 1\t"))
            .expect("the HAL device's caps");
        let hex = line.split('\t').nth(3).expect("the caps' bytes").trim();
        let word =
            |at: usize| u32::from_str_radix(&hex[2 * at..2 * at + 8], 16).map(u32::swap_bytes);
        let texture_caps = word(60).expect("a hex word");
        assert_eq!(texture_caps, TEXTURE_CAPS);
        assert_eq!(texture_caps & (0x2 | 0x20), 0, "no POW2, no SQUAREONLY");
        assert_ne!(texture_caps & 0x100, 0, "NONPOW2CONDITIONAL");
    }

    #[test]
    fn the_identifier_names_the_driver_and_the_card() {
        let b = identifier(&reference());
        assert!(b.starts_with(b"aticfx32.dll\0"));
        assert_eq!(&b[512..512 + 22], b"AMD Radeon RX 7900 XTX");
        assert_eq!(
            u32::from_le_bytes(b[1032..1036].try_into().unwrap()),
            0x1002
        );
        assert_eq!(
            u32::from_le_bytes(b[1036..1040].try_into().unwrap()),
            0x744c
        );
        assert_eq!(b.len(), 1100);
    }

    #[test]
    fn a_wgpu_card_replaces_the_reference_identity() {
        let gpu = d3d9_wgpu::identity::GpuIdentity {
            name: "NVIDIA GeForce RTX 4070 (NVK AD104)".into(),
            vendor_id: 0x10de,
            device_id: 0x2786,
            driver: "NVK".into(),
            driver_info: String::new(),
            software: false,
        };
        let p = reference().with_gpu(&gpu);
        assert_eq!(
            (p.name.as_str(), p.driver.as_str()),
            ("NVIDIA GeForce RTX 4070", "nvd3dum.dll")
        );
        let soft = d3d9_wgpu::identity::GpuIdentity {
            software: true,
            vendor_id: 0x10005,
            ..gpu
        };
        assert_eq!(reference().with_gpu(&soft), reference());
    }

    #[test]
    fn a_profile_file_overrides_the_reference() {
        let p = reference()
            .parse("# my machine\nname = Radeon X\nvendor_id = 0x1002\nmode = 2560x1440@144\nmode = 1920x1080@60\nbias_minutes = -60\n")
            .unwrap();
        assert_eq!(p.name, "Radeon X");
        assert_eq!(p.modes, vec![(2560, 1440, 144), (1920, 1080, 60)]);
        assert!(reference().parse("bias_minutes = soon").is_err());
        assert!(reference().parse("colour = red").is_err());
        let t = adapter_table(&p);
        assert!(t.contains(&format!(
            "GetAdapterDisplayMode\t0\t0\t{}",
            hex(&mode_bytes((2560, 1440, 144), 22))
        )));
    }

    #[test]
    fn the_reference_sound_table_loads_and_makes_a_device() {
        let t = reference_sound_table();
        let mut e = Engine::new();
        crate::dsound::load_table(&mut e, &t);
        assert!(t.starts_with("DsCreate\t\t0\t\n") && t.ends_with("DsEnd\n"));
    }

    #[test]
    fn the_sound_table_is_a_failed_create_and_loads() {
        let t = no_device_sound_table();
        let mut e = Engine::new();
        crate::dsound::load_table(&mut e, &t);
        assert_eq!(t, "DsCreate\t\t88780078\nDsEnd\n");
    }

    #[test]
    fn the_display_keys_are_added_where_missing_and_kept_where_set() {
        let auto = Resolution::Auto;
        let (new, _) =
            prefs_with_display("[Display]\r\nsD3DDevice=X\r\n", (2560, 1440), auto).unwrap();
        assert_eq!(
            new,
            "[Display]\r\niSize W=2560\r\niSize H=1440\r\nbFull Screen=1\r\nsD3DDevice=X\r\n"
        );
        // The player's own values stay; only the missing key is added.
        let (kept, _) = prefs_with_display(
            "[Display]\r\niSize W=800\r\niSize H=600\r\n",
            (2560, 1440),
            auto,
        )
        .unwrap();
        assert!(kept.contains("iSize W=800") && kept.contains("bFull Screen=1"));
        assert!(!kept.contains("2560"));
        let all = "[Display]\r\niSize W=1\r\niSize H=1\r\nbFull Screen=0\r\n";
        assert_eq!(prefs_with_display(all, (5, 5), auto), None);
        let (bare, _) = prefs_with_display("", (1920, 1080), auto).unwrap();
        assert!(bare.starts_with("[Display]\r\niSize W=1920"));
    }

    #[test]
    fn auto_replaces_exactly_the_stock_size_and_a_fixed_size_replaces_any() {
        let stock = "[Display]\r\niSize W=640\r\niSize H=480\r\nbFull Screen=0\r\n";
        let (n, _) = prefs_with_display(stock, (2560, 1440), Resolution::Auto).unwrap();
        assert_eq!(
            n,
            "[Display]\r\niSize W=2560\r\niSize H=1440\r\nbFull Screen=0\r\n"
        );
        // 640x481 is somebody's choice; one stock axis alone is not the stock size.
        let own = "[Display]\r\niSize W=640\r\niSize H=481\r\nbFull Screen=0\r\n";
        assert_eq!(
            prefs_with_display(own, (2560, 1440), Resolution::Auto),
            None
        );
        let (f, why) =
            prefs_with_display(own, (2560, 1440), Resolution::Fixed(1920, 1080)).unwrap();
        assert!(f.contains("iSize W=1920\r\niSize H=1080"));
        assert_eq!(why, "NV_RESOLUTION");
        assert_eq!(
            prefs_with_display(&f, (2560, 1440), Resolution::Fixed(1920, 1080)),
            None
        );
    }

    #[test]
    fn the_resolution_is_parsed() {
        assert_eq!(Resolution::parse(None), Resolution::Auto);
        assert_eq!(Resolution::parse(Some("auto")), Resolution::Auto);
        assert_eq!(
            Resolution::parse(Some("1920x1080")),
            Resolution::Fixed(1920, 1080)
        );
        assert_eq!(Resolution::parse(Some("0x5")), Resolution::Auto);
    }

    #[test]
    fn the_display_is_the_current_mode() {
        let m = with_current_mode(
            &[(1920, 1080, 60), (2560, 1440, 60), (640, 480, 60)],
            (2560, 1440, 144),
        );
        assert_eq!(m, vec![(2560, 1440, 144), (1920, 1080, 60), (640, 480, 60)]);
    }

    #[test]
    fn first_run_settings_are_made_once_and_never_overwritten() {
        let dir = std::env::temp_dir().join(format!("nv-seed-{}", std::process::id()));
        let install = dir.join("game");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("Fallout_default.ini"), "[Archive]\r\n").unwrap();
        let userdata = dir.join("ud");
        let ud = userdata.to_str().unwrap();
        let made = seed_settings(ud, &install, "Card X", None).unwrap();
        assert_eq!(made.len(), 2);
        let folder = settings_folder(ud);
        assert_eq!(
            std::fs::read_to_string(folder.join("FalloutPrefs.ini")).unwrap(),
            "[Display]\r\nsD3DDevice=Card X\r\n"
        );
        std::fs::write(folder.join("Fallout.ini"), "mine").unwrap();
        assert!(seed_settings(ud, &install, "Card Y", None)
            .unwrap()
            .is_empty());
        assert_eq!(
            std::fs::read_to_string(folder.join("Fallout.ini")).unwrap(),
            "mine"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
