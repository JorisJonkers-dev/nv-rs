# Fuzzing the pure parsers

Coverage-guided fuzzing for the parsers and decoders of untrusted files: `esm` (plugins, the zlib
decoder, `plugins.txt`, subrecords), `bsa` (archives), `fos` (saves, and the decoders of their
parts), `bink`, `mp3`, `vorbis`, `d3dx-tex` (DDS), `d3d9-stream` (the call stream), `d3d9-wgpu`
(shader bytecode to WGSL) and `nvse` (the save sidecar). The last three pull in `engine-core` or
wgpu, so their weekly jobs compile longer. The targets only assert that
arbitrary input never panics, overflows or reads out of bounds; parse errors are
expected and ignored.

This directory is its own Cargo workspace (`[workspace]` in `Cargo.toml`), the
same way the other tools under `scripts/` and `research/` are. The project rule
is that core crates use no outside libraries, so `libfuzzer-sys` must not leak
into the root workspace.

## Toolchain

The repository pins one toolchain in `rust-toolchain.toml` and that pin is not
changed for fuzzing. `cargo-fuzz` needs nightly, so select it per invocation:

    cargo +nightly fuzz run esm_plugin

`+nightly` takes precedence over `rust-toolchain.toml`, and rustup exports the
choice to the `cargo` processes `cargo-fuzz` spawns. Do not add a
`fuzz/rust-toolchain.toml`: it would silently shadow the repository pin for
anything run inside this directory and split the project across two toolchains.

## Usage

    cargo install --locked cargo-fuzz@0.13.2
    cargo +nightly fuzz list
    cargo +nightly fuzz run esm_plugin -- -max_total_time=600
    cargo +nightly fuzz run bsa_archive -- -runs=100000
    cargo +nightly fuzz run fos_save

Corpora land in `fuzz/corpus/<target>/` and crashes in `fuzz/artifacts/<target>/`.
Both are git-ignored on purpose: the repository hygiene test rejects committed
binary files, so corpora and crash inputs must never be checked in. Reproduce a
crash with `cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<crash-file>`.

CI runs each target for ten minutes weekly (`.github/workflows/fork-fuzz.yml`) and
uploads the artifacts of a failing run.

## A finding becomes a test

A crash input is a binary and is not committed. Reduce it (`cargo +nightly fuzz tmin <target> <file>`),
write the same bytes as a byte-array literal in a unit test of the crate that failed (a few dozen bytes
usually), fix the code, and keep the test: that test is what runs on every pull request.

A manual run of a few seconds per target checks a change to a target: Actions, Fuzz, Run workflow,
`seconds` 30.
