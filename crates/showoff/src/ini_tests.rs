// SPDX-License-Identifier: GPL-3.0-only
//! The cached INI commands through their records, on a fake file layer (the game's file calls
//! stood in for), and the cache's own file model. The answers pinned are the pages' and the
//! decisions of `provenance/02-cached-ini.md`; the commands have not been run in the real game.

use super::*;
use crate::test_support::{call_expr, engine, SCRIPT};
use fork_catalogue::{covers, ids};
use nvse::vars;
use std::cell::RefCell;
use std::collections::BTreeMap;

const SET: u32 = 0x3d26;
const CREATE: u32 = 0x3d27;
const GET: u32 = 0x3d29;
const GET_STRING: u32 = 0x3d2a;
const CLEAR: u32 = 0x3d2c;
const SAVE: u32 = 0x3d2d;

thread_local! {
    /// The fake files by lowercase path, the open handles (path, read position), the folders made.
    static FILES: RefCell<BTreeMap<String, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
    static OPEN: RefCell<Vec<(String, usize)>> = const { RefCell::new(Vec::new()) };
    static FOLDERS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static READS: RefCell<u32> = const { RefCell::new(0) };
}

fn eax(v: u32) -> Ret {
    Ret {
        eax: v,
        ..Ret::default()
    }
}

fn path_of(e: &Engine, at: u32) -> String {
    String::from_utf8_lossy(&nvse::table::guest_text(e, at)).to_ascii_lowercase()
}

fn setup() -> Engine {
    FILES.with(|f| f.borrow_mut().clear());
    OPEN.with(|f| f.borrow_mut().clear());
    FOLDERS.with(|f| f.borrow_mut().clear());
    READS.with(|r| *r.borrow_mut() = 0);
    let mut e = engine();
    e.map(0x011c_3000, 0x1000);
    e.register(CREATE_FILE_A, |e, a| {
        let path = path_of(e, a[0]);
        let exists = FILES.with(|f| f.borrow().contains_key(&path));
        if a[4] == 3 {
            if !exists {
                return eax(INVALID_HANDLE_VALUE);
            }
            READS.with(|r| *r.borrow_mut() += 1);
        } else {
            FILES.with(|f| f.borrow_mut().insert(path.clone(), Vec::new()));
        }
        OPEN.with(|o| {
            o.borrow_mut().push((path, 0));
            eax(o.borrow().len() as u32)
        })
    });
    e.register(0x00fd_f080, |e, a| {
        let (path, pos) = OPEN.with(|o| o.borrow()[a[0] as usize - 1].clone());
        let data = FILES.with(|f| f.borrow()[&path].clone());
        let n = (data.len() - pos).min(a[2] as usize);
        e.mem.write(a[1], &data[pos..pos + n]);
        e.mem.set_u32(a[3], n as u32);
        OPEN.with(|o| o.borrow_mut()[a[0] as usize - 1].1 += n);
        eax(1)
    });
    e.register(WRITE_FILE, |e, a| {
        let path = OPEN.with(|o| o.borrow()[a[0] as usize - 1].0.clone());
        let bytes = e.mem.bytes(a[1], a[2]);
        FILES.with(|f| f.borrow_mut().get_mut(&path).unwrap().extend(bytes));
        e.mem.set_u32(a[3], a[2]);
        eax(1)
    });
    e.register(CLOSE_HANDLE, |_, _| eax(1));
    e.register(CREATE_DIRECTORY, |e, a| {
        let p = path_of(e, a[0]);
        FOLDERS.with(|f| f.borrow_mut().push(p));
        eax(1)
    });
    // The script's mod: `MyMod.esp`.
    e.mem.set_u32(SCRIPT + 0x10, 0x0300_3000);
    e.mem.write(0x0300_3020, b"MyMod.esp\0");
    e
}

fn put(path: &str, text: &str) {
    FILES.with(|f| {
        f.borrow_mut()
            .insert(path.to_ascii_lowercase(), text.as_bytes().to_vec())
    });
}

fn file(path: &str) -> Option<String> {
    FILES.with(|f| {
        f.borrow()
            .get(&path.to_ascii_lowercase())
            .map(|b| String::from_utf8_lossy(b).into_owned())
    })
}

fn text(t: &str) -> Vec<u8> {
    let mut b = vec![b'S'];
    b.extend_from_slice(&(t.len() as u16).to_le_bytes());
    b.extend_from_slice(t.as_bytes());
    b
}

