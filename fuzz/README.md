# Fuzzing the pure parsers

Coverage-guided fuzzing for the three dependency-free parsers: `esm` (plugin
files), `bsa` (archives) and `fos` (save files). The targets only assert that
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
