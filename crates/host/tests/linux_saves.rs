//! Saving on Linux, end to end and without game data: the calls the game makes to save (create a
//! temporary file, write it, rename it over the save, move the old one to `.bak`, delete a save),
//! made through the engine's file layer on `Z:\` paths with long names (no 8.3 short names: the
//! host keeps none, as a Windows volume with their generation off); then
//! the host's copy-out landing the files in the port's own Saves folder (written under a
//! temporary name, synced, renamed); then a new run listing the folder back with
//! `FindFirstFileA` and `FindNextFileA`, reading a save and saving over it. A failure case: a
//! folder that cannot be written, a target in the way and a folder that goes away must be
//! logged and made again at the next look, never stop the run.
//!
//! Part of the `native-saves` component (.fork/components.toml): CI runs it on Linux.
#![cfg(unix)]

use engine::Engine;
use engine_platform::files::{
    copy_file_a, create_directory_a, create_file_a, delete_file_a, find_close, find_first_file_a,
    find_next_file_a, get_file_attributes_a, move_file_a, read_file, write_file, CREATE_ALWAYS,
    GENERIC_READ, GENERIC_WRITE, OPEN_EXISTING,
};
use engine_platform::Ptr;
use host::save_out::{jobs, run, Job, Watch};
use std::path::{Path, PathBuf};

const INVALID_HANDLE_VALUE: u32 = 0xffff_ffff;
const LONG: &str = "Save 1   Courier  Doc Mitchell s House  00 03 39";
const OTHER: &str = "Save 2   Courier  Goodsprings  00 05 10";

/// A scratch folder of this test's own.
fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nv-linux-saves-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A game path (`Z:\...`) of a host path.
fn z(path: &Path) -> String {
    format!("Z:{}", path.to_string_lossy().replace('/', "\\"))
}

/// The game's file calls on one engine (one run of the game).
struct Game {
    e: Engine,
    clock: u64,
}

/// One entry of a search: the long name, the short name, the size.
type Listed = (String, String, u64);

impl Game {
    fn new() -> Game {
        Game {
            e: Engine::new(),
            clock: 0x01dd_6b85_0000_0000,
        }
    }

    /// Time passes a second between the game's calls, as it does between its frames.
    fn tick(&mut self) {
        self.clock += 10_000_000;
        engine_platform::state_mut(&mut self.e.kernel.platform)
            .disk
            .clock = Some(self.clock);
    }

    fn text(&mut self, s: &str) -> Ptr {
        let at = self.e.mem.alloc(0x400);
        self.e.mem.set_cstr(at, s.as_bytes());
        Ptr::new(at)
    }

    fn open(&mut self, path: &str, access: u32, disposition: u32) -> u32 {
        self.tick();
        let name = self.text(path);
        create_file_a(&mut self.e, name, access, 0, 0, disposition, 0x80, 0)
    }

    fn write(&mut self, h: u32, bytes: &[u8]) {
        self.tick();
        let at = self.e.mem.alloc(bytes.len() as u32 + 4);
        self.e.mem.write(at, bytes);
        let done = self.e.mem.alloc(4);
        let ok = write_file(
            &mut self.e,
            h,
            Ptr::new(at),
            bytes.len() as u32,
            Ptr::new(done),
            0,
        );
        assert_eq!(ok, 1);
        assert_eq!(self.e.mem.u32(done), bytes.len() as u32);
    }

    fn close(&mut self, h: u32) {
        self.tick();
        assert_eq!(engine_platform::close_handle(&mut self.e, h), 1);
    }

