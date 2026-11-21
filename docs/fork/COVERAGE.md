# Test coverage report on pull requests

What the org does today, what this fork copied, and how to read the comment. Numbers from a read-only survey of the 70 repositories of JorisJonkers-dev (clones under `~/Projects/JorisJonkers-dev`), 2026-11-16.

## Survey of the org

| Finding | Count | Where |
| --- | ---: | --- |
| Repositories with a workflow that mentions coverage (llvm-cov, jacoco, kover, c8, coverage.py, go -cover) | 30 | e.g. `loot-rs/.github/workflows/ci.yml`, `nvmod-rs/.github/workflows/pipeline.yml`, `lodwright-rs/.github/workflows/pipeline.yml`, `github-workflows/.github/workflows/ci.yml` |
| Rust repositories that gate line coverage with `cargo llvm-cov` | 3 | loot-rs (`task coverage`, `--fail-under-lines`, `Taskfile.yml` line 32), nvmod-rs and lodwright-rs (`task test`, step "Tests and coverage gate") |
| Repositories that publish coverage to Codecov, Coveralls or Sonar | 0 | none |
| Reusable coverage workflow in `github-workflows` | 0 | coverage there is per-repo `coverage report --fail-under` (python scripts) and the gates of the caller's own Taskfile |
| Coverage history in a bucket (S3, Garage, R2, gh-pages) | 0 | "bucket" in the org means `path-buckets.yml` (which CI jobs a changed path reaches, `identity/.github/buckets.yml`); the S3/Garage hits in `arr`, `delivery`, `karakeep` are image publishing |
| Pull-request comment with a report | 1 | `github-workflows/.github/workflows/mutation-report.yml` (sticky comment by marker, `gh api` PATCH/POST, `pull-requests: write`, never fails the build); `repository-hygiene-sweep.yml` does the same for hygiene |
| Per-module coverage table | 1 | loot-rs `Coverage per module` step: `cargo llvm-cov report --summary-only` into `$GITHUB_STEP_SUMMARY` (a summary, not a comment) |
| Changed-line helper | 1 | `github-workflows/scripts/changed_ranges.py` (the added lines of a `git diff -U0`, for mutation testing) |
| Coverage minimum | 80 % lines | org standard (`repo-template` ci.yml, loot-rs `COVERAGE_MIN`) |

Closest fit: loot-rs for the tool (`cargo llvm-cov`, ignore regex for test files, summary per module) and `mutation-report.yml` for the comment mechanics (marker, update in place, report never blocks). Neither shows a base comparison, a delta or the uncovered changed lines, and no bucket storage exists, so the history is kept as workflow artifacts.

## What was copied

| From | To |
| --- | --- |
| loot-rs `cargo llvm-cov ... --ignore-filename-regex`, `report --summary-only` | the measure job of `.github/workflows/fork-coverage.yml` |
| `mutation-report.yml` sticky comment by marker, `pull-requests: write` only on the comment job | the `comment` job (marker `<!-- fork-coverage -->`) |
| `changed_ranges.py` idea (hunks of `git diff -U0`) | `scripts/fork/coverage_report.py` (changed executable lines that no test ran) |
| nv-rs `scripts/fork/components.py` affected selection | `coverage_report.py plan`: only the components a change reaches |

## How it runs

