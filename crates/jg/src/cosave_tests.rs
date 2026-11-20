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

/// A co-save section as the Reader gives it: the plugin's header read, ready for `load`.
fn section(chunks: Vec<Chunk>) -> Vec<u8> {
    nvse::cosave::write(&[nvse::cosave::Plugin {
        opcode_base: crate::identity::OPCODE_BASE,
        chunks,
    }])
}

fn load_from(e: &mut Engine, chunks: Vec<Chunk>) {
    let bytes = section(chunks);
    let mut r = Reader::open(&bytes).unwrap();
    r.next_plugin().unwrap().unwrap();
    load(e, &mut r).unwrap();
}

fn stats(pairs: &[(&str, i32)]) -> std::collections::BTreeMap<Vec<u8>, i32> {
    pairs
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), *v))
        .collect()
}

/// Set, save, load, read back: the statistics come back with the carried records beside them.
#[test]
fn the_statistics_are_saved_and_loaded() {
    let mut e = Engine::new();
    e.ext::<State>().carried = chunks();
    e.ext::<State>().stats = stats(&[("Bobbleheads Found", 3), ("Captives Rescued", -2)]);
    let mut out = Records::default();
    save(&mut e, &mut out);
    // The carried records first, in order, then ours.
    assert_eq!(out.chunks[..2], chunks()[..]);
    assert_eq!(out.chunks.len(), 3);
    assert_eq!(out.chunks[2].kind, STATS_KIND);

    // Through the co-save file format and into another game.
    let mut other = Engine::new();
    other.ext::<State>().stats = stats(&[("Left over", 9)]);
    load_from(&mut other, out.chunks);
    assert_eq!(
        other.ext::<State>().stats,
        stats(&[("Bobbleheads Found", 3), ("Captives Rescued", -2)])
    );
    // Ours is read, not carried: it is written once, not twice, by the next save.
    assert_eq!(other.ext::<State>().carried, chunks());
    let mut again = Records::default();
    save(&mut other, &mut again);
    assert_eq!(again.chunks.len(), 3);
}

/// A save without the record starts at 0, whatever the game before it counted; a new game too.
#[test]
fn a_save_without_the_record_and_a_new_game_start_at_zero() {
    let mut e = Engine::new();
    e.ext::<State>().stats = stats(&[("Captives Rescued", 2)]);
    load_from(&mut e, chunks());
    assert!(e.ext::<State>().stats.is_empty());
    assert_eq!(e.ext::<State>().carried, chunks());

    e.ext::<State>().stats = stats(&[("Bobbleheads Found", 3)]);
    new_game(&mut e);
    assert!(e.ext::<State>().stats.is_empty());
    // No statistics, no record.
    let mut out = Records::default();
    save(&mut e, &mut out);
    assert!(out.chunks.is_empty());
}

/// A record that is cut short, has bytes left over, or is of another version is not read as
/// statistics (and a record of another version is carried untouched).
#[test]
fn a_damaged_record_gives_no_statistics() {
    let good = encode_stats(&stats(&[("A", 1)]));
    assert_eq!(decode_stats(&good), Some(stats(&[("A", 1)])));
    assert_eq!(decode_stats(&good[..good.len() - 1]), None);
    let mut extra = good.clone();
    extra.push(0);
    assert_eq!(decode_stats(&extra), None);
    // A name longer than the bytes left (also near usize::MAX).
    let mut lie = 1u32.to_le_bytes().to_vec();
    lie.extend_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode_stats(&lie), None);
    assert_eq!(decode_stats(&[]), None);
    assert_eq!(decode_stats(&0u32.to_le_bytes()), Some(stats(&[])));

    let mut e = Engine::new();
    load_from(
        &mut e,
        vec![Chunk {
            kind: STATS_KIND,
            version: STATS_VERSION,
            data: good[..good.len() - 2].to_vec(),
        }],
    );
    assert!(e.ext::<State>().stats.is_empty());

    let other_version = Chunk {
        kind: STATS_KIND,
        version: 2,
        data: good,
    };
    load_from(&mut e, vec![other_version.clone()]);
    assert!(e.ext::<State>().stats.is_empty());
    assert_eq!(e.ext::<State>().carried, vec![other_version]);
}