    /// `CreateDirectoryA` of each folder of `path` below `existing` (the game makes its own
    /// folders in Documents when they are missing).
    fn make_folders(&mut self, existing: &str, below: &[&str]) {
        let mut path = existing.to_string();
        for part in below {
            path = format!("{path}\\{part}");
            self.tick();
            let name = self.text(&path);
            assert_eq!(create_directory_a(&mut self.e, name, 0), 1, "{path}");
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> u32 {
        self.tick();
        let (a, b) = (self.text(from), self.text(to));
        move_file_a(&mut self.e, a, b)
    }

    fn delete(&mut self, path: &str) -> u32 {
        self.tick();
        let name = self.text(path);
        delete_file_a(&mut self.e, name)
    }

    fn exists(&mut self, path: &str) -> bool {
        let name = self.text(path);
        get_file_attributes_a(&mut self.e, name) != 0xffff_ffff
    }

    /// The whole file `path` as the game reads it.
    fn read(&mut self, path: &str) -> Vec<u8> {
        let h = self.open(path, GENERIC_READ, OPEN_EXISTING);
        assert_ne!(h, INVALID_HANDLE_VALUE, "{path} opens");
        let buffer = self.e.mem.alloc(0x2000);
        let got = self.e.mem.alloc(4);
        assert_eq!(
            read_file(&mut self.e, h, Ptr::new(buffer), 0x2000, Ptr::new(got), 0),
            1
        );
        let bytes = self.e.mem.bytes(buffer, self.e.mem.u32(got));
        self.close(h);
        bytes
    }

    /// The calls the game makes to save `name` into `saves` with extension `ext`: the temporary
    /// file made and closed, opened again, written, closed; an older save of the name moved to
    /// `.bak` over the old backup; the temporary file moved to the name.
    fn save(&mut self, saves: &str, name: &str, ext: &str, bytes: &[u8]) {
        let base = format!("{saves}\\{name}.{ext}");
        let (tmp, bak) = (format!("{base}.tmp"), format!("{base}.bak"));
        assert!(!self.exists(&tmp));
        let h = self.open(&tmp, GENERIC_WRITE, CREATE_ALWAYS);
        assert_ne!(h, INVALID_HANDLE_VALUE, "{tmp} is made");
        self.close(h);
        let h = self.open(&tmp, GENERIC_READ | GENERIC_WRITE, OPEN_EXISTING);
        assert_ne!(h, INVALID_HANDLE_VALUE);
        self.write(h, bytes);
        self.close(h);
        if self.exists(&base) {
            if self.exists(&bak) {
                assert_eq!(self.delete(&bak), 1);
            }
            assert_eq!(self.rename(&base, &bak), 1, "the old save becomes .bak");
        }
        let moved = self.rename(&tmp, &base);
        assert_eq!(
            moved, 1,
            "the new save takes the name ({base}; error {})",
            self.e.kernel.last_error
        );
    }

    /// Every entry `FindFirstFileA` and `FindNextFileA` answer for `pattern`.
    fn list(&mut self, pattern: &str) -> Vec<Listed> {
        let name = self.text(pattern);
        let data = self.e.mem.alloc(0x140);
        let h = find_first_file_a(&mut self.e, name, Ptr::new(data));
        if h == INVALID_HANDLE_VALUE {
            return vec![];
        }
        let read = |e: &Engine| -> Listed {
            (
                String::from_utf8(e.mem.cstr(data + 0x2c)).unwrap(),
                String::from_utf8(e.mem.cstr(data + 0x130)).unwrap(),
                (u64::from(e.mem.u32(data + 0x1c)) << 32) | u64::from(e.mem.u32(data + 0x20)),
            )
        };
        let mut all = vec![read(&self.e)];
        while find_next_file_a(&mut self.e, h, Ptr::new(data)) == 1 {
            all.push(read(&self.e));
        }
        assert_eq!(find_close(&mut self.e, h), 1);
        all
    }

    /// What the copy-out would write now: its jobs, done (the log lines they give).
    fn copy_out(&self, watch: &mut Watch, dir: &Path) -> Vec<String> {
        jobs(&self.e, watch, Some(dir), None)
            .iter()
            .filter_map(|job| run(job).unwrap_or_else(|why| panic!("{why}")))
            .collect()
    }
}

/// The entries of a folder on disk, sorted.
fn on_disk(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_save_made_over_and_deleted_is_listed_by_its_long_name() {
    let root = scratch("engine");
    let saves = root.join(".local/share/nv-rs/userdata/Documents/My Games/FalloutNV/Saves");
    std::fs::create_dir_all(&saves).unwrap();
    let saves = z(&saves);
    let mut g = Game::new();

    g.save(&saves, LONG, "fos", &[1u8; 300]);
    g.save(&saves, LONG, "nvse", &[2u8; 40]);
    g.save(&saves, OTHER, "fos", &[3u8; 120]);
    // The same name saved again: the first becomes `.bak`, over the one before it.
    g.save(&saves, LONG, "fos", &[4u8; 310]);
    g.save(&saves, LONG, "fos", &[5u8; 320]);

    let exact = g.list(&format!("{saves}\\{LONG}.fos"));
    assert_eq!(exact.len(), 1, "a name made in this run is found by itself");
    assert_eq!(exact[0].0, format!("{LONG}.fos"));
    assert_eq!(exact[0].1, "", "no short name");
    assert_eq!(exact[0].2, 320);
    let all = g.list(&format!("{saves}\\*.fos"));
    let longs: Vec<&str> = all.iter().map(|l| l.0.as_str()).collect();
    assert_eq!(longs, [format!("{LONG}.fos"), format!("{OTHER}.fos")]);
    let shorts: Vec<&str> = all.iter().map(|l| l.1.as_str()).collect();
    assert_eq!(shorts, ["", ""]);
    // A search by a short name Windows would give finds nothing: there is none.
    assert!(g.list(&format!("{saves}\\SAVE2C~?.FOS")).is_empty());
    assert_eq!(g.list(&format!("{saves}\\*.nvse")).len(), 1);
    assert_eq!(g.read(&format!("{saves}\\{LONG}.fos")), vec![5u8; 320]);
    assert_eq!(g.read(&format!("{saves}\\{LONG}.fos.bak")), vec![4u8; 310]);

    // A copy of a save (a backup the game makes with CopyFileA) is a file of the folder too.
    let (from, to) = (
        g.text(&format!("{saves}\\{OTHER}.fos")),
        g.text(&format!("{saves}\\{OTHER}.fos.copy")),
    );
    assert_eq!(
        copy_file_a(&mut g.e, from, to, 1),
        1,
        "error {}",
        g.e.kernel.last_error
    );
    assert_eq!(
        g.read(&format!("{saves}\\{OTHER}.fos.copy")),
        vec![3u8; 120]
    );

    // A save deleted is gone from the listing.
    assert_eq!(g.delete(&format!("{saves}\\{OTHER}.fos")), 1);
    let left = g.list(&format!("{saves}\\*.fos"));
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].0, format!("{LONG}.fos"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_copy_out_lands_the_saves_and_a_new_run_lists_loads_and_saves_over_them() {
    let root = scratch("roundtrip");
    let saves_dir = root.join("userdata/Documents/My Games/FalloutNV/Saves");
    let saves = z(&saves_dir);
    let mut first = Game::new();
    std::fs::create_dir_all(root.join("userdata/Documents")).unwrap();
    first.make_folders(
        &z(&root.join("userdata/Documents")),
        &["My Games", "FalloutNV", "Saves"],
    );
    let mut watch = Watch::default();

    // A folder the game finds missing is made by the copy-out, not by the game.
    first.save(&saves, LONG, "fos", &[7u8; 500]);
    first.save(&saves, LONG, "nvse", &[8u8; 60]);
    first.save(&saves, OTHER, "fos", &[9u8; 90]);
    let lines = first.copy_out(&mut watch, &saves_dir);
    assert_eq!(lines.len(), 3, "{lines:?}");
    for line in &lines {
        assert!(line.contains(&saves_dir.display().to_string()), "{line}");
    }
    assert_eq!(
        on_disk(&saves_dir),
        [
            format!("{LONG}.fos"),
            format!("{LONG}.nvse"),
            format!("{OTHER}.fos")
        ]
    );
    assert_eq!(
        std::fs::read(saves_dir.join(format!("{LONG}.fos"))).unwrap(),
        vec![7u8; 500]
    );
    // Nothing changed: nothing is done again.
    assert!(first.copy_out(&mut watch, &saves_dir).is_empty());

    // A save made again replaces the file on disk whole; a deleted one goes.
    first.save(&saves, LONG, "fos", &[6u8; 40]);
    assert_eq!(first.delete(&format!("{saves}\\{OTHER}.fos")), 1);
    let lines = first.copy_out(&mut watch, &saves_dir);
    assert!(lines.iter().any(|l| l.contains("(deleted)")), "{lines:?}");
    assert_eq!(
        std::fs::read(saves_dir.join(format!("{LONG}.fos"))).unwrap(),
        vec![6u8; 40]
    );
    assert_eq!(
        on_disk(&saves_dir),
        [format!("{LONG}.fos"), format!("{LONG}.nvse")]
    );

    // The next run of the game: the folder is listed as the game lists it for its save list,
    // with no short names, and the save is read back.
    let mut next = Game::new();
    let all = next.list(&format!("{saves}\\*.fos"));
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].0, format!("{LONG}.fos"));
    assert_eq!(all[0].1, "");
    assert_eq!(all[0].2, 40);
    let exact = next.list(&format!("{saves}\\{LONG}.fos"));
    assert_eq!(exact[0].1, "");
    assert_eq!(next.read(&format!("{saves}\\{LONG}.fos")), vec![6u8; 40]);

    // Saved over, as the player does with the same name: the old save becomes `.bak`, and the
    // copy-out of this run puts the new one on disk.
    let mut watch = Watch::default();
    next.save(&saves, LONG, "fos", &[1u8; 70]);
    assert_eq!(next.read(&format!("{saves}\\{LONG}.fos")), vec![1u8; 70]);
    assert_eq!(
        next.read(&format!("{saves}\\{LONG}.fos.bak")),
        vec![6u8; 40]
    );
    let again = next.list(&format!("{saves}\\*.fos"));
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].2, 70);
    next.copy_out(&mut watch, &saves_dir);
    assert_eq!(
        std::fs::read(saves_dir.join(format!("{LONG}.fos"))).unwrap(),
        vec![1u8; 70]
    );
    assert!(
        on_disk(&saves_dir).iter().all(|n| !n.ends_with(".part")),
        "no temporary file is left"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A job for a save, as the copy-out makes it.
fn write_job(dir: &Path, name: &str, bytes: &[u8]) -> Job {
    Job::Write {
        key: name.to_ascii_lowercase().into_bytes(),
        dir: Some(dir.to_path_buf()),
        target: dir.join(name),
        temporary: dir.join(format!("{name}.part")),
        bytes: bytes.to_vec(),
        copied: Some(name.to_string()),
    }
}

fn is_root() -> bool {
    // A process that may write into a folder with no permissions is not a test of one.
    let probe = scratch("root-probe");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o500)).unwrap();
    let writable = std::fs::write(probe.join("x"), b"x").is_ok();
    let _ = std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_dir_all(&probe);
    writable
}