fn num(v: f64) -> Vec<u8> {
    let mut b = vec![b'Z'];
    b.extend_from_slice(&v.to_le_bytes());
    b
}

fn run(e: &mut Engine, opcode: u32, args: &[Vec<u8>]) -> f64 {
    let (ok, result) = call_expr(e, opcode, args);
    assert_eq!(ok, 1);
    result
}

#[test]
fn the_commands_are_registered_where_the_plugin_lists_them() {
    let e = setup();
    for (c, opcode) in COMMANDS
        .iter()
        .zip([0x3d26, 0x3d27, 0x3d29, 0x3d2a, 0x3d2c, 0x3d2d])
    {
        assert_eq!(c.opcode, opcode);
        let record = nvse::table::record_of(&e, opcode);
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record)),
            c.name.as_bytes()
        );
        assert_eq!(
            nvse::table::guest_text(&e, e.mem.u32(record + 4)),
            c.short.as_bytes()
        );
        assert_eq!(e.mem.u32(record + 0x1c), nvse::table::EXPRESSION_PARSE);
    }
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    assert_eq!(names, crate::COMMAND_NAMES[3..9]);
}

#[test]
fn a_float_is_read_from_its_section_and_key_without_case() {
    covers(&[ids::extenders::showoff::GETINIFLOATORDEFAULT_CACHED]);
    let mut e = setup();
    put(
        "Data\\Config\\Mine.ini",
        "[Game]\r\nfSpeed = 2.5\r\niCount=7\r\n[Other]\r\nfSpeed = 9\r\n",
    );
    let get = |e: &mut Engine, name: &str, path: &str, default: f64| {
        run(e, GET, &[text(name), text(path), num(default)])
    };
    assert_eq!(get(&mut e, "Game:fSpeed", "Mine.ini", -1.0), 2.5);
    assert_eq!(
        get(&mut e, "game\\FSPEED", "mine", -1.0),
        2.5,
        "other delimiter, case, no extension"
    );
    assert_eq!(get(&mut e, "Game/iCount", "Mine", 0.0), 7.0);
    assert_eq!(get(&mut e, "Other:fSpeed", "Mine", 0.0), 9.0);
    assert_eq!(
        get(&mut e, "Game:fMissing", "Mine", 3.0),
        3.0,
        "the default"
    );
    assert_eq!(get(&mut e, "Nope:fSpeed", "Mine", 4.0), 4.0);
    assert_eq!(
        get(&mut e, "NoDelimiter", "Mine", 5.0),
        5.0,
        "a name that is not Section:Key"
    );
    assert_eq!(
        get(&mut e, "Game:fSpeed", "Other", 6.0),
        6.0,
        "a file that is not there"
    );
    assert_eq!(
        run(&mut e, GET, &[text("Game:fSpeed"), text("Mine")]),
        2.5,
        "no default: 0 when missing"
    );
    assert_eq!(
        run(&mut e, GET, &[text("Game:zzz")]),
        0.0,
        "the mod's own file, not there"
    );
    let reads = READS.with(|r| *r.borrow());
    assert_eq!(reads, 1, "Mine.ini was opened once and then cached");
}

#[test]
fn the_file_is_cached_after_its_first_read() {
    covers(&[
        ids::extenders::showoff::GETINIFLOATORDEFAULT_CACHED,
        ids::extenders::showoff::CLEARFILECACHESHOWOFF,
    ]);
    let mut e = setup();
    put("Data\\Config\\Mine.ini", "[A]\r\nk = 1\r\n");
    assert_eq!(run(&mut e, GET, &[text("A:k"), text("Mine")]), 1.0);
    put("Data\\Config\\Mine.ini", "[A]\r\nk = 2\r\n");
    assert_eq!(
        run(&mut e, GET, &[text("A:k"), text("Mine")]),
        1.0,
        "from the cache"
    );
    assert_eq!(run(&mut e, CLEAR, &[text("Mine"), num(0.0)]), 1.0);
    assert_eq!(
        run(&mut e, GET, &[text("A:k"), text("Mine")]),
        2.0,
        "read again"
    );
}

#[test]
fn the_mods_own_file_is_the_default_path() {
    covers(&[ids::extenders::showoff::GETINIFLOATORDEFAULT_CACHED]);
    let mut e = setup();
    put("Data\\Config\\MyMod.ini", "[A]\r\nk = 8\r\n");
    assert_eq!(run(&mut e, GET, &[text("A:k")]), 8.0);
    assert_eq!(run(&mut e, GET, &[text("A:k"), text("")]), 8.0);
}

