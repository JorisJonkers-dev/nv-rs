# Why the fork's tests take long, and a ranked redesign

Analysis only: no engine crate, test or workflow is changed by this document. It follows
[`CI.md`](CI.md) and answers: after the component split, what is the critical path made of, which tests are
slow and why, what a change really forces to recompile and retest, where fuzzing fits, and which change buys
the most minutes for the least effort and the least divergence from upstream.

**Measured** means read from a log or computed from measured numbers by arithmetic that is shown. **Estimated**
means a model or a read of the code that no run has confirmed; every estimate says what would confirm it.

## 1. Verdict

1. The critical path is one job: `typed shader family` (45.2 min in run 43464769007, 46.7 min on the `fork` push
   43472725695). It is 40 s setup, 3 m 47 s compile and 2,454 s (40.9 min) of one test binary (`typed_shader`,
   249 tests). The second and third are geometry/pathing (23.1 min) and scene queries (19.7 min). Everything else
   is under 12 minutes. (Measured.)
2. The time is **CPU time of a few dozen tests**, not volume of tests: 36 of the 249 shader tests run longer than
   60 s and account for about 7,200 of the 9,800 CPU-seconds the binary uses (73 %); one test, `select_pass`,
   takes about 820 s on its own thread. The other 213 tests average 12 s. (Measured from timestamps, see 3.2.)
3. Each test is a **differential run of up to a few thousand cases, and every case builds two or three fresh
   engines** (`Diff::side` calls `Engine::new_untyped`, which collects and registers all 47,733 translated
   functions each time; the tests then run it again for the second control word). The per-case cost is therefore
   paid dozens of thousands of times per binary. (Read from the code; the share of engine construction in that
   cost is **not measured**: Section 7 says how to measure it with one test.)
4. The fork has one lever that needs no change to upstream's files and moves the wall-clock the most:
   **split the heavy binaries across jobs by measured test time.** Estimated critical path 18 to 24 min instead of
   45 to 47 (Section 6, item 1).
5. The levers that make the tests themselves cheaper are small changes in `engine-core`'s harness (upstream's
   file, offerable): reuse the function table between engines, and run the second control word only when the
   first run touched the x87 state. Estimated 30 to 60 % of the test CPU; each needs the one-test measurement
   of Section 7 before it is claimed.
6. "Extract crates so a change does not recompile and retest everything" has less to give than it looks. In the
   last 400 upstream commits a code change touches one crate in 155 cases and two in 91, but `engine` links all 19
   engine crates, so selection by crate closure reaches the typed jobs for 253 of 283 commits (`CI.md`). Selection
   by what the typed tests actually execute would skip them for about one commit in three (Section 5). The real
   extraction candidates are small (the test harness out of `engine-core`; typed families into a test crate with a
   smaller assembly), and each needs a proof before it is trusted.
7. Fuzzing is cheap to widen and does not touch the critical path: nine more entry points of parsers and
   decoders qualify (Section 4). It buys safety, not minutes, and runs weekly, off the pull request path.

## 2. What was read and measured