#[test]
fn a_copy_out_that_cannot_write_says_so_and_is_made_again_later() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("failures");
    let dir = root.join("Saves");
    std::fs::create_dir_all(&dir).unwrap();

    // The folder cannot be written (a full disk answers the same way: an error from the write).
    if !is_root() {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let why = run(&write_job(&dir, "a.fos", b"x")).expect_err("a read-only folder");
        assert!(why.contains("not copied"), "{why}");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(run(&write_job(&dir, "a.fos", b"x")).unwrap().is_some());
    }

    // A folder in the target's place (the rename cannot replace it): an error, the temporary file
    // kept out of the way for the next try, the save not lost.
    std::fs::create_dir(dir.join("b.fos")).unwrap();
    let why = run(&write_job(&dir, "b.fos", b"y")).expect_err("a folder where the save goes");
    assert!(why.contains("not copied"), "{why}");
    std::fs::remove_dir(dir.join("b.fos")).unwrap();
    assert!(run(&write_job(&dir, "b.fos", b"y")).unwrap().is_some());
    assert_eq!(std::fs::read(dir.join("b.fos")).unwrap(), b"y");

    // The whole folder removed while the game runs: made again, the save lands.
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(run(&write_job(&dir, "c.fos", b"z")).unwrap().is_some());
    assert_eq!(std::fs::read(dir.join("c.fos")).unwrap(), b"z");

    // A folder that is a file: made nowhere, an error, no panic.
    let blocked = root.join("blocked");
    std::fs::write(&blocked, b"x").unwrap();
    let why = run(&write_job(&blocked.join("Saves"), "d.fos", b"w")).expect_err("a file");
    assert!(why.contains("not copied"), "{why}");

    // A removal of a save that is not there is no error.
    let gone = Job::Remove {
        target: dir.join("nothing.fos"),
        name: "nothing.fos".into(),
    };
    assert!(run(&gone).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&root);
}