#[test]
fn a_value_that_is_not_a_number_reads_as_its_leading_number() {
    assert_eq!(number_of(b"1.5abc"), 1.5);
    assert_eq!(number_of(b"  -3 "), -3.0);
    assert_eq!(number_of(b"abc"), 0.0);
    assert_eq!(number_of(b"1e2x"), 100.0);
    assert_eq!(number_of(b""), 0.0);
    assert_eq!(number_of(b"2e"), 2.0);
}

#[test]
fn a_path_that_leaves_the_folder_is_refused() {
    covers(&[
        ids::extenders::showoff::GETINIFLOATORDEFAULT_CACHED,
        ids::extenders::showoff::SETINIVALUE_CACHED,
        ids::extenders::showoff::SAVECACHEDINIFILE,
    ]);
    let mut e = setup();
    put("Data\\Config\\x.ini", "[A]\r\nk = 1\r\n");
    for bad in ["..\\x", "C:\\x", "\\x", "a\\..\\x", "sub\\"] {
        assert_eq!(
            run(&mut e, GET, &[text("A:k"), text(bad), num(9.0)]),
            9.0,
            "{bad}"
        );
        assert_eq!(
            run(&mut e, SET, &[text("A:k"), num(1.0), text(bad)]),
            -1.0,
            "{bad}"
        );
        assert_eq!(run(&mut e, SAVE, &[text(bad)]), 0.0, "{bad}");
    }
}

#[test]
fn a_string_is_read_with_its_default() {
    covers(&[ids::extenders::showoff::GETINISTRINGORDEFAULT_CACHED]);
    let mut e = setup();
    put(
        "Data\\Config\\Mine.ini",
        "[A]\r\nsName =  Some Text ; not a comment\r\n",
    );
    let get = |e: &mut Engine, args: &[Vec<u8>]| {
        let a = call_expr(e, GET_STRING, args).1;
        vars::string(e, a as u32).expect("a string")
    };
    assert_eq!(
        get(&mut e, &[text("A:sName"), text("Mine"), text("d")]),
        b"Some Text ; not a comment"
    );
    assert_eq!(
        get(&mut e, &[text("A:sOther"), text("Mine"), text("d")]),
        b"d"
    );
    assert_eq!(get(&mut e, &[text("A:sOther"), text("Mine")]), b"");
    assert_eq!(
        get(&mut e, &[text("nodelimiter"), text("Mine"), text("d")]),
        b"d"
    );
}

#[test]
fn create_writes_a_missing_key_to_the_cache_only() {
    covers(&[
        ids::extenders::showoff::GETINIFLOATORCREATE_CACHED,
        ids::extenders::showoff::GETINIFLOATORDEFAULT_CACHED,
        ids::extenders::showoff::SAVECACHEDINIFILE,
    ]);
    let mut e = setup();
    put("Data\\Config\\Mine.ini", "[A]\r\nk = 1\r\n");
    let create = |e: &mut Engine, name: &str, v: f64, comment: Option<&str>| {
        let mut args = vec![text(name), text("Mine"), num(v)];
        if let Some(c) = comment {
            args.push(text(c));
        }
        run(e, CREATE, &args)
    };
    assert_eq!(create(&mut e, "A:k", 5.0, None), 1.0, "there: its value");
    assert_eq!(
        create(&mut e, "A:new", 2.5, Some("what it does")),
        2.5,
        "missing: the create value"
    );
    assert_eq!(create(&mut e, "B:other", 3.0, None), 3.0);
    assert_eq!(
        run(&mut e, GET, &[text("A:new"), text("Mine"), num(0.0)]),
        2.5,
        "now in the cache"
    );
    assert_eq!(
        file("Data\\Config\\Mine.ini").unwrap(),
        "[A]\r\nk = 1\r\n",
        "the file is not touched"
    );
    assert_eq!(run(&mut e, SAVE, &[text("Mine")]), 1.0);
    assert_eq!(
        file("Data\\Config\\Mine.ini").unwrap(),
        "[A]\r\nk = 1\r\n; what it does\r\nnew = 2.5\r\n\r\n[B]\r\nother = 3\r\n"
    );
}

