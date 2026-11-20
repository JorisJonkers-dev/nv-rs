// SPDX-License-Identifier: GPL-3.0-only
//! Prints what the sound output sees on this machine: the default sink's channel map, the
//! `NV_SOUND_LAYOUT` setting and the layout the game will be given (docs/fork/LINUX.md, "Sound
//! output"). `cargo run -p ds-mix --example nv_sound_report`; a program that links ds-mix prints
//! the same with `ds_mix::fork_sound::report()`.

#[cfg(not(windows))]
fn main() {
    println!("{}", ds_mix::fork_sound::report());
}

#[cfg(windows)]
fn main() {
    println!(
        "The sound layout on Windows is the playback device's own (Sound settings, Configure)."
    );
}