/// Without `NV_SAVE_OUT` the native build keeps saves in its own Saves folder: the default, with
/// the whole path through `copy_saves` and the thread that does the disk work. Then the folder
/// goes read-only under the running game: the copy fails, is said, the game goes on, and the
/// next look makes the copy.
#[test]
fn the_default_copy_out_keeps_the_saves_in_the_ports_documents() {
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("default");
    // Set before the copy-out first looks at the environment, in this one test of the binary.
    std::env::remove_var("NV_SAVE_OUT");
    std::env::set_var("XDG_DATA_HOME", &root);
    let saves_dir = root.join("nv-rs/userdata/Documents/My Games/FalloutNV/Saves");
    let saves = z(&saves_dir);
    let mut g = Game::new();
    std::fs::create_dir_all(root.join("nv-rs/userdata/Documents")).unwrap();
    g.make_folders(
        &z(&root.join("nv-rs/userdata/Documents")),
        &["My Games", "FalloutNV", "Saves"],
    );

    g.save(&saves, LONG, "fos", &[1u8; 200]);
    g.save(&saves, LONG, "nvse", &[2u8; 30]);
    host::save_out::copy_saves(&g.e, false);
    host::save_out::flush();
    assert_eq!(
        on_disk(&saves_dir),
        [format!("{LONG}.fos"), format!("{LONG}.nvse")]
    );
    assert_eq!(
        std::fs::read(saves_dir.join(format!("{LONG}.fos"))).unwrap(),
        vec![1u8; 200]
    );

    // The next save while the folder cannot be written: nothing stops, the file on disk is the
    // one before.
    if !is_root() {
        std::fs::set_permissions(&saves_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        g.save(&saves, LONG, "fos", &[3u8; 210]);
        host::save_out::copy_saves(&g.e, false);
        host::save_out::flush();
        assert_eq!(
            std::fs::read(saves_dir.join(format!("{LONG}.fos"))).unwrap(),
            vec![1u8; 200]
        );
        // The folder can be written again: the status line's look makes the copy again.
        std::fs::set_permissions(&saves_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        host::save_out::copy_saves(&g.e, true);
        host::save_out::flush();
        assert_eq!(
            std::fs::read(saves_dir.join(format!("{LONG}.fos"))).unwrap(),
            vec![3u8; 210]
        );
    }
    // The game itself still answers its own calls after all of it.
    assert_eq!(
        g.read(&format!("{saves}\\{LONG}.fos")).len(),
        if is_root() { 200 } else { 210 }
    );
    let _ = std::fs::remove_dir_all(&root);
}
