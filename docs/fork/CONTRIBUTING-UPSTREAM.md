# Getting an improvement back to upstream

Upstream's rules are in its own [CONTRIBUTING.md](../../CONTRIBUTING.md) and [AGENTS.md](../../AGENTS.md).
They win over anything written here. The points that shape the fork's flow:

- Work from the latest upstream `main`, on a topic branch, from a fork. **This fork is that fork.**
- **Open an issue first and wait for the maintainer's go-ahead** before starting, then comment
  `Claiming this` with the branch name.
- Trace every behaviour in the original game; no invented behaviour. Never commit game data, raw
  decompiler exports or fnv_decomp's C++ ([LEGAL.md](../../LEGAL.md)).
- Small pull requests that pass upstream's checks. The maintainer decides what is accepted.

Opening an issue or a pull request on `slaterain/nv-rs` is an outward action. It is only done when the
fork's owner has decided to, for that one change.

## The flow

1. **Sync first.** Let [the sync](KEEPING-IN-SYNC.md) run so `upstream-main` (an exact mirror of upstream's `main`, with upstream's own commit ids) is current.
2. **Branch from the mirror, not from `main`:**
   `git switch -c fix/<topic> upstream-main`. The fork's overlay (workflows, `deny.toml`, ...) must not
   be in a branch that goes upstream.
3. **Make the change with upstream's rules.** Run upstream's checks (`cargo fmt`, `cargo clippy`,
   `cargo test` as its CI does). The fork's own checks do not apply here.
4. **Commit messages** inside the topic branch may follow the fork's Conventional Commits hook; upstream
   squash-merges, so only the pull request title matters there, and upstream's titles are plain English
   sentences. Retitle accordingly.
5. **Try it on the fork first (optional).** Open a pull request from the topic branch into the fork's
   `main` to get a second look and CI. Merging it there is fine, but then the same change is on the
   fork's `main` before upstream has it: when upstream merges its own version, the replay of that
   commit either applies as an empty change or stops on a conflict, and you resolve it by taking
   upstream's. To avoid that, keep changes meant for upstream on their topic branch until upstream
   accepts them.
6. **Offer it upstream, when the owner has decided to.** The issue first, then, after the go-ahead, the
   pull request from the fork's topic branch to `slaterain/nv-rs` `main`:
   `gh pr create --repo slaterain/nv-rs --base main --head JorisJonkers-dev:<branch>`.
7. **After upstream merges it,** the next sync replays upstream's commit onto the fork's `main`. Delete
   the topic branch.
8. **If upstream declines it,** the change is dropped, or kept on the fork's `main` as a fork-only
   commit (a deliberate, reviewed choice: if it changes a file upstream owns, add that file to
   `.fork/allowed-divergence`, and expect to resolve a replay conflict whenever upstream touches the same
   lines).

## What goes upstream, and what stays here

| Goes upstream when asked for | Stays in the fork |
| --- | --- |
| Bug fixes and translation fixes, with the traced evidence | Required checks, Pipeline Complete, the ruleset |
| Linux and Proton reports, and fixes the maintainer wants | Renovate, release-please, Conventional Commits |
| Small, separate offers of things the maintainer might want (a `deny.toml`, a weekly fuzz run, a Linux CI job), one at a time, each with its own issue | The hooks, the Taskfile, mise |

The drafts for those offers are in the fork's owner's notes, not in this repository, and are used only
when the owner decides to.

## Reports without code

A Proton or Linux data point (what ran, what failed, the log lines) is useful without a pull request.
Upstream asked for such reports in its issues; posting one is the owner's call, per report.