| Source | Used for |
| --- | --- |
| Run 43464769007 (pull request `ci/components`, all components, cold cache) and run 43472725695 (push to `fork`) | per-job minutes of every job; job logs of the integration jobs, fallout and engine-base jobs |
| `.fork/test-timings.json` (upstream's single job, run 43417602878) | per-binary seconds, to cross-check the fork's logs |
| `crates/engine/tests/typed_*.rs`, `crates/engine-core/src/typed/diff.rs`, `engine.rs`, `Cargo.toml` of the crates | what a test does per case; the dependency graph |
| `scripts/fork/tests/fixtures/workspace-graph.json` and `crates/*/src` line counts | reverse dependencies, recompile sets |
| `git log` of the last 400 non-merge commits of `fork` (upstream's, replayed) | which crates a change really touches |

The run id named in the request, 43464768426, is the `Security` run of the same pull request (two jobs, 15 s);
the pipeline is 43464769007, which is what is used here.

**Local measurement was not possible.** The engine crate does not build on Linux today (`engine-platform`
line 214, see `CI.md`), and this workstation has no C linker on `PATH`, so not even a stand-alone benchmark
links. Nothing below comes from a local run. The jobs' own logs hold no per-test times (libtest prints them
only with `--report-time`), so per-test durations below are reconstructed from the "has been running for over
60 seconds" lines (3.2).

## 3. Where the time goes

### 3.1 Per job and per test binary (measured)

Run 43464769007, Windows, 4 vCPU. "Compile" is `Finished test profile` of the job's build step.

| Job | Job minutes | Compile | Test binaries (libtest "finished in") |
| --- | ---: | ---: | --- |
| typed shader family | 45.2 | 3 m 47 s | `typed_shader` 2,454 s (249 tests) |
| typed geometry and pathing | 23.1 | 3 m 34 s | geometry 578 s (32), pathing 556 s (172) |
| typed scene queries | 19.7 | 2 m 55 s | collision 115 s (9), culling 424 s (32), dispatch 429 s (110) |
| typed draw, render, lights, particles | 18.4 (32.6 in run 2) | 3 m 03 s | draw 536 s (46), lights 27, multibound 97, particles 141, render 86 |
| typed animation | 11.5 | 3 m 48 s | anim_keys 131 s (7), animation 293 s (11) |
| typed runtime | 10.2 | 2 m 54 s | crt 190 s (65), leaves 45, lockfree 56 (87), process 107 (8) |
| aggregator (engine lib, host, play-patch, viewer) | 7.5 | 3 m 46 s plus 2 m 58 s | 3 s of tests; the two compiles are 6.7 min |
| fallout-world | 8.1 | 5 m 07 s | 5,288 tests in 28 s, 5,177 in 86 s |
| fallout-gameplay | 6.7 | 4 m 40 s | 6,207 tests in 31 s |
| engine-base | 6.3 | 4 m 38 s | 12,530 tests in 22 s |
| Clippy (workspace) | 5.2 to 7.2 | check only | none |

Four things stand out.

- The unit tests are not the problem. 57 k tests of the fallout and engine crates run in minutes, about 5 ms
  each, and these jobs are 60 to 85 % compile (measured: world 5:07 of 8.1 min, gameplay 4:40 of 6.7, engine-base 4:38 of 6.3) (the three fallout components each compile the same
  18-crate, 3.98 M-line closure, `CI.md`).
- The seventeen `typed_*` binaries are about 6,270 test seconds in all (shader 2,454 of them), and each of the
  seven integration jobs compiles the `engine` closure again (about 3 to 4 minutes each, about 26 job-minutes
  in all).
- Run-to-run spread is large on some jobs (draw: 18.4 against 32.6 min; the earlier study measured 25 to 29 %
  on single binaries) and absent on others (shader: 45.2 against 46.7). The estimates below carry a factor
  of 1.0 to 1.3 for that.
- Test binaries inside one job run one after the other, each on four threads. A job such as geometry and
  pathing is two binaries in series, so its tail leaves cores idle.

### 3.2 The shader binary, test by test (measured, reconstructed)

`typed_shader` ran 249 tests in 2,454 s. libtest uses 4 threads, so the binary used about 9,800 CPU-seconds.
Tests that ran over 60 s are named in the log with the time of the warning and of the finish; duration is
60 s plus the difference. 36 tests qualify and sum to about 7,200 s (73 % of the CPU-seconds). The slowest:

| Test | About (s) | What it covers |
| --- | ---: | --- |
| `select_pass` | 820 | pure choice over every pass type (0x2f0 values) times 5 ways: 3,760 cases |
| `setup_geometry_constants_other_types` | 550 | the shadow light shader's constants set-up |
| `lights_by_count_and_mode` | 520 | lights by count and mode |
| `setup_type_102`, `setup_type_d1`, `setup_type_ca` | 510, 435, 400 | the shadow-pass type set-ups |
| `setup_geometry_constants` | 455 | as above |
| `lighting30_setup_constants` | 445 | lighting set-up |
| `simple_shadow_clip_planes_made_or_kept`, `render_batches`, `lights` | 270, 260, 207 | |
| `setup_geometry_textures_every_type` | 154 | |

The first eight tests are about 4,100 s, 42 % of the binary's CPU-seconds. A job that splits the binary has a floor of its
longest test: 820 s, 13.7 minutes.

### 3.3 Why a test costs this much (read from `typed/diff.rs`)

`Diff::run` runs every case once per control word, and each run is **three engines**: the translation, the typed
version called by address, and the typed version with `typed_call!` going direct (the third only with debug
assertions). Defaults: `max_cases` 512, plus `float_sweep` cases, times two control words (`Threads::Both`, the
default of the shader family; only 6 of 249 shader tests override the words). The shader tests set `max_cases`
from 64 to 3,760. So a typical test is 560 cases x 2 words x 3 engines = 3,400 engine builds; `select_pass` is
about 22,000.

Each `Diff::side` does `Engine::new_untyped()` (a `Vec` of 47,733 `(address, fn)` pairs gathered from every
crate's `funcs`, then a hash map insert per function with a duplicate check, then a fresh memory model with a
512 KB call cache), registers the family, calls the test's `setup` (the shadow-light world is hundreds of
`e.mem.set_*` calls), runs the call and compares every mapped page, the calls, the heap and the control word.
All at opt-level 0. A shader test needs 2,454 s x 4 / 249 = 39 CPU-seconds on average; at 3,400 builds that
is about 11 ms per engine side, `select_pass` is about 37 ms per side. (The averages are measured; that table
building is the largest part of it is a reading of the code, **estimated, not measured**.)

