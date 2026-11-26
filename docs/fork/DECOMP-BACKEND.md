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
uses `VirtualAlloc` and `VirtualProtect`. The function table is host-owned, has
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

`report.json` and `report.md` in the output folder list every compiled unit
and every skipped unit with its reason. `manifest.json` lists the callable
exports, their original addresses and the global order.

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

`find_toolchain` looks for clang, wasm-ld and wasm2c on PATH, then in a per-user
cache outside the repository, and fetches them once when they are missing. The
cache is `<XDG_CACHE_HOME>/nv-rs/backend/toolchain` on Linux and macOS and
`<LOCALAPPDATA>\nv-rs\cache\backend\toolchain` on Windows; `NV_TOOLCHAIN_CACHE`
replaces that location. It holds one stamped folder per host and toolchain
version, so a second run does not fetch. `NV_TOOLCHAIN_URL` sets the base URL of
the clang and wabt archives; the launcher supplies it on first run
(rauls-trusty-toolbox#635). Explicit `NV_CLANG`, `NV_WASM_LD` and `NV_WASM2C`
paths win over both PATH and the cache and need no fetch.

When a toolchain is neither present nor fetchable, `build` takes the native
fallback: it compiles the same selected units for the host and gives them the
same `decomp_init`, `decomp_memory_base` and `decomp_call_*` ABI, the same
report and the same manifest as the wasm path. The engine supplies its
Cranelift-backed compiler to that path; without one the tool uses the host C
compiler. Units whose signature is not in `build/units.json` are skipped and
stay on their translations. `--offline` blocks fetching and `--cranelift`
forces this path; both are useful on machines without a network.

Without a built backend nothing loads and the engine's differential harness
runs without a reference peer, as before. `load_library` returns `None` for a
missing library and `verify` reports the missing backend instead of failing the
caller.

## Tests

`scripts/fork/tests/test_decomp_backend.py` builds every input from scratch.
It checks the two rewrites, the address rewrite, the GOT.func and trap
accessors in the generated shim, the report, the layout comparison and hygiene
without a toolchain. Fakes cover each supported host's library name and guest
reservation, a toolchain fetched once and reused from the cache, and a blocked
fetch that falls back to a working native module. Setting
`NV_BACKEND_TOOLCHAIN_TEST=1` inside the nix shell adds a test that compiles a
synthetic two-unit checkout into a library, calls its pure functions, takes a
function address inside it, calls through the stored address and calls an
empty address to check that the trap is reported.

Clang, lld and wabt come from `nix shell nixpkgs#clang_19 nixpkgs#lld_19
nixpkgs#wabt`. The tool finds mingw-w64 headers and libc++ through its standard
nix store paths; `NV_MINGW_INCLUDE`, `NV_LIBCXX_INCLUDE`, `NV_CLANG`,
`NV_WASM_LD`, `NV_WASM2C`, `NV_CC`, `NV_CXX`, `NV_WABT_SHARE`,
`NV_WABT_WASM_WAT`, `NV_WABT_WAT_WASM`, `NV_TOOLCHAIN_CACHE` and
`NV_TOOLCHAIN_URL` override the discovery and cache.
