# Local decomp backend build

`scripts/fork/decomp_backend.py` builds the public `momusdigital/fnv_decomp`
checkout into a native library on the machine that runs the engine (nv-rs#926,
nv-rs#929, ADR-0020). Nothing it reads from the decomp or writes as output enters the
repository, a test fixture, a package or a release. The output folder
`.decomp-backend/` is git-ignored, and `hygiene` refuses built artefacts that
are visible to git.

## Commands

```sh
nix shell nixpkgs#clang_19 nixpkgs#lld_19 nixpkgs#wabt -c \
  python3 scripts/fork/decomp_backend.py build \
    --decomp ~/Projects/momusdigital/fnv_decomp \
    --unit third_party/zlib-1.2.1/adler32.c \
    --unit third_party/zlib-1.2.1/crc32.c \
    --out .decomp-backend
```

`build` copies the checkout's `include/`, `src/` and `third_party/` into the
output folder and applies two mechanical rewrites to that copy only: the
decomp's `operator new`/`delete(unsigned int)` and CRT `unsigned int` `size_t`
spellings become `__SIZE_TYPE__`, and every named enum is given `: int`. It
then compiles each selected unit for wasm32 with `-mms-bitfields -fshort-wchar`,
`-fPIC` and `-flto`, links with `wasm-ld` using imported memory and table and a
4 GiB maximum, converts the module with `wasm2c`, and compiles the generated C
with the local C compiler. The library is `libfnv_decomp.so` on Linux,
`libfnv_decomp.dylib` on macOS and `fnv_decomp.dll` on Windows.

The library's linear memory is the guest reservation: `decomp_init` reserves 8 GiB
with guard pages and makes the first 4 GiB readable and writable, or accepts a
guest mapping supplied by the host. POSIX hosts use `mmap` and `mprotect`; Windows
reserves the range and commits its first half with `VirtualAlloc`. The function table is host-owned, has
16,777,216 entries and is indexed by the original address; each compiled
export is entered at the address recorded in `build/units.json`. Calls to
imports go through `decomp_set_import_hook` to the engine's by-address
dispatch. GOT.mem globals are filled in the order listed in `manifest.json`
from the original addresses in the unit symbol tables. Units that fail to
compile are reported with the compiler's error and are not linked; they stay
on their Rust translations.

## Function addresses taken in the module

A callback or vtable slot filled in the decomp holds a function pointer. With
position-independent wasm that pointer is an index into the element table
(`__table_base + i`), not the game's address. Before `wasm2c` runs, `build`
converts the module to WAT with `wasm2wat` and rewrites each address-of
expression to the function's original address from `build/units.json`. The
rewritten module is assembled again with `wat2wasm`. The value a guest vtable
or stored callback holds is then the original address, which indexes the
host-owned table directly; no second mapping is needed when it is called.
`GOT.func` imports, used when the address is of a function outside the module,
get their original addresses through the same symbol table. The number of
addresses bound is written to `report.json` as `bound_function_addresses`, and
the GOT.func values as `got_func_values`.

`decomp_call_*` wrappers catch a trap instead of letting it unwind into the
caller. `decomp_last_trap_code`, `decomp_last_trap_reason` and
`decomp_clear_trap` report the trap; `decomp_table_has` says whether an address
has a function behind it. Calling through an address with nothing behind it
therefore returns to the caller with a `call_indirect` trap recorded, and
`verify` reports the trap and its reason as a mismatch instead of stopping.
The trap state and wasm2c's jump buffer are process-global: call the library
from one thread at a time.

`report.json` and `report.md` in the output folder list every compiled unit
and every skipped unit with its reason. `manifest.json` lists the callable
exports, their original addresses and the global order. `exports.tsv` is the
plain-text form the Rust peer reads: `export <va> <wrapper> <arity> [f32]`,
`import <field> <va>` and `global <va>` lines. It holds addresses already bound
in the shim, no compiled output.

```sh
python3 scripts/fork/decomp_backend.py layout \
  --decomp ~/Projects/momusdigital/fnv_decomp \
  --unit src/bscore/bscoremessage.cpp --out .decomp-backend/layout
```

`layout` dumps record layouts for each unit with clang as i686-MSVC and as
wasm32 with the same two flags, normalises the vtable and anonymous-member
spellings, and compares every record both targets lay out, including its
offsets, size and alignment. It writes `layout.json` and exits 1 on any
difference. Units that compile on only one target are listed as skipped.

`verify --library ... --manifest ... --vectors ...` calls exports through the
library and compares their return values with results produced by the
translations; the vectors may be a JSON array or JSON lines. `hygiene --root .`
lists built artefacts visible to git and exits 1 when there are any.

## Hosts and the toolchain cache

The build runs on Linux and macOS on x86-64 and aarch64, and on Windows. The
host only decides the library name, the native link flags and how guest memory
is reserved; the two layout flags, the i686 guest defines and the address table
are the same on every host.

`find_toolchain` looks for clang, wasm-ld, wasm2c, wasm2wat and wat2wasm on PATH,
then in a per-user cache outside the repository, and fetches them once when they
are missing. The cache is `<XDG_CACHE_HOME>/nv-rs/backend/toolchain` on Linux and
macOS (`~/.cache/nv-rs/backend/toolchain` when `XDG_CACHE_HOME` is unset; macOS
does not use `~/Library/Caches` here) and
`<LOCALAPPDATA>\nv-rs\cache\backend\toolchain` on Windows; `NV_TOOLCHAIN_CACHE`
replaces that location. It holds one stamped folder per host and toolchain
version; the binaries and the wasm2c runtime files are checked on every use, so
a second run does not fetch while a broken cache is fetched again. A cached host
supplies the whole tool set: PATH tools are not mixed with a cached wabt.
`NV_TOOLCHAIN_URL` sets the base URL of the clang and wabt archives; the
launcher supplies it on first run (rauls-trusty-toolbox#635). No release is
published at the default URL yet, so fetching needs both a published archive
and SHA-256 pins in `NV_TOOLCHAIN_SHA256` (JSON with a `clang` and a `wabt`
key, or the `NV_TOOLCHAIN_SHA256_CLANG` and `NV_TOOLCHAIN_SHA256_WABT`
variables). An unpinned or mismatched archive is refused before it is unpacked,
and archive members may not write outside the cache folder. Explicit
`NV_CLANG`, `NV_WASM_LD`, `NV_WASM2C`, `NV_WABT_WASM_WAT` and
`NV_WABT_WAT_WASM` paths win over both PATH and the cache and need no fetch.

When no toolchain is available, `build` stops with that reason: the native
fallback is a different module and is never substituted silently. `--offline`
blocks fetching. `--cranelift` takes the native fallback explicitly: it
compiles the same selected units for the host with the host C compiler (a
`compiler` can be injected by a caller; no Cranelift compiler is part of this
tool); it keeps the `decomp_init`, `decomp_memory_base` and `decomp_call_*` ABI,
the same report and the same manifest shape. Its `manifest.json` lists the
decomp's symbol names and their addresses in `globals`/`global_values`, because
native code binds symbols at link time and has no wasm GOT slots to fill. Units
whose signature is not in `build/units.json` are skipped and stay on their
translations. The fallback's exports are registered at their original
addresses in its table, but it does not run the wasm module's address-indexed
guest code, so it is for machines where that module cannot be built.

Without a built backend nothing loads and the engine's differential harness
runs without a reference peer, as before. `load_library` returns `None` for a
missing library and `verify` reports the missing backend instead of failing the
caller.

## Tests

`scripts/fork/tests/test_decomp_backend.py` builds every input from scratch.
It checks the two rewrites, the address rewrite, the GOT.func and trap
accessors in the generated shim, the report, the layout comparison and hygiene
without a toolchain. Fakes cover each supported host's library name and guest
reservation, a toolchain fetched once and reused from the cache, a blocked
fetch that reports why no wasm module was built, and an explicitly requested
native fallback that produces a working module. Setting
`NV_BACKEND_TOOLCHAIN_TEST=1` inside the nix shell adds a test that compiles a
synthetic two-unit checkout into a library, calls its pure functions, takes a
function address inside it, calls through the stored address and calls an
empty address to check that the trap is reported.

Clang, lld and wabt come from `nix shell nixpkgs#clang_19 nixpkgs#lld_19
nixpkgs#wabt`. The tool finds mingw-w64 headers and libc++ through its standard
nix store paths; `NV_MINGW_INCLUDE`, `NV_LIBCXX_INCLUDE`, `NV_CLANG`,
`NV_WASM_LD`, `NV_WASM2C`, `NV_CC`, `NV_CXX`, `NV_WABT_SHARE`,
`NV_WABT_WASM_WAT`, `NV_WABT_WAT_WASM`, `NV_TOOLCHAIN_CACHE`,
`NV_TOOLCHAIN_TIMEOUT` and `NV_TOOLCHAIN_URL` override the discovery and cache;
`NV_TOOLCHAIN_SHA256` supplies the archive pins a fetch requires.

## Real gaps (nv-rs#930)

About 5,860 `// Translation incomplete:` markers exist. A marker whose reason names only the C++
exception frame (`FS:[0]`, SEH, a funclet or filter) or the `/GS` cookie check is exception-only
(about 5,760); one that also names an uninitialised read, an x87 or MXCSR state, a register-only
entry or a write past a frame is a real gap (96). `python3 -I scripts/fork/real_gaps.py --root .`
writes them to `crates/engine-core/src/typed/real_gaps.txt` (`--check` fails when it is stale; a
unit test compares the committed list with the tree). Exception-only markers are counted, not listed.
The split is by what the reason says is left out (a clause that names more than the frame or the
cookie makes a gap), and the unit tests pin the mixed cases, including a reason that reads
"the frame is not modeled, and the new block's slots are read before they are written".

The list has a third column. `eax` means the backend's one-word result is all a caller can read.
`registers` means it is not, and the gap stays on its translation: the marker names the x87 or
SSE state, `EDX`, `ST0`, a register input or a register-only entry, a stale register or XMM lane,
or the translation's own signature returns a `u64`, `f64` or `Ret` (the script reads the function
that follows the marker). That is decided from the marker and the source, not from where the file
lives.

With `NV_BACKEND_LIBRARY` set to the built library and `NV_BACKEND_GAPS=1`, `Engine::new` registers
the backend's function at each `eax` address it has, after the typed families are applied.
Without both variables, or without a built backend, nothing is registered and the engine behaves
as before. The switch is separate from the `Diff` peer on purpose: the differential cases keep
comparing the translation with the backend.

The switch is inert until nv-rs#1000 lands: the backend cannot yet build the units that hold the
gaps, so no export exists at a gap address and no `Diff` case has run against a real backend for
them. #1000 also widens the call to return `EAX`, `EDX` and `ST0`, after which the `registers`
rows can be reconsidered.

Two refusals, each one `backend-gaps:` line on stderr and nothing registered:

- Real threads. A backend call holds one process-wide lock (the module's trap state and import
  hook are global) while its import hook runs engine code, and that code can wait on another real
  thread that then needs the lock. The switch is therefore refused when `NV_THREADS` names `real`;
  a registered gap called on an engine that runs real threads anyway (the host may choose them)
  stops with a message naming the address. The mutex is not released across import calls, because
  the module's trap state would then be shared by two calls at once. With stepped threads only one
  thread runs at a time, so the lock changes no interleaving.
- A stale `.tsv`. Before it registers, the loader checks that `decomp_init` and every wrapper the
  `.tsv` lists resolve in the library. A `.tsv` left over from another build names a symbol the
  library lacks, and then nothing is registered. (An arity or signature change under the same name
  is not visible from the library; the build writes both files together.)

When the switch is on and the library does not load, or loads but fails the check, the line says
why. On success it prints `backend-gaps: N of M real gaps registered` once. The private run's
`real gaps on the backend` command runs `cargo test -p engine-core --lib typed::backend_gaps` with
both variables.

Status: the backend could not be built for the gap units on the machine this was written on (see the
pull request): the wasm toolchain rejects the units' inline x86 assembly, and the few that compile
stop at unresolved GOT symbols.
