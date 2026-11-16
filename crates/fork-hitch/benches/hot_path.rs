//! The recorder's per-call cost on the hot paths, in nanoseconds a call (docs/fork/HITCHES.md,
//! "Overhead"). Std only, no harness: `cargo bench -p fork-hitch --bench hot_path`, with
//! `NV_HITCH=off` for the switched-off path. The call rate it is weighed against is the play
//! frame's: about 10,100 outermost main-thread platform calls and 840 draws a frame (newgame).

use std::hint::black_box;
use std::time::Instant;

const N: u32 = 5_000_000;

fn per_call(name: &str, mut f: impl FnMut(u32)) -> f64 {
    for i in 0..N / 10 {
        f(i);
    }
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        for i in 0..N {
            f(i);
        }
        best = best.min(t.elapsed().as_nanos() as f64 / f64::from(N));
    }
    println!("{name:<28} {best:7.2} ns");
    best
}

fn main() {
    fork_hitch::init();
    println!("NV_HITCH={:?}", fork_hitch::mode());
    // A guest call stack as the engine hands it over: callers, the import last.
    let stack: Vec<u32> = (0..12).map(|i| 0x0040_0000 + i * 0x40).collect();
    let empty = per_call("empty loop", |i| {
        black_box(i);
    });
    fork_hitch::set_role(fork_hitch::Role::Main);
    let call = per_call("enter_call + leave (main)", |i| {
        let g = fork_hitch::enter_call(black_box(0x0100_0000 | (i & 63)), black_box(&stack));
        drop(black_box(g));
    });
    per_call("span (texture create)", |_| {
        let s = fork_hitch::span(
            fork_hitch::Counter::TextureNs,
            Some(fork_hitch::Counter::TextureN),
        );
        drop(black_box(s));
    });
    per_call("span_main (sound tick)", |_| {
        let s = fork_hitch::span_main(fork_hitch::Counter::SoundNs);
        drop(black_box(s));
    });
    per_call("add (draw count)", |_| {
        fork_hitch::add(black_box(fork_hitch::Counter::Draws), 1);
    });
    per_call("note_holder", |i| fork_hitch::note_holder(black_box(i)));
    let frame_us = (call - empty).max(0.0) * 10_100.0 / 1000.0;
    println!("platform calls of one play frame (10,100): {frame_us:.1} us");
}
