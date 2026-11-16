//! The container a recorded stream is kept in for benchmarking (`*.nvrs`).
//!
//! `NV_D3D9_RECORD` files (`d3d9_stream::wire`) are tied to the exact command list of the build
//! that wrote them and are not compressed. A container wraps one of them with what a benchmark
//! needs to know before it reads the stream, and compresses the stream (zlib, level 1: a stream is
//! mostly texture bytes, which compress at several hundred MB/s):
//!
//! ```text
//! "NVRSTRM\0"      8 bytes
//! version          u16 LE (1)
//! flags            u16 LE (0)
//! list_hash        u64 LE, d3d9_stream::wire::LIST_HASH of the stream inside
//! meta_len         u32 LE
//! meta             `meta_len` bytes, UTF-8, `key=value` lines (see [`Meta`])
//! raw_len          u64 LE, the stream's length uncompressed
//! stream           zlib of the wire file (its own 16-byte header included)
//! ```
//!
//! A reader refuses another version and a stream written with another command list, with the
//! numbers in the message. The files hold draw parameters, shader bytecode and texture bytes of
//! the game: private data, kept on the machine that recorded them, never committed.

use std::io::{Read, Write};

/// The first bytes of a container.
pub const MAGIC: &[u8; 8] = b"NVRSTRM\0";
/// This build's container version.
pub const VERSION: u16 = 1;

/// What is known about a stream without reading it: numbers and a name, no game data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    /// A name for the scene (`doc-room`, `street`).
    pub scene: String,
    /// The frames (presents) in the stream after its setup.
    pub frames: u32,
    /// The commands before the first frame: what makes the resources the frames use.
    pub setup_cmds: u64,
    /// Draw calls in the frames.
    pub draws: u64,
    /// The back buffer's size when it was recorded.
    pub width: u32,
    pub height: u32,
    /// Seconds since the Unix epoch when it was packed.
    pub packed_at: u64,
    /// Anything else, `key=value`, in order (the build, the first recorded frame).
    pub extra: Vec<(String, String)>,
}

impl Meta {
    fn encode(&self) -> String {
        let mut s = String::new();
        let mut put = |k: &str, v: &dyn std::fmt::Display| {
            s.push_str(&format!("{k}={v}\n"));
        };
        put("scene", &self.scene);
        put("frames", &self.frames);
        put("setup_cmds", &self.setup_cmds);
        put("draws", &self.draws);
        put("width", &self.width);
        put("height", &self.height);
        put("packed_at", &self.packed_at);
        for (k, v) in &self.extra {
            put(k, v);
        }
        s
    }

    fn decode(text: &str) -> Meta {
        let mut m = Meta::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let n = || v.parse::<u64>().unwrap_or(0);
            match k {
                "scene" => m.scene = v.to_string(),
                "frames" => m.frames = n() as u32,
                "setup_cmds" => m.setup_cmds = n(),
                "draws" => m.draws = n(),
                "width" => m.width = n() as u32,
                "height" => m.height = n() as u32,
                "packed_at" => m.packed_at = n(),
                _ => m.extra.push((k.to_string(), v.to_string())),
            }
        }
        m
    }
}