### 3.4 Classification of the slow tests

| Class | Tests | Evidence | What to do |
| --- | --- | --- | --- |
| Golden-oracle candidate: translation side is identical on every run of a typed-only change | all `typed_*` | the translation run (one of the three engines) depends only on translation crates; the typed runs depend on `engine-typed` | record the translation's observable result per case (a digest of return, calls, memory) once, compare typed runs against it; invalidate by the source hash of the translation crates it executed. Saves at most one engine in three, and only when no translation crate changed |
| Redundant recomputation: second control word on integer-only code | the 84 `float_sweep(0)` shader tests, `select_pass`, set-up choosers | a function that never touches the x87 state gives identical results under both words, yet runs twice | run the second word only when the first run executed an x87 instruction or read the word. Halves those tests |
| Redundant recomputation: engine and table rebuilt per case | every typed test | 3.3 | build the function table once per thread and clone it for each engine |
| Exhaustive table where classes would do | `select_pass` (3,760 cases of a pure function of two small inputs), the `*_every_type` tests | `max_cases(0x2f0 * 5)` | keep exhaustive in a nightly or weekly run; run boundary classes (a few dozen) on pull requests. Upstream's decision; offer it |
| Genuine integration | the shadow set-ups on the full world (`setup_type_*`, `lighting30_*`) | they run real translated code through callee chains | keep, shard |
| Unit tests proper | the 57 k tests of the crates | 5 ms each | nothing to do |

## 4. Fuzz targets

`fuzz/` is a stand-alone workspace (so `libfuzzer-sys` stays out of the root workspace) with three targets,
`esm_plugin`, `bsa_archive`, `fos_save`; `fork-fuzz.yml` runs each for 10 minutes weekly on Linux with nightly
`cargo-fuzz`; the three targets only assert that arbitrary input never panics. The skeleton is sound; there is
no seed corpus (corpora must not be committed, `nvinspect` rejects binaries), and no step turns a finding into a
checked-in regression test.

Entry points read from the sources that qualify and are not fuzzed yet:

| Crate | Entry point | Why it qualifies |
| --- | --- | --- |
| `esm` | `inflate::zlib_decompress`, `parse_plugins_txt`, `parse_subrecords` | the shared decoder under plugins, archives and the launcher; no dependencies |
| `bink` | `Movie::parse`, `decode`, `decode_packet` (2.7 k lines) | video and audio from untrusted archives |
| `mp3` | `decode_all` (5.1 k lines) | bit reader and Huffman tables |
| `vorbis` | `decode_all`, `VorbisFile::open` (11.5 k lines, the largest decoder) | return codes, not `Result`: assert only no panic |
| `d3dx-tex` | `parse`, `decode_dxt3/5` (5 k lines) | texture containers |
| `d3d9-wgpu` | `sm::package::Package::parse`, `sm::tokens`, `sm::parse`, `convert` | shader bytecode to WGSL; `naga` validates the output, which is a free oracle (`validate`). Needs wgpu in the fuzz build |
| `d3d9-stream` | `wire::commands`, `wire::replay` | recorded call stream |
| `nvse` | `cosave::Reader::open` and its readers | the extender's save sidecar |
| `fos` | already covered; add the decoded parts (`decode`) | |

