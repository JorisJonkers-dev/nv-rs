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

* Workflow `Coverage` (`fork-coverage.yml`), separate from `Pipeline`. It is not in `Pipeline Complete`, is not a required check and never fails a pull request: the measure jobs are `continue-on-error`.
* Linux only, `cargo llvm-cov 0.9.1` (pinned) with the toolchain of `rust-toolchain.toml`, one job per component with the component's own first `cargo test -p ...` line. Not measured: `rendering` and `platform` (Nix shell, GPU), `integration` (hours), `launcher-and-ledger`, `release-files`, `repo-checks`. Windows-only components have no Linux job and are not measured.
* A change to `fork-coverage.yml` or `coverage_report.py` measures `formats` as a smoke run, so the comment has numbers.
* Baseline: the nightly run (03:47 UTC, also manual) measures every Linux component on `fork` and stores `cov-<component>` (lcov) for 30 days. A pull request downloads the latest one; with none yet it measures the base commit in the same job.
* Output: a sticky comment (total line coverage PR against base and the delta, a table per component, changed files with their uncovered lines, the count of uncovered changed lines), the same text in the run summary, and the artifacts `cov-<component>`, `coverage-comment`, `coverage-plan`.
* Permissions: `contents: read` everywhere; `actions: read` on the measure jobs (baseline download); `pull-requests: write` only on the comment job, which checks out nothing and posts a file the report job rendered. A pull request from a fork gets the run summary and no comment.
* Making it required later: add `coverage` to the `needs` of `pipeline-complete` in `fork-ci.yml` (owner decision, see below).

## Limits

* The delta compares with the last nightly run, not exactly the pull request's base commit.
* `core` and `fallout-*` components run instrumented differential tests; their jobs may take far longer than uncovered builds (timeout 90 minutes, then the report says "not measured").
* Files of a component that is not measured are counted as "not measured", not as uncovered.