/// Why a container could not be read.
#[derive(Debug)]
pub enum Error {
    NotAContainer,
    Version { file: u16, this: u16 },
    OtherList { file: u64, this: u64 },
    Truncated,
    Io(std::io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotAContainer => write!(f, "not a replay-bench container (no {MAGIC:?})"),
            Error::Version { file, this } => {
                write!(f, "container version {file}; this build reads {this}")
            }
            Error::OtherList { file, this } => write!(
                f,
                "recorded with another command list ({file:016x}; this build's is {this:016x}): \
                 record again with a viewer of this build"
            ),
            Error::Truncated => write!(f, "the container ends early"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

/// Writes `meta` and the wire file `stream` (a header and records, `d3d9_stream::wire`) as a
/// container.
pub fn write(out: &mut impl Write, meta: &Meta, stream: &[u8]) -> Result<(), Error> {
    let list = if stream.len() >= 16 {
        u64::from_le_bytes(stream[8..16].try_into().expect("8 bytes"))
    } else {
        d3d9_stream::wire::LIST_HASH
    };
    let text = meta.encode();
    out.write_all(MAGIC)?;
    out.write_all(&VERSION.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?;
    out.write_all(&list.to_le_bytes())?;
    out.write_all(&(text.len() as u32).to_le_bytes())?;
    out.write_all(text.as_bytes())?;
    out.write_all(&(stream.len() as u64).to_le_bytes())?;
    let mut z = flate2::write::ZlibEncoder::new(out, flate2::Compression::new(1));
    z.write_all(stream)?;
    z.finish()?;
    Ok(())
}

/// The container's metadata, read without the stream.
pub fn read_meta(input: &mut impl Read) -> Result<Meta, Error> {
    Ok(read_head(input)?.0)
}

fn read_head(input: &mut impl Read) -> Result<(Meta, u64), Error> {
    let mut head = [0u8; 8 + 2 + 2 + 8 + 4];
    let mut magic = [0u8; 8];
    input.read_exact(&mut magic).map_err(eof)?;
    if &magic != MAGIC {
        return Err(Error::NotAContainer);
    }
    head[..8].copy_from_slice(&magic);
    input.read_exact(&mut head[8..]).map_err(eof)?;
    let version = u16::from_le_bytes([head[8], head[9]]);
    if version != VERSION {
        return Err(Error::Version {
            file: version,
            this: VERSION,
        });
    }
    let list = u64::from_le_bytes(head[12..20].try_into().expect("8 bytes"));
    if list != d3d9_stream::wire::LIST_HASH {
        return Err(Error::OtherList {
            file: list,
            this: d3d9_stream::wire::LIST_HASH,
        });
    }
    let meta_len = u32::from_le_bytes(head[20..24].try_into().expect("4 bytes")) as usize;
    let mut text = vec![0u8; meta_len];
    input.read_exact(&mut text).map_err(eof)?;
    let mut raw = [0u8; 8];
    input.read_exact(&mut raw).map_err(eof)?;
    Ok((
        Meta::decode(&String::from_utf8_lossy(&text)),
        u64::from_le_bytes(raw),
    ))
}

fn eof(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::Truncated
    } else {
        Error::Io(e)
    }
}

/// The metadata and the wire file of a container.
pub fn read(input: &mut impl Read) -> Result<(Meta, Vec<u8>), Error> {
    let (meta, raw_len) = read_head(input)?;
    let mut stream = Vec::with_capacity(raw_len as usize);
    flate2::read::ZlibDecoder::new(input)
        .read_to_end(&mut stream)
        .map_err(eof)?;
    if stream.len() as u64 != raw_len {
        return Err(Error::Truncated);
    }
    Ok((meta, stream))
}

/// A file as a wire file: a container (decompressed) or a plain `NV_D3D9_RECORD` file.
pub fn load(path: &std::path::Path) -> Result<(Option<Meta>, Vec<u8>), Error> {
    let mut f = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
    let mut first = [0u8; 8];
    f.read_exact(&mut first).map_err(eof)?;
    let mut all = std::io::Cursor::new(first.to_vec()).chain(f);
    if &first == MAGIC {
        let (m, s) = read(&mut all)?;
        Ok((Some(m), s))
    } else {
        let mut s = Vec::new();
        all.read_to_end(&mut s)?;
        Ok((None, s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream() -> Vec<u8> {
        crate::synthetic::stream(&crate::synthetic::Scene::small())
    }

    #[test]
    fn round_trip() {
        let s = stream();
        let meta = Meta {
            scene: "synthetic".into(),
            frames: 4,
            setup_cmds: 12,
            draws: 40,
            width: 640,
            height: 480,
            packed_at: 1,
            extra: vec![("note".into(), "a=b".into())],
        };
        let mut file = Vec::new();
        write(&mut file, &meta, &s).unwrap();
        assert!(file.len() < s.len(), "compressed");
        let (m, back) = read(&mut file.as_slice()).unwrap();
        assert_eq!(m, meta);
        assert_eq!(back, s);
        assert_eq!(read_meta(&mut file.as_slice()).unwrap(), meta);
    }

    #[test]
    fn refuses_other_versions_lists_and_truncation() {
        let s = stream();
        let mut file = Vec::new();
        write(&mut file, &Meta::default(), &s).unwrap();
        let mut v = file.clone();
        v[8] = 9;
        assert!(matches!(
            read(&mut v.as_slice()),
            Err(Error::Version { file: 9, .. })
        ));
        let mut l = file.clone();
        l[12] ^= 1;
        assert!(matches!(
            read(&mut l.as_slice()),
            Err(Error::OtherList { .. })
        ));
        assert!(matches!(
            read(&mut file[..file.len() - 9].to_vec().as_slice()),
            Err(Error::Truncated)
        ));
        assert!(matches!(
            read(&mut b"not a container at all".as_slice()),
            Err(Error::NotAContainer)
        ));
    }
}