There is no NIF reader as a stand-alone parser: the `NiStream` functions are translations of the exe, executed
by the engine and not parsers with a byte-slice entry point. What can be fuzzed there is the translation behind
the typed seam, which is a differential test, not a fuzz target.

Cost: each target is one file of ten lines and one `[[bin]]` entry; the matrix of `fork-fuzz.yml` grows by one
line each. Compile cost is small (these crates have no dependencies), except `d3d9-wgpu` (wgpu, naga: a few minutes
per weekly job). A differential fuzz target (decode with two paths and compare, as `d3dx-tex` has with
`software-truncation`) would find more than a no-panic target, at higher effort.

## 5. What a change forces to recompile and to retest

From the workspace graph (36 crates, 4.25 M lines; normal dependencies, test closures include dev-dependencies):

| Crate | Own lines | Crates that rebuild | Lines that rebuild |
| --- | ---: | ---: | ---: |
| `engine-core` | 112 k | 24 | 4.07 M |
| `engine-gamebryo` | 421 k | 18 | 3.40 M |
| `engine-havok`, `-bs`, `-speedtree` | 913 k, 333 k, 59 k | 16 | 2.15 M |
| `engine-fallout-facegen` | 57 k | 11 (15 for tests) | 1.51 M |
| `engine-fallout-cell` | 308 k | 8 (15 for tests) | 1.03 M |
| `engine-typed` | 46 k | 4 | 18 k |
| `d3d9-wgpu` | 19 k | 5 | 70 k |

The last 400 upstream commits that touch code (352): 155 touch one crate and 91 two. Single-crate commits
by crate: `engine` 56, `d3d9-wgpu` 33, `engine-core` 15, `engine-platform` 12, `nvse` 8, `jip` 6. 105 of the 400
touch `engine-core` (88 of them `engine-core/src`: `units/` 45, `mem.rs` 24, `engine.rs` 23, `typed/` 19). 28
(8 % of code commits) touch only `engine-typed` and the typed tests, mostly one to three families.

