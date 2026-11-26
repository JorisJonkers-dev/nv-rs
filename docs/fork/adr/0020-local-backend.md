# ADR-0020: The local decomp backend

> A fork decision record kept under docs/fork/adr/ because docs/adr/ is upstream-owned; it takes the next free number after ADR-0019.

**Status:** Accepted (maintainer decision recorded in nv-rs#916, 2026-11-26)
**Date:** 2026-11-26
**Issues:** nv-rs#916 (track C, stories 10-12, 20, 29-36 and 62); nv-rs#947 (this record)

## Context

nv-rs runs the game as a 64-bit program whose engine still lives in a modelled 32-bit
address space. About 47,800 functions are address-level translations that read and write
guest memory at the game's original addresses; about 1,100 are typed native Rust. Every
typed function has a `Diff` test against its translation. A mistake in the translation is
therefore inherited by the typed version: nothing in public CI compares either with the
original code (nv-rs#916).

The community's matching C++ reconstruction (`momusdigital/fnv_decomp`, ADR-0009) gives a
second execution of the same functions, but it may not become part of a distributed build.
Its contributors dedicated their own work under CC0 on 2026-11-19; the game and the
libraries linked into it are not covered. The maintainer decided on 2026-11-26 that the
reconstruction is built where the player or maintainer runs the engine, and that it is the
only reference peer `Diff` compares against.

A prototype built four units and ran them end to end on x86-64 and compiled them for
aarch64. Globals at their original addresses and calls through a table indexed by original
address worked, an empty address trapped cleanly, and the results matched the reference
values. The measured cost was about 1.4x that of equivalent Rust, so the module is a
reference, not a speed path. In 120 sampled units, 63% compiled to wasm32 after two
mechanical source rewrites; 76% compiled with clang as MSVC i686. Units that do not
compile stay as Rust translations.

## Decision

1. The launcher builds the backend on the user's machine from the public source and caches
   the result locally. It fetches clang and wasm2c on first run; where that fails, a
   Cranelift fallback covers the machine. The build is local on every host (Windows,
   Linux, macOS, x86-64 and aarch64); there are no remote builders.
2. Nothing built from the reconstruction leaves that machine: no C++ source, object,
   module, library or built output enters the repository, a test fixture, a release asset
   or a package. The engine runs without the backend when it is not built.
3. Linear memory is the guest reservation. Global symbols are bound to their original
   addresses and the function table is indexed by original address, so guest vtables and
   stored callbacks dispatch into the module; calls to functions outside it go to the
   engine's by-address dispatch. The two source rewrites the build needs are made in the
   build copy only.
4. `Diff` gains an optional reference peer behind a small trait: given the same guest
   memory and arguments, run a function by address and report the return value, outgoing
   calls and written memory, compared byte-level outside the callee's frame. The backend
   is the only peer. Without a peer, behaviour is unchanged. Public CI runs without the
   backend; the private run requires it, and tests that need it report skipped in public
   CI and fail in the private run.
5. Bit-exact x87 cases rely on the existing exact x87 helper and local `nv-call` and
   `nv-pure` runs, not on a second peer.
6. The backend is the fallback for translations marked really incomplete, and is not used
   to raise frame rate. When no family needs guest memory, the guest memory, the
   translations and the backend are deleted together (nv-rs#957), after the ledger shows
   no remaining caller by address.

## Consequences

- A typed function can be checked against a second execution of the game's code on every
  platform, and a translation mistake is caught instead of copied.
- A player needs a toolchain on first run, fetched by the launcher; the engine remains
  playable when the build does not happen.
- The ledger records which peers a function has passed; the queue and the private run
  use those states.
- The module is about 1.4x slower than equivalent Rust and is never a speed path.
- The units that do not compile leave gaps that stay covered by the Rust translations.
- The backend, its build copy and its objects are outside the repository, so no game
  bytes or reconstruction output are committed, uploaded or attached.
