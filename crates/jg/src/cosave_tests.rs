//! The carried section: records survive a load and a save unchanged, and a new game drops them.

use super::*;
use nvse::cosave::Chunk;

fn chunks() -> Vec<Chunk> {
    vec![
        Chunk {
            kind: nvse::cosave::tag(b"ABCD"),
            version: 3,
            data: vec![1, 2, 3, 4, 5],
        },
        Chunk {
            kind: nvse::cosave::tag(b"EFGH"),
            version: 1,
            data: Vec::new(),
        },
    ]
}

/// What is carried is written back record for record; a new game empties it.
#[test]
fn saves_what_it_carried() {
    let mut e = Engine::new();
    e.ext::<State>().carried = chunks();
    let mut out = Records::default();
    save(&mut e, &mut out);
    assert_eq!(out.chunks, chunks());
    new_game(&mut e);
    let mut out = Records::default();
    save(&mut e, &mut out);
    assert!(out.chunks.is_empty());
}

/// `write_chunks` after `read_chunks` is the identity on the records of a co-save section.
#[test]
fn read_then_write_is_the_identity() {
    let plugin = nvse::cosave::Plugin {
        opcode_base: crate::identity::OPCODE_BASE,
        chunks: chunks(),
    };
    let bytes = nvse::cosave::write(&[plugin]);
    let mut r = Reader::open(&bytes).unwrap();
    let header = r.next_plugin().unwrap().unwrap();
    assert_eq!(header.opcode_base, crate::identity::OPCODE_BASE);
    let read = read_chunks(&mut r).unwrap();
    assert_eq!(read, chunks());
    let mut out = Records::default();
    write_chunks(&read, &mut out);
    assert_eq!(out.chunks, chunks());
}
