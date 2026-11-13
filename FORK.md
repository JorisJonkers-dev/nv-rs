# About this fork

This repository is a fork of [slaterain/nv-rs](https://github.com/slaterain/nv-rs), the Rust port of
Fallout: New Vegas, kept by [JorisJonkers-dev](https://github.com/JorisJonkers-dev). The code, the
README, [CONTRIBUTING.md](CONTRIBUTING.md), the [LICENSE](LICENSE) (GPL-3.0) and
[LEGAL.md](LEGAL.md) are upstream's. We keep them and follow them; the fork does not change what the
project is or how it treats the original game's data.

## Why it exists

- To run the way this organisation works on a codebase we want to contribute to: a required check on
  every merge, Conventional Commits, release-please, secret scanning, dependency cooldown, fuzzing,
  Linux builds. Upstream has not asked for any of this, and we did not want to open issues or
  pull requests asking it to change before we had shown it working.
- To work on running nv-rs on Linux (Proton, then native) and report what we find.
- To have a place to try changes first, and to offer the good ones back to upstream, one small pull
  request at a time, the way its [CONTRIBUTING.md](CONTRIBUTING.md) asks.

## What the fork adds

Everything the fork adds is listed in [`.fork/overlay-paths`](.fork/overlay-paths) and nothing else
differs from upstream, apart from the notice at the top of the README
([`.fork/allowed-divergence`](.fork/allowed-divergence)); `scripts/fork/verify-sync.sh` checks it. Mainly: `Taskfile.yml` and `mise.toml` (the local checks), `deny.toml`,
`.gitleaks.toml`, `fuzz/`, `renovate.json`, `release-please-config.json`, the `fork-*.yml` and
`release.yml` workflows, `.github/rulesets/`, and `scripts/fork/`.

## Branches

| Branch | What it is |
| --- | --- |
| `fork` | **the default branch**: upstream's history, replayed commit by commit with the same changes and Conventional Commit messages, plus the fork's own commits. Protected: pull requests only (squash for people, rebase for the sync), the `Pipeline Complete` check must pass. Never rewritten. |
| `main` | an exact copy of `slaterain/nv-rs` `main`, with upstream's own commit ids, kept by GitHub's own sync (Sync fork is safe here). Nothing else writes it. Topic branches meant for upstream start here. |
| `sync/upstream` | the open sync pull request. |
| `<type>/<topic>` | work on the fork, for example `feat/linux-vulkan`. |

## Reading on

- [docs/fork/KEEPING-IN-SYNC.md](docs/fork/KEEPING-IN-SYNC.md): how the fork follows upstream, and what to do when a sync stops.
- [docs/fork/CONTRIBUTING-UPSTREAM.md](docs/fork/CONTRIBUTING-UPSTREAM.md): how an improvement gets back to upstream.
- [docs/fork/PRACTICES.md](docs/fork/PRACTICES.md): the checks, the release flow and the hooks the fork adds.
