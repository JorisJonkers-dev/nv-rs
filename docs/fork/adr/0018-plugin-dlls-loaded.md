# ADR-0018: Script-extender plugin DLLs are loaded, beside the built-in extenders

> A fork decision record kept under docs/fork/adr/ because docs/adr/ is upstream-owned. It supersedes ADR-0016's rule that no plugin DLL ever loads; the rest of ADR-0016 (the command table in guest memory, the wrap chain, the built-in crates) stands.

**Status:** Proposed (design for the maintainer's decision; the maintainer said on 2026-11-22 that loading DLLs is essential)
**Date:** 2026-11-22
**Supersedes:** [ADR-0016](../../adr/0016-script-extenders-built-in.md), "no plugin DLL ever loads" only.

## 1. What ADR-0016 decided, and why

ADR-0016 (2026-11-07) built xNVSE and JIP LN NVSE into the engine as Rust crates, translated from their source with the authors' permission. It built them in because the engine runs the game's code translated to Rust inside a model of the 32-bit address space; it executes no x86 at all, so a plugin DLL could not run. The same reasons keep it that way for the other extenders (`jg`, `showoff`, `stewie`, `ttw-nvse`) and `docs/FIXES.md` says "no plugin DLL ever loads". The cost: every command and every game patch of every plugin has to be translated by hand. JIP LN is 166,153 lines of Rust in `crates/jip` (1,118 commands, 183 game patches; 91 commits, 2026-11-12 to 11-21) and xNVSE 93,703 lines (59 commits, 11-07 to 11-21). JohnnyGuitar has 11 of 412 commands and ShowOff 16 of 215. The owner's profile (312 mods) ships 47 plugin DLLs; its scripts call 44 JohnnyGuitar names and 33 ShowOff names the engine lacks (EXTENDERS.md, 2026-11-22), and 36 of the 50 DLLs the other lane counted have no counterpart at all.

## 2. What changes

The goal changes from "nothing to install, nothing loads" to "a mod's plugin DLL runs when it can, and the built-ins answer when it cannot or is absent". Concretely:

1. A new fork-owned crate (`crates/plugin-host`; not `nvse`, `jip`, `jg`, `showoff`) maps 32-bit plugin DLLs into the guest address space and runs their x86 code in an interpreter that works on the guest memory. Off by default (`bLoadPluginDlls=0` in `[nv-rs Plugins]` of Fallout.ini, `NV_PLUGIN_DLLS`), per plugin allow list.
2. xNVSE itself stays built in, always: it is the host a plugin talks to (its interfaces are the `nvse` crate's). Plugins are loaded in xNVSE's own order (file name without case), after the built-ins have registered.
3. **Policy per plugin, by name:** a built-in always wins (the owner's rule 2 of 2026-11-22, section 8, "What the host does with a DLL"). A DLL for an extender the engine builds in (xNVSE, JIP LN, JohnnyGuitar, ShowOff, MCM, UIO, ttw_nvse and the other built-ins of `fork_plugins::BUILT_IN`) is not loaded, so no opcode block is registered twice; the verdict is "built in" and a missing command of a built-in is built natively. The host loads a DLL, when the setting is on, only for a plugin with no native build and only if it passes preflight (section 6). The real JohnnyGuitar and ShowOff DLLs are still run in the steps of section 8, to measure how far they get and to compare their commands with the built-ins' one by one; that does not switch the built-in off.
4. A plugin DLL for which nothing is built in is loaded when the setting is on.

## 3. The choice, with numbers: load the real DLLs, or rebuild by hand

| | Rebuild by hand | Load the real DLL |
|---|---|---|
| JohnnyGuitar + ShowOff, what is left | 401 + 199 = 600 commands, at JIP's measured 149 Rust lines per command about 89,000 lines, plus the plugins' own game patches and events, again for every plugin release (JG 525, ShowOff 184 are the versions in the profile) | nothing per command |
| The other 45 DLLs | each one separately, if at all (36 have no counterpart) | the same loader; the loadable ones (section 5) need no per-plugin code |
| One-time cost | none | x86 core (interpreter: the profile's three heavy DLLs hold 457,269 instructions in 327 distinct mnemonics, 99% of them in about 120: integer core, SSE/SSE2 scalar and packed, little x87, which `engine-core` already models), PE mapper, import shims, interface tables, patch handling, runtime. Estimate 20,000 to 30,000 lines (an estimate, not a measurement) |
| Faithfulness | a translation: right where it was checked | the plugin's own code, bugs included |
| Risk | known | the loader may not reach the heavy plugins at all (section 8 measures it) |

Import coverage (565 distinct imports over the 47 DLLs; 108 already provided to the game by the platform layer; kernel32: 212 distinct, 86 provided): JohnnyGuitar misses 17 kernel32 functions (SRW locks, `VirtualProtect`, `InitializeSListHead`, ...), ShowOff 27, JIP 10; none of the 47 has all its kernel32 and user32 imports provided today, so every plugin needs a few more shims (small functions). The three heavy plugins also import 88 `msvcp140` C++ library symbols (iostreams, locale facets, mutex), 17 `vcruntime140` (C++ exceptions: `_CxxThrowException`, `__CxxFrameHandler3`) and 85 UCRT functions. 13 of the 47 link the runtime statically and need kernel32 only.

The honest reading: loading is less work than rebuilding once the two plugins are counted together (about a quarter of the lines, once), and more faithful, but it is a bigger step into the unknown: the runtime libraries and the patches (section 4) decide whether it works. Hence the gate in section 8, and the built-ins stay until the gate is met. The second worker's JohnnyGuitar and ShowOff commands are not wasted: they are the answer without the DLL and the reference the loaded DLL is compared with command by command.

## 4. What a plugin DLL needs from its host, and what the engine offers

**The calls.** `NVSEPlugin_Query(const NVSEInterface*, PluginInfo*)` then `NVSEPlugin_Load(const NVSEInterface*)` (and `NVSEPlugin_Preload` where exported: CellOffsetGenerator, CrashLogger, NVTF). `NVSEInterface` holds the versions (`nvseVersion` 0x06040090 as ADR-0016 answers, `runtimeVersion` 0x040020D0, `isEditor` 0), `RegisterCommand`, `RegisterTypedCommand`, `SetOpcodeBase`, `QueryInterface`, `GetPluginHandle`, `GetRuntimeDirectory`, `InitExpressionEvaluatorUtils`. Built in guest memory with function words pointing at stubs in the extender code range (`mem::EXTENDER_CODE`).

**The interfaces `QueryInterface` hands out:** messaging (listeners, `Dispatch`; `crates/nvse/src/plugins.rs` already has the message kinds and dispatch), serialization (co-save records; `crates/nvse/src/saveload.rs`), string variables, array variables, script (`CallFunction`, `CompileScript`, `CompileExpression`), command table (`GetByOpcode`, `GetByName`, `GetReturnType`), data interface (Stewie-style game data), event manager (`RegisterEvent`, `SetNativeEventHandler`), console. xNVSE's Rust side (`vars.rs`, `functions.rs`, `events.rs`, `saveload.rs`) is the implementation; the new crate adds the guest-memory structure and the call stubs.

**Registering a command.** `RegisterCommand(CommandInfo*)` writes a record into the guest script table (ADR-0016 section 2: names, parameter list, handler word at `+0x18`). The handler word is an address in the plugin's image. `EvaluateLine` calls handler words by address through `Engine::call`; an address inside a plugin image goes to the interpreter instead (section 7). Nothing in the evaluator changes. Opcode bases: a plugin's `SetOpcodeBase` is honoured; a base inside a built-in's block is refused and reported.

**The game's own addresses.** A plugin calls game functions at fixed addresses (`0x00XXXXXX`): the interpreter forwards a call into the exe range to `Engine::call(addr, args)`, which runs the translation (or the typed function behind it, ADR-0014 point 2). It reads and writes game objects at their offsets in guest memory. This is the same as the built-ins do.

**What the engine does not have.** Hazards: (a) objects of a typed family that stage 2 moved to host memory behind handles cannot be read by a plugin that reads raw offsets (ADR-0014: a family moves only "when nothing untyped reads their fields"; a plugin is an untyped reader), so with the setting on, those families stay in guest memory (`NV_TYPED=<family>:off`, by a table the loader checks at start). (b) Game code is translated, not executed from bytes, and the Steam executable's code is encrypted and never read (section 7). (c) The renderer is replaced by wgpu behind an own Direct3D 9 layer (ADR-0007): plugins that need a real device do not work.

**Windows API.** The platform layer (`engine-platform`, `// Platform: replaces <DLL> <name> (import)`) already provides 179 imports to the exe (101 KERNEL32, 21 USER32, 11 d3dx9_38). A plugin's import is resolved by DLL and name against that table first (a thunk address in the extender code range whose stub calls the same Rust function), then against a plugin-shim table (the missing kernel32 functions above: all small), else the module loads with that import bound to a stub that records "plugin X wanted DLL!name" and stops that plugin's call (section 6). The functions a plugin calls on the game's own state (`GetModuleHandle("FalloutNV.exe")`, `VirtualProtect` on exe code) are answered from the model.

**Image placement.** Plugin DLLs default to the linker's default base for a DLL `0x10000000`, which is the start of the stage-2 moved area; that base is only the relocation origin, not where the image is placed. `Pe::map` in `crates/plugin-host/src/pe.rs` allocates the image from the guest heap with `e.mem.alloc`, page-aligns it and applies the relocation delta. The heap starts at `0x20000000`, above the moved area, so an image never lands in the moved area. A DLL without relocations is refused and reported unless the heap happens to allocate it exactly at its declared base. One image is 4 KB (HDPipboy) to 1.2 MB (SUP).

**The C++ runtime.** Fourteen of the 47 import `msvcp140`, 26 use the UCRT and `vcruntime140`. Two routes, decided by the gate: (1) map the real x86 Visual C++ runtime DLLs when the user's machine has them (Windows: `SysWOW64`; elsewhere a Visual C++ 2015-2022 x86 redistributable, fetched by raul from Microsoft's own download address after the person agrees, hash-checked, kept in raul's cache, never in a repository or release asset) and provide their kernel32 imports; or (2) implement the needed subset natively (88 + 17 + 85 symbols for JIP, JG and ShowOff). Route 1 shrinks the work to the Windows layer and is the first attempt.

## 5. Where loading cannot work by construction

The 47 DLLs, by what they do (from names, imports and strings; the preflight verdict of section 6 replaces this guess, nothing here has been run):

- **Loadable by design (interfaces only, plus game calls): about 20.** JIP LN, JohnnyGuitar, ShowOff, kNVSE, MCM, UI Organizer, ttw_nvse, yUI, SUP NVSE, AnhNVSE, Improved Console, Console Paste, Character Preset Menu, Better Caravan, Modern Minimap, Climate Control, Cloud Upgrade, MoonlightNVSE, Cell Offset Generator, High Res Local Maps. Many also write patches into the exe (JIP 183, JohnnyGuitar and ShowOff several): section 7.
- **Patch-only (no script commands), work only through section 7, most already have a fix row: about 11.** ActorCause Save Bloat Fix, VATS Lag Fix, Mod Limit Fix (row `NV_DIRECT_FILES`), Muzzle Flash Light Fix, Audio Region Fix, Combat Lag Fix, LOD Fixes, zlib Updated, External Emittance Fix, Mobile Pip-Boy Light, Fog-based Object Culling, lStewieAl's Tweaks (1.0 MB, 158 rows built, 368 more not).
- **Renderer and DirectX: never by loading, need a built-in answer or a refusal: about 11.** Fallout Shader Loader, Vanilla Plus AO (imports `d3dx9_43`: texture and matrix functions, needs a real device), Vanilla Plus Terrain, Depth Resolve, High Resolution Bloom, High Resolution Water Fog, High Resolution Screens (HDPipboy), Depth of Field Fix, Alpha Fixes, LOD Flicker Fix, Viewmodel Shading Fix. They patch the game's renderer preparation and shader constants or call the Direct3D 9 device; the device here is the engine's own layer over wgpu (ADR-0007), the shaders are translated, and there is no GPU-side `IDirect3DDevice9` for them to hook. Where they only change a constant in a function that is still translated game code, section 7 can carry them; where they replace a shader or call the device, only a fix row can (issues #169, #170).
- **Allocator, timer, crash handling: never by loading: 4.** NVAC (patches the heap and memory manager, which the engine replaced by Rust: its effect is the engine's own allocator), Engine Optimizations (the same, partly rows), NVTF (its `d3dx9_43` and `winmm` use, and the `BSTimer` replacement: a fixed-step row exists), Yvile Crash Logger (imports `dbghelp` `StackWalk`/`SymFromAddr`, `dxgi`: it walks a native x86 stack that does not exist here; the engine reports its own stops). These are refused with a report line naming the plugin and why.

## 6. A plugin that fails never stops the engine

- Every call into a plugin runs under a guard: an unmapped read or write, an instruction the interpreter lacks, an unresolved import, a stack overflow, an infinite loop (an instruction budget per call) or a Rust panic inside the interpreter stops that call, marks the plugin failed, unregisters its commands (a script that calls one then fails at that line as it does without the DLL today, ADR-0016) and writes one line in the play log: which plugin, which export or command, which address and instruction, which import or address it wanted. The game goes on.
- **Preflight, once per file, cached by the file's SHA-256 and the engine version** (`plugin-reports/<hash>.json` beside raul's other caches; raul does it when a mod is installed or a Profile is started, never by hand): decode every export's reachable code and list instructions the interpreter lacks; resolve every import and list the missing; run `NVSEPlugin_Query` and `Load` against a recording host in a throw-away engine and record the commands registered, the messages listened to and every write into exe code (address, length, the translated function it falls in). Verdict: `loadable`, `loadable with patches` (all sites known), `needs` (list), `refused` (class of section 5). Raul shows the verdict per mod; nothing is translated or rewritten, because the interpreter runs the DLL as it is.
- Verification of a verdict: the test DLLs of section 8 (made-up, built from our own source at test time); for real DLLs, the preflight report is compared with a run: each registered command is called once with a made-up argument set and the result is compared with the built-in's answer where there is one.

## 7. The game's code is not bytes: patches

The exe is not executed from its bytes; each function is a Rust translation registered at its exe address (a few are typed native, ADR-0014). A plugin that writes into exe code (`SafeWrite8/16/32`, `WriteRelCall`, `WriteRelJump`, a `VirtualProtect` and a `memcpy`) changes bytes nothing runs. Three mechanisms, in the order used:

1. **A table of supported patch sites** (named events): the sites the built-ins already know are seams and wraps in `engine-core` (`SITES`, `WRAPS`, and the JIP, Stewie and nvse rows: hundreds). When a plugin's write matches a site in the table by address and by the bytes it writes (a call to its own function in a known place), the loader maps it to that seam: the seam then calls the plugin's function through the interpreter. This keeps the plugin's own logic and the engine's translation.
2. **A patched function run from its original bytes: not available on the Steam executable.** The idea: map the exe's code read-only and run a patched function's original x86 bytes in the interpreter, its calls into other exe addresses going back to `Engine::call`. It would make any patch inside a function work. But `crates/engine-core/src/exe.rs` maps only `.rdata`, `.data`, `.tls` and `CONST`; the Steam executable's `.text` is encrypted (SteamStub) and the engine never reads it, only the GOG profile pins a `.text` hash. So on a Steam install there are no original code bytes to run, and only a person-supplied unpacked executable (GOG) would allow this. It is recorded as an option for that profile, not part of the first steps.
3. **Everything else is reported, never ignored, and does not stop the plugin:** one play-log line, "plugin X wrote N bytes at 0x00XXXXXX (function 0xYYYYYYYY, in the table: no)"; the write is dropped and the plugin goes on. A plugin that needs the patch then misbehaves in a way the report explains, and raul shows the list per mod.

The consequence for section 3: a loaded JohnnyGuitar or ShowOff gives its commands for free, but every hook inside a translated function needs a seam in the translation (a reviewed one-line edit, as the built-ins have today), added by hand, the same cost as the built-in's patches. Commands that only read and call (most of the 600) have no such cost; plugins that are mostly hooks (JIP's 183 patches, Stewie's 368 unbuilt rows, the render class) do, which is why they stay built-in rows.

Where a plugin's change is something nv-rs offers itself (a fix row or a setting), the row is the answer and the DLL is not loaded for it (section 2, rule 3).

## 8. Per system, and what the first step proves

**What the engine is on each system.** The same program on all of them: the game's exe translated ahead of time into Rust (no x86 interpreted, emulated or recompiled at run time), on a 64-bit host with a 4 GiB model of the guest address space (`mmap` of a sparse reservation; Windows `VirtualAlloc`). It is native on Linux x86-64 and Windows x86-64. macOS Apple silicon and Intel build the same code; the start is still refused on Apple silicon (#698), independent of this ADR. So a plugin's 32-bit x86 code cannot "run natively" anywhere: a 64-bit process cannot enter 32-bit code on Linux or macOS, and the guest is not a flat host mapping on Windows either. Hence one portable x86 core, identical on all four, and the plugin's imports come from the same Rust platform layer as the game's.

| | Linux x86-64 | Windows x86-64 | macOS arm64 | macOS x86-64 |
|---|---|---|---|---|
| The plugin's x86 code | interpreter | interpreter | interpreter | interpreter |
| Executable memory needed | none | none | none (no W^X or entitlement) | none |
| Windows API | platform layer + plugin shims | the same (not the host's: the plugin sees the model) | same | same |
| C++ runtime DLLs | redistributable fetched into raul's cache, or native subset | `SysWOW64` copy read, else as Linux | as Linux | as Linux |
| Case-insensitive file names, `\` | by the platform layer's file seam (`mod_files`) | native | seam | seam |
| Status | first step runs | run by CI check and the same test | unit tests only until the start is allowed (#698) | same |

A faster route (translating plugin x86 to host code ahead of time or at load) is not part of this ADR; it needs one back end per host instruction set (x86-64, aarch64) and is only worth it if the interpreter's measured cost on the heavy plugins' hot paths asks for it. The preparation step of section 6 is built so that a translation cache can be added behind the same hash key.

**Licence.** A plugin DLL is its author's binary. The loader reads it from the user's mod folder, maps it in memory and runs it, on the user's machine, for the user's own use: the same act as Windows loading it. Nothing is translated to a file that is shared, nothing is redistributed. No plugin binary, and no Visual C++ runtime binary, enters any repository, test fixture or release asset: tests use a DLL assembled at test time from our own source (a made-up PE built in memory).

**Gate and first step.**
1. First working step (this ADR's first pull request): the crate with the PE mapper, an x86 core for the instructions of a small test plugin, the interface structures for `Query`, `Load`, `RegisterCommand`, messaging `RegisterListener`, and one test: a made-up PE32 DLL (assembled in the test from our own source) registers one command and answers one message (`kMessage_PostLoad`), the command runs through `EvaluateLine`, with the setting off by default and no effect when off. Runs on Linux; the same test is platform independent, so Windows and macOS need on top only: the Windows-API shims' file and registry functions on their own file systems, and (macOS arm64) the allowed start.
2. Then the real DLLs, in this order: JohnnyGuitar (17 missing kernel32 functions, 32 msvcp140, C++ exceptions), ShowOff (27, 78), kNVSE (53 kernel32 and user32 imports missing, no msvcp140); the report says how far each gets (mapped, queried, loaded, which import or address or instruction stops it).
3. The gate for turning a real DLL on by default: all commands the plugin registers are called once with made-up arguments with no stop; the commands that the built-in has give the same results; no unexplained patch write; a bench shows the interpreter's cost on the heavy plugins' per-frame calls under the hitch budget.

**What the runtime is made of (built).** The plugins import three groups of C runtime. The engine itself provides, as host functions over guest memory (`crt.rs`, `winapi.rs`, `printf.rs` in `crates/plugin-host`, written from the documented behaviour): vcruntime140's memory and string functions, the Universal CRT (`api-ms-win-crt-*`: heap, strings, conversion, environment, `FILE` streams over the mod-files seam, `printf`, start-up and exit tables) and the kernel32 and user32 functions plugins call. The C++ standard library (`msvcp140.dll`: iostreams, locale, `std::string` helpers, 88 imports of JohnnyGuitar alone) is not rewritten: it is mapped like any other DLL from a file the user supplies (`plugin_host::supply_runtime`; the host reads every `.dll` of the folder named by `NV_PLUGIN_RUNTIME` before it loads the plugins), and a plugin's import of `msvcp140.dll` is bound to that file's exports. What must be on a user's machine: nothing for plugins that import no C++ library; for those that do, the user's own `msvcp140.dll` (32-bit, from the Visual C++ 2015-2022 redistributable). Decided by the owner (2026-11-22, "user's copy, else fetch"): raul looks for the files in the user's own game or Proton folders first and, if they are absent, offers to fetch Microsoft's own installer on request and unpacks it; the host takes the folder of those files as a setting (`NV_PLUGIN_RUNTIME`). Tracked in nv-rs #574 and Toolbox #551. Nothing of Microsoft's is shipped in a repository, a test or a release asset; the tests supply a made-up DLL assembled in the test. Not yet built: the floating-point functions of the math library (they return in the x87 and SSE registers, which wait for the instruction set), `DllMain` of the plugins (needs `fs:` access and SSE), C++ exceptions.

**What the host does with a DLL: the owner's rules (2026-11-22).**
1. The plugin host stays opt-in (off by default). raul shows one verdict per plugin: loads, serves commands, built into the engine, not needed here, not working. 0.7.0 ships when the plugins the owner's own scripts call commands from are served correctly.
2. Built-in always wins: a DLL for an extender the engine has natively (xNVSE, JIP LN, JohnnyGuitar, ShowOff, ttw_nvse, and the other built-ins of `fork_plugins::BUILT_IN`) is not loaded; the verdict is "built in". Missing commands of a built-in are built natively. The host loads only plugins with no native build.
3. A plugin that works by patching the original program's machine code (stability fixes, tweaks) cannot take effect in a native engine. Each is marked "engine already has the fix", "not needed here" or "feature missing"; only features the owner's mods use are rebuilt natively, each as its own ticket.
4. The data comes from the engine (`plugin_host::verdicts`, written beside the log as `mod-support.json`); raul shows it on the Doctor page and through `raul plugins check`.

What a patching plugin wrote into the game's code is kept as its patch-site table (`plugin_host::patch_sites`: address, bytes, whether the site is a function's entry the engine can wrap), reported once per plugin and dropped. A plugin that reads the original bytes before it patches (about eight of the fifty) refuses to patch when it reads zeros: the host reads the executable's code from an unpacked `FalloutNV.exe` the user supplies (`NV_PLUGIN_CODE`; the Steam file's code is encrypted and is refused).

## The verdict file (`mod-support.json`)

When `NV_PLUGIN_VERDICTS` names a file, the host writes one verdict per plugin DLL it was given (UTF-8 JSON, written after the plugins were loaded). Format:

```json
{"plugins": [{"file": "name.dll", "mod": "Mod name", "verdict": "...", "reason": "...", "ticket": 550,
  "mapped": true, "queried": true, "loaded": true, "commands": 0, "patch_writes": 1, "missing_imports": 0}]}
```

- `verdict` is one of: `built into the engine`, `loads`, `serves commands`, `engine already has the fix`, `not needed here`, `feature missing`, `not working`.
- `reason` is plain text for the user, with what the host saw (the instruction it stopped at, the number of writes into the game's code it dropped).
- `ticket` is the nv-rs issue that tracks the plugin (0 when none); a file the table does not know is judged by the run alone.
- The booleans and counters are what the host observed: mapped, `NVSEPlugin_Query` and `NVSEPlugin_Load` answered yes, commands registered, writes into the game's code recorded, imports with no answer.
- The verdicts of the patch-only plugins rest on the plugin's published purpose, not on a run of the game; the reason says so.

## Consequences

- `docs/FIXES.md` and `.fork/capabilities.json` gain a section "Plugin DLLs" (generated by the fork's tools, never by hand); `capabilities.json` lists per plugin whether the answer is the built-in or the DLL.
- ADR-0016's "no plugin DLL ever loads" is superseded; the built-in crates stay as the answer without the DLL.
- A new crate and a new row of CI bookkeeping (`.fork/components.toml`, `.fork/overlay-paths`).
- The raul side (verdict per mod, the runtime fetch) is a Toolbox issue once the first step works.

## Open questions

1. Which of the patch sites JohnnyGuitar and ShowOff write (the preflight list) already have a seam, and how many need a new one.
2. How many typed families are stage 2 (host memory) today, and which hold objects a plugin reads?
3. Route for the Visual C++ runtime: decided (the user's copy, else fetch on request; nv-rs #574, Toolbox #551).
4. Whether JIP LN keeps the built-in after the gate (it is complete and tested: recommended yes).
