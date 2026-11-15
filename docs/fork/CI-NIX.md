# Nix in the fork's Linux CI

Fork-owned. Evidence from pull request 42 (run of 2026-11-15) and from the pinned `flake.lock`; the plan is at the end.

## Why

The runners' apt `libvkd3d-shader1` is 1.2. Compiling HLSL to d3dbc needs a recent vkd3d-shader (local tests used 1.19), so
the shader compiler tests (`crates/d3dx-shader`, engine-platform's `d3dxshader`) printed `skipped:` in CI and proved nothing.

## Answers

**Can the Linux jobs run inside Nix?** Yes. `flake.nix` has a `ci` dev shell (`nix develop .#ci --command ...`) with the
pinned compiler (rust-overlay, `rust-toolchain.toml`), `pkg-config`, libvkd3d-shader, Mesa's lavapipe, the Vulkan loader,
Xvfb and `vulkaninfo`. It exports `NV_VKD3D_SHADER`, `NV_REQUIRE_VKD3D=1`, `VK_ICD_FILENAMES`, `LD_LIBRARY_PATH`,
`NV_WGPU_BACKEND=software` and `RUST_TEST_NOCAPTURE=1`. The Linux `rendering` and `platform` jobs now run
`scripts/fork/run-component.sh` inside it. The rest of the build is not moved yet.

**Why the whole job, not only the library.** Nix's libvkd3d-shader needs glibc 2.43 symbols (`objdump -T`: `GLIBC_2.43`);
the runner has 2.39. A test binary linked by the runner's compiler loads the runner's libc, so `dlopen` of the Nix
library fails. The tests must be built and run with the Nix compiler and libc. The same holds for lavapipe and the Vulkan loader
(everything `dlopen`ed shares one libc), so the Xvfb window smoke, the lavapipe GPU tests and the vkd3d tests belong in this one shell.

**Which action installs Nix.** None. The repository allows only GitHub-owned and a few listed actions, so
`install-nix-action` (cachix) and `magic-nix-cache` (Determinate Systems) are not allowed (this is why pull request 41 exists).
The jobs use the official installer (`curl https://nixos.org/nix/install | sh -s -- --no-daemon`), the same step as
`fork-release-build.yml`. Not pinned by hash: the installer script is fetched from nixos.org over TLS; the Nix it installs
is the runner's default, and every package comes from the pinned nixpkgs. Residual trust: nixos.org and cache.nixos.org (narinfo
signatures are checked against the key Nix ships).

**Binary cache.** Only cache.nixos.org today; no extra secret, `permissions: contents: read`.
- Cachix: free tier for open source projects, reading a public cache needs no token but pushing does (a repository secret, which
  the estate does not use for the package proxy, see `fuzz.yml` and nvmod-rs workflows); the client is open source (Apache-2.0),
  the service is proprietary. Not acceptable now: an action not on the allow list and a new secret.
- magic-nix-cache: Determinate Systems' action, not on the allow list, and it talks to a Determinate service (trust).
- `actions/cache` (GitHub-owned, allowed): feasible by saving the closure with `nix copy` to a file store and restoring it keyed on
  `flake.lock`; no new trust or secret. Planned (phase 1).

**Cold and warm times** (rendering, cold means no cache of the Nix closure or of the `-nix` cargo cache; Linux jobs are 2 vCPU):

| | before (apt, rustup) | with Nix, cold |
| --- | --- | --- |
| rendering Linux, whole job | 1 m 10 s (rustup 8 s, lavapipe apt 16 s, vkd3d apt 4 s, tests 32 s) | 2 m 02 s (Nix install 19 s, shell realised 38 s, tests 49 s) |
| platform Linux (new twin) | n/a | Nix install 8 s, shell 28 s |

Extra cold cost about 50 s per Nix job (mostly downloading the shell closure, mesa and LLVM among it); the tests themselves
were slower (49 s against 32 s) because the `-nix` cargo cache had not been saved yet (it saves on the fork branch).
Warm time is not measured yet; without a Nix store cache the 38 s of shell realisation recurs every run (phase 1).

**Which jobs benefit.** `rendering`: the vkd3d tests (7 executed, none skipped, `vkd3d-shader 2.1` is nixpkgs' version
name, the library is 1.19.x) and the lavapipe tests (47 executed on llvmpipe, Mesa 26.3.0), from one shell instead of two apt steps.
`platform`: engine-platform's `d3dxshader` test now executes (it never ran on Linux: the component ran on Windows only); the
component is a new required Linux twin; 5 other engine-platform tests fail on Linux and are skipped by name in
`.fork/linux-skip.txt` (nv-linux #205) (`every_import_is_answered_by_one_function`,
three `files::tests_code_pages` and `files::tests_player_files::a_read_only_open_reads_the_players_file_in_place`, found by this run, ). `scripting` and the others gain nothing from Nix today beyond a pinned compiler. The Xvfb window smoke
(pull request 40) needs the same shell: `xvfb-run` is in it already.

**Failing loudly.** `NV_REQUIRE_VKD3D=1` (already honoured by the tests) makes a library that cannot be loaded a test failure, and
`scripts/fork/vkd3d_gate.py` fails the job when any vkd3d test printed `skipped:` or none printed `executed:`, as
`gpu_gate.py` does for the GPU tests.

**Pinning.** `flake.lock` pins nixpkgs (`d4f25a3e562008648d45bcf2d70bcc7422cad84e`, 2026-11-15: vkd3d `2.1` packaging of
libvkd3d-shader 1.19.0, mesa 26.3.0, glibc 2.43) and rust-overlay; the compiler comes from `rust-toolchain.toml`. Bumps are `nix flake update` in
a pull request of their own.

**Windows.** Stays on native Windows runners (the game is a Windows program, the Windows jobs verify upstream's gate and the
`cfg(windows)` code). Only Linux jobs move.

## Plan (issues in JorisJonkers-dev/nv-linux, prefix `nv-rs linux:`)

1. Done here: `ci` shell for `rendering` and `platform`, gate, strict env.
2. Cache the Nix store with `actions/cache` (nv-linux 201).
3. All Linux component jobs in the shell, no apt or rustup in them (nv-linux 202); fix the 5 skipped platform tests (nv-linux #205).
4. Xvfb window smoke, lavapipe and vkd3d in one job log with a gate each (nv-linux 203).
5. `nix flake check` as the Linux gate (fmt, clippy, tests) and automatic `flake.lock` bumps (nv-linux 204).