#[test]
fn set_answers_updated_inserted_or_an_error() {
    covers(&[
        ids::extenders::showoff::SETINIVALUE_CACHED,
        ids::extenders::showoff::SAVECACHEDINIFILE,
    ]);
    let mut e = setup();
    put("Data\\Config\\Mine.ini", "; header\r\n[A]\r\nK = 1\r\n");
    assert_eq!(
        run(&mut e, SET, &[text("A:k"), num(4.0), text("Mine")]),
        1.0,
        "updated"
    );
    assert_eq!(
        run(
            &mut e,
            SET,
            &[text("A:s"), text("hello"), text("Mine"), text("note")]
        ),
        2.0,
        "inserted"
    );
    assert_eq!(
        run(&mut e, SET, &[text("noway"), num(1.0), text("Mine")]),
        -1.0
    );
    assert_eq!(
        run(&mut e, SET, &[text(":k"), num(1.0), text("Mine")]),
        -1.0
    );
    assert_eq!(
        run(&mut e, SET, &[text("A:"), num(1.0), text("Mine")]),
        -1.0
    );
    assert_eq!(run(&mut e, SAVE, &[text("Mine")]), 1.0);
    assert_eq!(
        file("Data\\Config\\Mine.ini").unwrap(),
        "; header\r\n[A]\r\nK = 4\r\n; note\r\ns = hello\r\n",
        "comments kept, the key keeps its spelling"
    );
}

#[test]
fn the_pages_example_makes_the_file_and_its_folders() {
    covers(&[
        ids::extenders::showoff::SETINIVALUE_CACHED,
        ids::extenders::showoff::SAVECACHEDINIFILE,
    ]);
    let mut e = setup();
    assert_eq!(
        run(
            &mut e,
            SET,
            &[
                text("GamePlay:fMeleeDamageMultiplier"),
                num(2.0),
                text("MyFolder\\MyFile.ini")
            ]
        ),
        2.0
    );
    assert_eq!(
        file("Data\\Config\\MyFolder\\MyFile.ini"),
        None,
        "not before it is saved"
    );
    assert_eq!(run(&mut e, SAVE, &[text("MyFolder\\MyFile.ini")]), 1.0);
    assert_eq!(
        file("Data\\Config\\MyFolder\\MyFile.ini").unwrap(),
        "[GamePlay]\r\nfMeleeDamageMultiplier = 2\r\n"
    );
    let folders = FOLDERS.with(|f| f.borrow().clone());
    assert_eq!(folders, ["data\\config", "data\\config\\myfolder"]);
}

#[test]
fn save_and_clear_answer_whether_the_cache_was_there() {
    covers(&[
        ids::extenders::showoff::SAVECACHEDINIFILE,
        ids::extenders::showoff::CLEARFILECACHESHOWOFF,
        ids::extenders::showoff::SETINIVALUE_CACHED,
    ]);
    let mut e = setup();
    assert_eq!(run(&mut e, SAVE, &[text("Mine")]), 0.0, "nothing cached");
    assert_eq!(run(&mut e, CLEAR, &[text("Mine"), num(0.0)]), 0.0);
    run(&mut e, SET, &[text("A:k"), num(1.0), text("Mine")]);
    assert_eq!(
        run(&mut e, CLEAR, &[text("MINE.INI"), num(1.0)]),
        0.0,
        "type 1 is JSON: no cache"
    );
    assert_eq!(
        run(&mut e, CLEAR, &[text("MINE.INI"), num(7.0)]),
        0.0,
        "an unknown type"
    );
    assert_eq!(
        run(&mut e, CLEAR, &[text("MINE.INI"), num(0.0)]),
        1.0,
        "without case"
    );
    assert_eq!(
        run(&mut e, CLEAR, &[text("MINE.INI"), num(0.0)]),
        0.0,
        "already gone"
    );
    assert_eq!(
        run(&mut e, SAVE, &[text("Mine")]),
        0.0,
        "a cleared cache is not saved"
    );
    assert_eq!(file("Data\\Config\\Mine.ini"), None);
}

#[test]
fn the_file_model_keeps_what_it_does_not_touch() {
    let text = b"; top\r\n\r\n[S]\r\na=1\r\n# note\r\njunk line\r\nb = 2\r\n[T]\r\n";
    let f = IniFile::parse(text);
    assert_eq!(f.get(b"s", b"B"), Some(&b"2"[..]));
    assert_eq!(f.get(b"T", b"a"), None, "a key of another section");
    assert_eq!(
        f.text(),
        b"; top\r\n\r\n[S]\r\na = 1\r\n# note\r\njunk line\r\nb = 2\r\n[T]\r\n"
    );
    // A file with a byte order mark and bare line feeds.
    let f = IniFile::parse(b"\xef\xbb\xbf[S]\nk=v\n");
    assert_eq!(f.get(b"S", b"k"), Some(&b"v"[..]));
}
