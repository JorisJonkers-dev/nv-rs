//! Writes the Linux machine tables (nv-linux#179): `cargo run -p engine-platform --example
//! linux_tables -- adapter > d3d9_adapter.tsv` or `-- sound > dsound.tsv`. Both stay local.
fn main() {
    #[cfg(not(windows))]
    {
        use engine_platform::fork_tables as t;
        match std::env::args().nth(1).as_deref() {
            Some("adapter") => match t::machine_profile() {
                Ok(p) => print!("{}", t::adapter_table(&p)),
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