**Selection by what the typed tests execute (estimated).** Every typed job runs for any change in the crate
closure of `engine`, and `engine-platform` is in it, so a `d3d9-wgpu` change (33 single-crate commits) reaches
all seven integration jobs although the typed tests never run the renderer. If the typed jobs ran only when a
crate they execute changed (`engine-core`, `engine-typed`, `engine`, `gamebryo`, `bs`, `havok`, `speedtree`,
the fallout crates; not `d3d9-*`, `d3dx-tex`, the readers and codecs, `nvse`, `jip`, `host`, `play-patch`,
`nv-viewer`, `engine-platform`, `frame-model`, `nvinspect`), 115 of the 352 code commits (33 %) would skip all
six typed jobs, about 130 job-minutes each (the six typed jobs of run 43464769007). That count uses a coarse list of crates, not a recorded footprint, so
it is an upper bound; the footprint (which crates' translations a family really executes) has to be recorded once
by a run with the call trace on (`call_trace.rs`) before the rule is trusted. A rule that is wrong skips a test
that would have failed, so the nightly and push runs stay complete, as today.

**A typed family test needs a smaller assembly (estimated).** `engine-typed` depends only on `engine-core`, and
`engine-core/tests/typed_seam.rs` already assembles a function table by hand (`assemble!()`). The typed tests
sit in the `engine` crate only because it is the crate that assembles all parts. The addresses a test names
(counted in the test text, crate by crate) are a mix: `typed_shader` names 728 addresses, 385 not translations
(stand-ins), 213 in `engine-bs`, 52 in `engine-gamebryo`, and about 50 more in `engine-fallout-*`; `typed_crt`
names 68 in `engine-core` and 2 elsewhere; `typed_pathing` 140 in `engine-fallout-pathfinding`; `typed_anim_keys`
7 in `engine-gamebryo`. So a few families can be tested in a crate that links `engine-core`, `engine-typed` and
one or two parts: for shader that is `engine-bs` and `engine-gamebryo` (0.87 M lines, 20 % of the workspace) plus
whatever else the proof shows is reached. The proof is a run: build the test with the reduced assembly and look
for stops at an unregistered address ("open function"). A family whose callees stay inside the reduced set
compiles in a fraction of the time and builds a table of a few thousand functions instead of 47,733.

**Extraction that follows from the numbers:**

| Candidate | What | Why | Cost |
| --- | --- | --- | --- |
| `engine-core::typed::diff` (4.5 k lines, plus `leaf`, `moved`) into a dev-only crate | the differential harness | 19 of 88 `engine-core` commits touch `typed/`, 9 only `typed/`: each rebuilds 24 crates (4.07 M lines) for a change that no library uses | moves public paths (`engine_core::typed::diff`) in 17 test files and `test-support` users: upstream churn, medium |
| `engine-core::units::crt` and `::platform` (69 k of 112 k lines) | the C runtime and platform units | 45 of 88 `engine-core` commits touch `units/`, so a CRT fix rebuilds the workspace | 12 crates re-export them (`pub use engine_core::units::crt`); Phase B-sized change. Only worth it if the extracted crate has no dependency from the parts |
| One test crate per typed family group | the reduced assembly above | compile, table size and selection at once | new crates in upstream's layout; offerable only as a whole |

## 6. Ranked proposals

Minutes are the change to the critical path unless stated (the pipeline's wall-clock for a pull request that
reaches everything); "job-min" is runner time. Divergence: **none** = fork-only files (workflow, `.fork/`,
`scripts/fork/`, `.cargo/` of the fork); **offer** = a change in an upstream file, small and separable, worth
offering upstream; **big** = changes upstream's layout.

| # | Proposal | Saves (critical path) | Effort | Divergence | Basis |
| ---: | --- | --- | --- | --- | --- |
| 1 | **Shard the heavy typed binaries by measured test time.** Per-test times from one run with `-Zunstable-options --report-time` (stable toolchain with the bootstrap override), kept in `.fork/test-timings-per-test.json`; a script packs tests greedily into N jobs; each job runs `cargo test -p engine --test X -- --exact <names>` (the filter syntax libtest has). Shader into 3 jobs, geometry/pathing and scene queries into 2 each | shader 45 to about 19 min (3:47 compile + 13.7 floor + 1 setup); pipeline 47 to about 19 to 24 (geometry 23 and scene 20 become about 13 to 14) | small to medium: `components.toml` gains `shards`, one script, one summary line | none | Estimated from the measured CPU-seconds (9,800 / 4 threads / 3 shards = 13.6 min, floor 820 s). Costs about +11 job-min of compile |
| 2 | **Reuse the function table between engines**: build the `(address, fn)` list and the map once per thread and clone it in `Engine::new_untyped` | 15 to 40 % of test CPU, 6 to 16 min of the shader binary (before shards) | small (about 15 lines in `engine-core/src/engine.rs`) | offer | Estimated; confirm with Section 7's measurement (one test, a few minutes) before building |
| 3 | **Run the second control word only if the first run touched the x87 state** (a flag set by the x87 helpers and `FSTCW`), in `Diff::run` | up to half of the 84+ integer-only shader tests, `select_pass` 820 to about 410 s; shader binary 20 to 40 % | small to medium: a touched flag in `x87.rs`, a branch in `run` | offer | Estimated; must keep one test that proves the flag fires (a function that does read the word) |
| 4 | **opt-level 1 for `engine-core`, `engine-typed` and `engine`** in the integration jobs: `cargo test --config profile.dev.package.engine-core.opt-level=1` (and the other two). Debug assertions and overflow checks stay on, so the third run (direct) still happens | unknown; the earlier study estimated 3 to 10 times on the hot code; compile grows | small (flags in `run-component.sh`) | none | **Unmeasured.** One manual run answers it; it can be combined with the measurement of Section 7. Ranked below 2 and 3 only because it may fail to help (translations in the other crates stay at opt 0) |
| 5 | **Select typed jobs by the crates the tests execute**, not by `engine`'s closure (Section 5): footprint file, `pr_paths` per job in `components.toml` | 33 % of code commits skip about 130 job-min (upper bound); no wall-clock change for the rest | medium: a footprint run, a rule in `components.py`, tests | none | Estimated from 352 commits with a coarse crate list |
| 6 | **Build once, test many**: compile the integration jobs' test binaries in one job, upload them, run the shards from the artifact (`cargo nextest archive` or the binaries and a manifest) | about 20 job-min (the 6 repeated engine compiles), 3 min of each non-critical job; with proposal 1 it also takes the 3:47 compile off each shard | medium to large: artifact size (opt-0 test binaries linking 4 M lines; unmeasured), Windows paths, `--exact` over an archive | none | The size is unknown; measure the `target/debug/deps/typed_*.exe` sizes first |
| 7 | **Merge the three fallout components' compile**: one job, one `-p` list of the 12 crates (they share the 18-crate closure) | 14 job-min (three 5-minute compiles become one); wall-clock about equal | small | none | Measured compile times for world (5:07) and gameplay (4:40); front's is estimated at about the same (7.6 min job) |
| 8 | **Boundary classes on pull requests, the exhaustive table nightly** for tests like `select_pass` | `select_pass` 820 to about 20 s; the shader floor drops to the next test (about 550 s) | medium: a `Diff::thorough` switch and a choice per test | offer | Estimated; the decision about what coverage a pull request needs is upstream's |
| 9 | **Golden oracle for the translation side**: digest per case, keyed by a hash of the translation sources it executes | at most one engine in three, on typed-only changes (8 % of code commits) | large: a store, an invalidation rule, a risk of a stale oracle hiding a regression | big | Lowest rank: small gain, new failure mode. Revisit once 2 to 5 are in |
| 10 | **A reduced assembly per typed family** (Section 5) | per family 1 to 2.5 min of compile and a table 10 times smaller (so part of 2); selection gets exact | large: new crates, a proof per family | big | Only after 2 and 5 show how much is left |
| 11 | **Fuzz**: the nine targets of Section 4, a seed step that does not commit binaries (a generator in the fuzz crate), a rule that a finding becomes a unit test | none (weekly job) | small per target | none (fuzz is fork-owned) | Safety, not minutes |

Expected order of effect on a pull request that reaches everything (estimated): proposal 1 alone takes about
47 to about 19 to 24 minutes. Proposals 2, 3 and 4 shrink the CPU behind each shard; with 1 they let the shard
count fall from 3 to 2 or the floor reach about 8 to 10 minutes for the shader family. Nothing else in the
list moves the wall-clock; 5, 6 and 7 move cost, 11 moves safety.

What is offerable upstream (each as its own small pull request, no mention of the fork's CI): 2, 3, 8,
and, if the measurement supports it, an `opt-level` for the test profile of `engine-core` and `engine-typed`
(upstream's `Cargo.toml`). Proposals 1, 4 to 7 and 11 stay in the fork.

## 7. The one measurement that decides 2, 3 and 4

No new CI run was made for this document. The three proposals that change the cost of a test depend on one
unknown: how much of a case is engine construction and the second control word, and how much is the function
under test. One test answers it. In a throwaway branch of the fork (not merged), add to
`crates/engine/tests/` an ignored test that:

1. times `Engine::new_untyped_for_test()` 200 times (it exists for tests), prints microseconds per engine;
2. times 200 runs of `select_pass`'s single case path with one control word and with both;
3. is run in the existing `integration` aggregator job with `--ignored --nocapture`.

Reading it: if one engine costs more than half of the per-side 11 to 37 ms, proposal 2 is the first change; if
the table is cheap and the call is the cost, 3 and 4 are. The same branch can run `typed_culling` (558 s warm)
with `--config profile.dev.package.engine-core.opt-level=1` to answer proposal 4, and the per-test times of
proposal 1 come from running one binary with `-Zunstable-options --report-time` once.

## 8. Acceptance for any of these

Whatever lands, the test count must stay what `CI.md` says: the component summaries together equal
`cargo test --workspace` (57,444 passed, 53 ignored). A shard list that drops a test name must fail the plan
step the way an unlisted test file fails `components.py check`. Nothing here stores game data: per-test
timing files hold test names and seconds only.
