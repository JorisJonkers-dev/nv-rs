# What the fork adds

| Practice | Where | Notes |
| --- | --- | --- |
| One required check, `Pipeline Complete` | `.github/workflows/fork-ci.yml`, `.github/rulesets/main.json` | One job; the checks are steps (`task check`). The two reusable workflows (hygiene, PR title) are jobs it waits for |
| Branch protection | `.github/rulesets/main.json`, `scripts/fork/apply-ruleset.sh` | The estate's ruleset: no deletion, no force push, linear history, pull requests only, squash for people and rebase for the sync, `Pipeline Complete` required. No bypass actors |
| Conventional Commits | `pr-title` job, `scripts/fork/hooks/commit-msg` | The pull request title is the squash commit, so it is what release-please reads |
| Following upstream | `scripts/fork/replay-upstream.sh`, `.github/workflows/fork-sync.yml`, `scripts/fork/verify-sync.sh` | Replays each upstream commit with the same change and a Conventional Commit message; see KEEPING-IN-SYNC.md |
| Release notes and tags | `release-please-config.json`, `.release-please-manifest.json`, `.github/workflows/release.yml` | `release-type: simple`: it never edits `Cargo.toml` or `Cargo.lock`, so `distribution/DEPENDENCY-LOCK.sha256` stays valid |
| Secret scanning | `.gitleaks.toml`, `task secrets` | Every commit, in CI and locally |
| Workflow lint and security | `actionlint`, `zizmor` in `task lint:actions` | |
| Licences, bans, sources, advisories | `deny.toml`, `task deny` | |
| Dependency cooldown | `.cargo/config.toml`, `renovate.json` | Cargo: 7 days, stable from 1.100 (inert while upstream pins 1.99.1). Renovate: the estate preset's `minimumReleaseAge` |
| Dependency updates | `renovate.json` | Only fork-owned files |
| Fuzzing | `fuzz/`, `.github/workflows/fork-fuzz.yml` | Weekly, three parsers; needs `cargo +nightly` |
| Linux-first build | `.github/workflows/fork-build-linux.yml`, `fork-build-windows.yml`, `.fork/linux-ok.txt`, `scripts/fork/linux_probe.py` | Everything that can run on Linux does; Windows only for what needs it. A probe reports which crates build on Linux; crates listed in `linux-ok.txt` are required there |
| Windows and Linux release builds | `.github/workflows/fork-release-build.yml` | Artefacts only; never publishes. Packaging stays local (needs the player's `FalloutNV.exe`) |
| Local setup | `mise.toml`, `Taskfile.yml`, `scripts/fork/install-hooks.sh` | `mise install`, then `task check` |

Not adopted on purpose: Dependabot (the estate uses Renovate), and a changed Rust toolchain
(upstream pins one in `rust-toolchain.toml`).
