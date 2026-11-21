# Keeping the fork up to date with upstream

`slaterain/nv-rs` moves fast (dozens of merges a day). The fork follows it by **replaying**: every new
upstream commit is applied to the fork's own branch, `fork`, as one commit with **the same change**, and a
Conventional Commit message so that release-please can read it. Nothing is merged, nothing is
force-pushed, and `fork` only ever grows.

Upstream is an imported source, not a partner: its commits are replayed with their author, date and
`Upstream-Commit` trailer (licence and attribution intact), and nothing is offered back. The fork may diverge
wherever it is better; the rules below say who wins when both sides touch the same thing.

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

`.github/workflows/fork-sync.yml` runs weekly (Mondays 05:00 UTC) and by hand; there is only ever one `sync/upstream` pull request, which each run refreshes. The run summary opens with how many upstream commits the fork is behind and the files and lines they change, by crate (`sync_report.py behind`). It first fast-forwards `main` from upstream with GitHub's own sync, then runs `scripts/fork/replay-upstream.sh --push`,
which reads `main`, and opens (or refreshes) the pull request `sync/upstream` into `fork`, and turns on auto-merge by **rebase** once
`Pipeline Complete` passes. Rebase, not squash: squash would put 30 commits into one message and
release-please would see one `chore`. The repository ruleset allows both squash (people) and rebase (the
sync).

A squash merge's message is the pull request's description, so a squashed replay keeps its replay point only when
the description ends with it. A sync was squash-merged without it (#376; #377 recorded the point
afterwards), and verify-sync, the next replay and the release build then read an older replay point. So every pull
request whose commits carry `Upstream-Commit:` trailers (the automatic sync and a hand-made one) must end its
description with the line `Upstream-Commit: <newest replayed sha>`: the Verify sync job checks it
(`scripts/fork/sync_trailer.py`, also on description edits), and the sync workflow writes it.

A pull request's checks run on GitHub's merge commit of the pull request into `fork`, made when the event came. If
`fork` moved shortly before (a replay-point commit, for example), that merge may still be on the older `fork`, and a
re-run reuses the same merge commit. Close and reopen the pull request (or push to it) once GitHub shows the new base
to get a run on the current merge.

The script:

1. fetches `main` (the upstream copy) and `fork` from `origin`; it never talks to upstream itself;
2. finds where the last run stopped: the newest `Upstream-Commit:` trailer on `fork` (before the
   first run, the point where the fork was cut);
3. for each upstream commit since then, oldest first: cherry-picks it, asks for a title
   (`scripts/fork/rewrite_title.py`), commits it with upstream's author and date;
4. leaves out the changes to paths the fork owns (see "Who wins") and stops if a commit does not apply (see below);
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

## Who wins

| Path | What the replay does |
| --- | --- |
| a path in `.fork/overlay-paths` (also one upstream happens to have) | the **fork wins**: the commit is replayed without its change to that path, and the path is listed in the sync summary and in the pull request |
| `.github/workflows/` (any file, new or changed) | the **fork wins**, see "Workflow files" |
| a file in `.fork/allowed-divergence` | upstream's change is replayed; if it touches the lines the fork changed, the replay **stops** (a person merges the two) |
| any other file | replayed; a conflict stops the replay |

A commit that adds a `mod x;` line to an overlay file upstream also has would arrive without that line (the fork wins) but with `x.rs`, and
the build breaks. The replay stops on it with reason `partial`: take the file out of `.fork/overlay-paths` and list it in
`.fork/allowed-divergence`, or apply the commit by hand.

Since nv-rs#438 the list is a folder: `.fork/allowed-divergence` is frozen history (a test fails on any change to it) and a new entry is one file under `.fork/allowed-divergence.d/`, so two pull requests that each add one do not conflict. Wherever this document says `.fork/allowed-divergence`, read the old list and that folder together.

The divergence list is meant to grow. A file the fork wants to change for good goes into `.fork/allowed-divergence` with a
reason; nothing needs to be accepted anywhere else.

## Workflow files

The fork does not use upstream's CI (too slow; its own `Pipeline` is split by component and keeps both Windows and Linux
jobs, with room for macOS). `.github/workflows/` is therefore fork-owned: an upstream commit that changes it is
replayed **without** those changes (the rest of the commit applies, and the commit message says what was left out), and the
left-out paths appear in the run summary and the pull request. A change to upstream's `ci.yml` also gets the
`ci-parity-review` label, so that a person can mirror something worth having into `fork-build.yml`. This needs no
`workflows` permission for the sync App, and `verify-sync.sh` leaves `.github/workflows/` out of its comparison.
`WORKFLOW_POLICY=apply` (or `ALLOW_WORKFLOW_CHANGES=1`) on a manual run replays them too.

## When the replay stops

