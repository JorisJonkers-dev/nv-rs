# Keeping the fork up to date with upstream

`slaterain/nv-rs` moves fast (dozens of merges a day). The fork follows it by **replaying**: every new
upstream commit is applied to the fork's own branch, `fork`, as one commit with **the same change**, and a
Conventional Commit message so that release-please can read it. Nothing is merged, nothing is
force-pushed, and `fork` only ever grows.

## The two branches

| Branch | Role | Who writes it |
| --- | --- | --- |
| `main` | an exact copy of `slaterain/nv-rs` `main`, with upstream's own commit ids. No ruleset, no fork-only commits | GitHub's own sync only (the Sync fork button, `gh repo sync JorisJonkers-dev/nv-rs -b main`, or the `Upstream Sync` workflow's first step). It is always a fast-forward |
| `fork` | **the default branch**: upstream's history replayed with Conventional Commit titles, plus our own commits. Protected | the replay's pull requests and ours |

`main` is where you click **Sync fork** (it is safe there, and the "behind" banner on `main` is true). The replay
reads `main` and writes `fork`. The banner on `fork` will always say "ahead/behind" because the replayed commits
have new ids: ignore it, `scripts/fork/verify-sync.sh` is the real check.

## What stays identical, and what does not

| | |
| --- | --- |
| The change in each commit | Identical: the commit is a `git cherry-pick` of upstream's, so its diff is upstream's diff |
| Author and author date | Upstream's. The committer is the sync bot (or whoever ran the script) |
| The message | Rewritten: `type(scope): subject`, the original body without coding-agent credits, `#123` turned into `slaterain/nv-rs#123` so it does not link to this repository, and the trailer `Upstream-Commit: <full sha>` |
| The commit id | Different (a new message makes a new id). The trailer is the link back |
| The files | Upstream's, plus the fork's own files (`.fork/overlay-paths`) and the README notice (`.fork/allowed-divergence`). `scripts/fork/verify-sync.sh` proves it |

## How it runs

`.github/workflows/fork-sync.yml` runs daily and by hand. It first fast-forwards `main` from upstream with GitHub's own sync, then runs `scripts/fork/replay-upstream.sh --push`,
which reads `main`, and opens (or refreshes) the pull request `sync/upstream` into `fork`, and turns on auto-merge by **rebase** once
`Pipeline Complete` passes. Rebase, not squash: squash would put 30 commits into one message and
release-please would see one `chore`. The repository ruleset allows both squash (people) and rebase (the
sync).

The script:

1. fetches `main` (the upstream copy) and `fork` from `origin`; it never talks to upstream itself;
2. finds where the last run stopped: the newest `Upstream-Commit:` trailer on `fork` (before the
   first run, the point where the fork was cut);
3. for each upstream commit since then, oldest first: cherry-picks it, asks for a title
   (`scripts/fork/rewrite_title.py`), commits it with upstream's author and date;
4. stops if a commit does not apply (see below);
5. with `--push`, pushes the branch `sync/upstream`.

By hand, from a clean clone: `scripts/fork/replay-upstream.sh`, read the log it prints, then `--push`.

### The titles

The title is a type and a subject: `fix(engine-fallout-ai): correct the pathing flag`. With the repository
secret `ANTHROPIC_API_KEY` a small model (Haiku, set as `FORK_TITLE_MODEL` in the workflow; a few cents a month) proposes it from the
commit's message and file list. Commit text is untrusted input, so the model has no tools, and its answer
is used only if it is one line that matches the same pattern as the `commit-msg` hook; otherwise a fixed
rule is used (`fix`, `add`, `refactor`, ... on the first word, `chore` otherwise). A title is written once,
when the commit is replayed, and never regenerated, so it cannot drift between runs. The reusable `PR
title` check then validates every commit in the pull request.

## When the replay stops

| Message | Meaning | What to do |
| --- | --- | --- |
| `upstream commit <sha> does not apply on top of the fork` | Upstream changed lines that a fork-only change also changed (most often the top of the README) | `git switch -c fix/replay origin/main`, `git cherry-pick <sha>`, resolve it, commit with a Conventional message that ends with `Upstream-Commit: <full sha>`, merge it by rebase, rerun. The sync continues after it |
| `upstream main no longer contains <sha>` | Upstream rewrote its history (the copy on `main` then cannot fast-forward either) | Stop. Read upstream's repository, then choose the fork point again by hand |
| `the commit ... was refused` | A `commit-msg` hook refused a rewritten message | Read the reason; nothing was pushed |
| `already in sync` | Nothing new upstream | Nothing to do |

## Checking that the fork has not diverged

`scripts/fork/verify-sync.sh` (also run by the sync workflow) compares the fork with upstream at the
commit last replayed. Every file upstream has must be identical in the fork, apart from the fork's own
files and the README notice. It prints how many commits the fork is behind and fails with the list of
files that differ. A fork-only change to an upstream file is therefore always a deliberate act: add the
file to `.fork/allowed-divergence`, or do not make the change.

## What does not follow upstream

- **Dependencies.** Renovate only updates fork-owned files ([`renovate.json`](../../renovate.json)).
  Upstream's `Cargo.toml` and `Cargo.lock` arrive through the replay.
- **Releases.** Upstream tags `preview-YYYY-MM-DD.N`. The fork's release-please makes `vX.Y.Z` tags from the
  replayed messages. They are separate version lines.
- **Upstream's CI.** `.github/workflows/ci.yml` arrives with the replay and is not required here. The fork's
  required check is `Pipeline Complete` from `fork-ci.yml`; its build is split by engine component ([`CI.md`](CI.md)), and
  a new upstream crate must be added to `.fork/components.toml` (or it fails the `Plan components` job).

## Setting up from nothing

```sh
git clone git@github.com:JorisJonkers-dev/nv-rs.git ~/Projects/JorisJonkers-dev/nv-rs
cd ~/Projects/JorisJonkers-dev/nv-rs
git remote add upstream https://github.com/slaterain/nv-rs.git
scripts/fork/install-hooks.sh     # commit-msg and pre-push hooks
mise install                      # task, actionlint, gitleaks, cargo-deny, zizmor
task check
```

Repository settings that the workflow needs: the secret `ANTHROPIC_API_KEY` (optional), the release App
secrets `RELEASE_APP_ID` and `RELEASE_APP_PRIVATE_KEY`, the label `allow-repository-hygiene-exception`,
auto-merge enabled, and the ruleset applied with `scripts/fork/apply-ruleset.sh`.

## If GitHub's sync of `main` is refused

The sync API is asked to fast-forward `main`. It can refuse if someone committed to `main` (nobody should: it has no
fork-only commits) or, possibly, when upstream changed workflow files and the token that asks may not push them (not
verified: the first scheduled run shows it). Then the workflow warns and the replay works from where `main` is. Fix it
from your own login: `gh repo sync JorisJonkers-dev/nv-rs -b main` (or the Sync fork button on `main`).
