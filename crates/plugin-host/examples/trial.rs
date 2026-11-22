// SPDX-License-Identifier: GPL-3.0-only
//! `trial FILE.dll...`: maps, binds, queries and loads plugin DLLs in a fresh engine and prints
//! how far each got. Reads the files, writes nothing. Each file alone, or with `TRIAL_TOGETHER`
//! all in one engine as xNVSE loads them (every file mapped, then queried, then loaded).
//! `TRIAL_RUNTIME=DIR` supplies runtime DLLs, `TRIAL_ENTRY` runs entry points, `TRIAL_LIST`
//! lists every missing import, `TRIAL_PATCHES` lists the writes into the game's code,
//! `TRIAL_CODE=FalloutNV.exe` supplies the executable's code bytes.

use engine_core::prelude::*;

engine_core::assemble!();

fn fresh() -> Engine {
    let mut e = Engine::new();
    e.map(0x0118_e000, 0x9000);
    e.map(0x0300_0000, 0x1_0000);
    nvse::table::install(&mut e);
    plugin_host::set_enabled(&mut e, true);
    plugin_host::set_entry_points(&mut e, std::env::var_os("TRIAL_ENTRY").is_some());
    if let Ok(dir) = std::env::var("TRIAL_RUNTIME") {
        for f in std::fs::read_dir(dir).expect("runtime folder").flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let bytes = std::fs::read(f.path()).expect("read");
            let r = plugin_host::supply_runtime(&mut e, &name, &bytes);
            println!("runtime {name}: {r:?}");
        }
    }
    if let Ok(exe) = std::env::var("TRIAL_CODE") {
        let r = plugin_host::supply_code_from_exe(&mut e, &std::fs::read(exe).expect("read"));
        println!("code image: {r:?}");
    }
    e
}

fn show(
    e: &mut Engine,
    files: &[(String, Vec<u8>)],
    results: &[Result<usize, plugin_host::LoadError>],
) {
    let plugins = plugin_host::plugins_loaded(e);
    for ((file, bytes), r) in files.iter().zip(results) {
        println!("== {file}: {r:?}");
        let Some(p) = plugins.iter().find(|p| &p.file == file) else {
            continue;
        };
        println!(
            "   name {:?} v{} base {:#x} unresolved {} commands {} failed {:?}",
            p.name,
            p.version,
            p.base,
            p.unresolved.len(),
            p.commands.len(),
            p.failed
        );
        let image = plugin_host::pe::Pe::parse(bytes).unwrap();
        let mut by: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
        for i in &image.imports {
            by.entry(i.dll.to_ascii_lowercase()).or_default().1 += 1;
        }
        for u in &p.unresolved {
            let dll = u.split('!').next().unwrap().to_ascii_lowercase();
            by.entry(dll).or_default().0 += 1;
        }
        for (d, (miss, all)) in by {
            println!("   {d}: {miss} of {all} imports missing");
        }
        if std::env::var_os("TRIAL_LIST").is_some() {
            for u in &p.unresolved {
                println!("   missing {u}");
            }
        }
    }
    if let Ok(path) = std::env::var("TRIAL_JSON") {
        let names: Vec<String> = files.iter().map(|f| f.0.clone()).collect();
        let v = plugin_host::verdicts::verdicts(e, &names);
        std::fs::write(&path, plugin_host::verdicts::to_json(&v)).expect("write");
        for x in &v {
            println!("   verdict {}: {} ({})", x.file, x.verdict, x.reason);
        }
    }
    if std::env::var_os("TRIAL_PATCHES").is_some() {
        for site in plugin_host::patch_sites(e) {
            println!("   {}", site.describe());
        }
    }
}

fn main() {
    let files: Vec<(String, Vec<u8>)> = std::env::args()
        .skip(1)
        .map(|path| {
            let file = std::path::Path::new(&path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (file, std::fs::read(&path).expect("read"))
        })
        .collect();
    if std::env::var_os("TRIAL_TOGETHER").is_some() {
        let mut e = fresh();
        let results = plugin_host::load_many(&mut e, &files);
        show(&mut e, &files, &results);
    } else {
        for one in files.chunks(1) {
            let mut e = fresh();
            let results = plugin_host::load_many(&mut e, one);
            show(&mut e, one, &results);
        }
    }
}