| Message | Meaning | What to do |
| --- | --- | --- |
| `upstream commit <sha> does not apply on top of the fork` | Upstream changed lines that a fork-only change also changed (most often the top of the README) | `git switch -c fix/replay origin/main`, `git cherry-pick <sha>`, resolve it, commit with a Conventional message that ends with `Upstream-Commit: <full sha>`, merge it by rebase, rerun. The sync continues after it |
| `upstream main no longer contains <sha>` | Upstream rewrote its history (the copy on `main` then cannot fast-forward either) | Stop. Read upstream's repository, then choose the fork point again by hand |
| `the commit ... was refused` | A `commit-msg` hook refused a rewritten message | Read the reason; nothing was pushed |
| `already in sync` | Nothing new upstream | Nothing to do |

### The tracking issue

A run of `Upstream Sync` that stops or fails opens (or updates) **one** issue labelled `sync-blocked`, and the next
successful run closes it; a green run, or "already in sync", opens nothing. The issue names the upstream commit where the
replay stopped and its subject, the reason (one of the rows above, or `diverged` when `verify-sync.sh` failed, or `other`),
the run, and for a conflict the files from `git merge-tree`, each classified by `scripts/fork/sync_report.py`: (a) listed in
`.fork/allowed-divergence` (merge by hand, the fork's change stays), (b) listed in `.fork/overlay-paths` (the fork owns it:
nothing to do), (c) anything else (stop and read). The issue uses the job's own token (`issues: write`).

**When the repository has Issues disabled** (the nv-rs fork does; enabling them is the owner's choice and nothing here
needs it), `sync_report.py` asks `gh api repos/{owner}/{repo} --jq .has_issues` first and degrades instead of crashing:
`open` writes the same report to the job's step summary and prints a `::warning::`, `close` prints the warning and
exits 0, so a successful sync stays green. A blocked sync still fails the run: the step that stopped (the replay, the
divergence check) is what turns it red and sends the notification, and the report is read in the run's summary. The
same fallback applies when the issue API errors for another reason. With Issues enabled nothing changes.

## Checking that the fork has not diverged

`scripts/fork/verify-sync.sh` compares the fork with upstream at the commit last replayed. Every file upstream has must be
identical in the fork, apart from the fork's own files, `.github/workflows/` and the files in
`.fork/allowed-divergence`. It prints how many commits the fork is behind and fails with the list of files that differ.
It runs in the **Pipeline** (job `Verify sync`, a need of `Pipeline Complete`, on every pull request and push, also when
the builds are skipped), so a change to an upstream-owned file is caught in the pull request that makes it, and at the end
of the sync workflow. A fork-only change to an upstream file is therefore always deliberate: add the file to
`.fork/allowed-divergence` with a reason, or do not make the change. It also fails when a file only the fork has (not in upstream's tree at
that commit, workflows aside) is missing from `.fork/overlay-paths`: list a new fork-only file or crate folder there in the
pull request that adds it.

## After a sync: the capabilities file

`.fork/capabilities.json` lists what the built-in script extenders implement, read from the command tables
([`RELEASE-FILES.md`](RELEASE-FILES.md)). After the replay, the sync workflow regenerates it (`release-files capabilities`,
built from a plain export of the sync branch with the fork's own copy of the tool, so upstream's files never replace it),
validates it against `.fork/schemas/nv-rs-capabilities.v1.json` (a failure fails the run), and when the file changed commits
`chore(fork): regenerate the capabilities file for the replayed upstream commits` to `sync/upstream`; the pull request body
lists the commands added and removed. A sync that changes no table adds no commit. By hand it is still `task capabilities`.

## What does not follow upstream

- **Dependencies.** Renovate only updates fork-owned files ([`renovate.json`](../../renovate.json)).
  Upstream's `Cargo.toml` and `Cargo.lock` arrive through the replay.
- **Releases.** Upstream tags `preview-YYYY-MM-DD.N`. The fork's release-please makes `vX.Y.Z` tags from the
  replayed messages. They are separate version lines.
- **Upstream's CI.** `.github/workflows/ci.yml` is left out of the replay ("Workflow files") and is not used here. The fork's
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

## Stop reports per sync

The bench and soak runs write a `stop` final record into their JSON log when the game's main stops (docs/fork/LOGGING.md): its class
and, when the reason names a moved family, `family` and `field`. After a sync, count them for that sync with
`python3 -I scripts/fork/sync_report.py stops --dir ~/.local/state/nv-rs/bench --sync <replay point sha>`: the total, a table by class
and by family, and one tracking entry per family a stop names; `--open-entries` opens each as an issue labelled `stop-family`
(`Stop names moved family N field +x`; one open issue per title). The runs live on the machine that made them, so this is run
there, not in CI.