* Workflow `Coverage` (`fork-coverage.yml`). On a pull request `fork-ci.yml` calls it as the job `coverage` (`workflow_call`), so it is part of `Pipeline Complete`; the nightly baseline is its own schedule. Only the gate (below) can fail it: the measure jobs are `continue-on-error`, and a failed comment does not count.
* Linux only, `cargo llvm-cov 0.9.1` (pinned) with the toolchain of `rust-toolchain.toml`, one job per component with the component's own first `cargo test -p ...` line. Not measured: `rendering` and `platform` (Nix shell, GPU), `integration` (hours), `launcher-and-ledger`, `release-files`, `repo-checks`. Windows-only components have no Linux job and are not measured.
* The `window-layer` job starts Xvfb and sets `NV_WINDOW_SMOKE=1` first: its tests open real windows and read them back from the X server (`fork_window::probe`), and without a display they would not run.
* A change to `fork-coverage.yml` or `coverage_report.py` measures `formats` as a smoke run, so the comment has numbers.
* Baseline: the nightly run (03:47 UTC, also manual) measures every Linux component on `fork` and stores `cov-<component>` (lcov) for 30 days. A pull request uses the nightly run that measured exactly its merge base; when no nightly run is on that commit it measures the merge base in the same job (the nightly's lcov of another commit would make the delta a mix of the pull request and the commits between). With no nightly run at all it measures the base commit and the gate only informs.
* Output: a sticky comment (total line coverage PR against base and the delta, a table per component, changed files with their uncovered lines, the count of uncovered changed lines), the same text in the run summary, and the artifacts `cov-<component>`, `coverage-comment`, `coverage-plan`.
* Permissions: `contents: read` everywhere; `actions: read` on the measure jobs (baseline download); `pull-requests: write` only on the comment job, which checks out nothing and posts a file the report job rendered. A pull request from a fork gets the run summary and no comment.
* The gate: the job `coverage-gate` (below) checks the changed lines of the fork-owned crates and blocks the merge through `Pipeline Complete`.

## The gate: fork-owned crates

The job `coverage-gate` of `fork-coverage.yml` computes, from the same artifacts as the comment, the share of the executable changed lines of the **fork-owned crates** that a test ran, and compares it with `min_changed_line_coverage` (70) of `.fork/coverage-gate.toml`. Fork-owned crates are derived, not listed by hand: directories named in `.fork/overlay-paths` (`crates/<name>/`) and the crates of a component of `.fork/components.toml` whose purpose says "Fork-owned" (today `fork-log`, `fork-hitch`, `fork-window`, `fork-presence`, `fork-memlog`, `fork-api`, `fork-play`), plus `extra_crates` of the toml. Upstream-owned code stays report-only. The comment shows the same result under "Fork-owned crates: changed-line coverage".

**The rule.** A pull request whose executable changed lines in the fork-owned crates are less than 70% covered (`min_changed_line_coverage`) fails `coverage-gate`, so it fails `Pipeline Complete` and cannot merge. The gate enforces only when BOTH hold: `enforce = true` in `.fork/coverage-gate.toml` (set since the first nightly baseline, run 43850925215), and the pull request's baseline is made at its merge base (`baseline.txt` reads "the last nightly run ... on the merge base" or "the merge base ..., measured in this run"; only the base commit measured because no nightly run exists at all does not count). In every other case it passes with a report:

* No nightly baseline (none yet, or the last one expired after 30 days): the gate warns ("informational: no nightly baseline artifact yet") and passes.
* A change to upstream-owned code only: no changed line in a fork-owned crate, so there is nothing to gate; the comment still reports its coverage.
* A fork-owned component that failed to measure: the report names it as not measured; its lines are not gated.

To switch the gate off again, set `enforce = false` (the job then passes and says so); to change the threshold, edit `min_changed_line_coverage`.

## The nightly run and heavy components

Every Linux component runs with a 90 minute timeout and `continue-on-error`. `core` and the `fallout-*` components run instrumented differential tests and are the ones that may exceed it; a component that times out is reported as "not measured" and does not break the run. If the first nightly run shows a timeout, split that component into `[[component.job]]` entries (as `integration` does) or add it to `SKIP` in `coverage_report.py`, and record the measured times here.

The first nightly run (run 43850925215, started by hand on `fork` on 2026-11-16) passed with every component measured and none
near the timeout; the whole run took about 13 minutes. Job times, from start to end:

| Component | Time |
|---|---|
| fallout-gameplay | 7 min 30 s |
| engine-base | 5 min 17 s |
| fallout-front | 4 min 56 s |
| fallout-world | 4 min 54 s |
| scripting | 4 min 09 s |
| native-saves | 3 min 41 s |
| core | 2 min 01 s |
| window-layer | 2 min 01 s |
| presence | 1 min 40 s |
| log | 1 min 21 s |
| memlog | 1 min 20 s |
| hitch | 1 min 17 s |
| formats | 1 min 09 s |

## Limits

* The delta compares with the pull request's merge base: the nightly lcov when a nightly ran on that commit, else the merge base measured in the same job (twice the measuring time of the component). A nightly runs once a day, so most pull requests measure their merge base.
* `core` and `fallout-*` components run instrumented differential tests; their jobs may take far longer than uncovered builds (timeout 90 minutes, then the report says "not measured").
* Files of a component that is not measured are counted as "not measured", not as uncovered.
