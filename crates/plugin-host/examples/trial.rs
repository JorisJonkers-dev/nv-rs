// SPDX-License-Identifier: GPL-3.0-only
//! `trial FILE.dll...`: maps, binds, queries and loads each plugin DLL in a fresh engine and
//! prints how far it got. Reads the files, writes nothing.

use engine_core::prelude::*;

engine_core::assemble!();

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read");
        let file = std::path::Path::new(&path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut e = Engine::new();
        e.map(0x0118_e000, 0x9000);
        e.map(0x0300_0000, 0x1_0000);
        nvse::table::install(&mut e);
        plugin_host::set_enabled(&mut e, true);
        let r = plugin_host::load(&mut e, &file, &bytes);
        println!("== {file}: {r:?}");
        for p in plugin_host::plugins_loaded(&mut e) {
            println!(
                "   name {:?} v{} base {:#x} unresolved {} commands {} failed {:?}",
                p.name,
                p.version,
                p.base,
                p.unresolved.len(),
                p.commands.len(),
                p.failed
            );
            let image = plugin_host::pe::Pe::parse(&bytes).unwrap();
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
    }
}
