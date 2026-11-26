# Local decomp backend build

`scripts/fork/decomp_backend.py` builds the public `momusdigital/fnv_decomp`
checkout into a native library on the machine that runs the engine (nv-rs#926,
ADR-0020). Nothing it reads from the decomp or writes as output enters the
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
with the local C compiler into `libfnv_decomp.so`.

The library's linear memory is the guest reservation: `decomp_init` maps 8 GiB
with guard pages and makes the first 4 GiB readable and writable, or accepts a
guest mapping supplied by the host. The function table is host-owned, has
16,777,216 entries and is indexed by the original address; each compiled
export is entered at the address recorded in `build/units.json`. Calls to
imports go through `decomp_set_import_hook` to the engine's by-address
dispatch. GOT.mem globals are filled in the order listed in `manifest.json`
from the original addresses in the unit symbol tables. Units that fail to
compile are reported with the compiler's error and are not linked; they stay
on their Rust translations.

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

## Tests

`scripts/fork/tests/test_decomp_backend.py` builds every input from scratch.
It checks the two rewrites, the report, the layout comparison and hygiene
without a toolchain. Setting `NV_BACKEND_TOOLCHAIN_TEST=1` inside the nix shell
adds a test that compiles a synthetic two-unit checkout into a library and
calls its pure functions.

Clang, lld and wabt come from `nix shell nixpkgs#clang_19 nixpkgs#lld_19
nixpkgs#wabt`. The tool finds mingw-w64 headers and libc++ through its standard
nix store paths; `NV_MINGW_INCLUDE`, `NV_LIBCXX_INCLUDE`, `NV_CLANG`,
`NV_WASM_LD`, `NV_WASM2C`, `NV_CC`, `NV_CXX` and `NV_WABT_SHARE` override the
discovery.
