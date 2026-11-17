//! Writes the Linux machine tables (nv-linux#179): `cargo run -p engine-platform --example
//! linux_tables -- adapter > d3d9_adapter.tsv` or `-- sound > dsound.tsv`. Both stay local.
fn main() {
    #[cfg(not(windows))]
    {
        use engine_platform::fork_tables as t;
        match std::env::args().nth(1).as_deref() {
            // The whole table a run makes: upstream's machine table, then the fork's adapter table.
            Some("adapter") => match t::machine_profile() {
                Ok(p) => {
                    let own = engine_platform::fork_platform::data_dir()
                        .ok_or_else(|| "no data folder".to_string());
                    let (machine, problems) = engine_platform::machine_table::this_machine(own);
                    for line in problems {
                        eprintln!("{line}");
                    }
                    print!("{machine}{}", t::adapter_table(&p))
                }
                Err(why) => {
                    eprintln!("the profile file is not usable: {why}");
                    std::process::exit(1);
                }
            },
            Some("sound") => print!("{}", t::no_device_sound_table()),
            _ => {
                eprintln!("usage: linux_tables adapter|sound");
                std::process::exit(2);
            }
        }
    }
}
