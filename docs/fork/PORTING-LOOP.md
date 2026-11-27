<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The porting loop

`scripts/fork/porting_loop.py` runs one batch of typed-function work from the
ledger's queue, then reports the rate at which verified functions are delivered.
It is fork tooling; it does not change the engine. The rules it enforces are in
[`PORTING-RULEBOOK.md`](PORTING-RULEBOOK.md).

```sh
python3 -I scripts/fork/porting_loop.py batch --count 20 --name 2026-11-26
python3 -I scripts/fork/porting_loop.py shakedown --count 100
python3 -I scripts/fork/porting_loop.py weekly --days 7 --out target/porting-loop/weekly.md
```

## What one batch does

1. It checks the checker hash (`private_integrity.py`) before doing any work.
2. It reads work only from `ledger queue`, through the build daemon's client
   command. Rows already marked `done`, and retired functions, are not work.
   A batch takes whole families: the queue keeps siblings together, so a
   family is never split across two batches.
3. It takes an exclusive claim per family and creates one git worktree for it.
   Claims live in the clone's common git directory, so worktrees of the same
   clone cannot take the same family; separate clones need `NV_PORT_CLAIM_DIR`.
   The worker's checker files are verified in its worktree right after the
   worker, before the Diff run and the review. A claim records its owner, time and
   process; one older than `NV_PORT_CLAIM_AGE` seconds (3600 by default) is
   reclaimed. A family held by another worker is skipped, and both skipped and
   reclaimed families are listed in the report and the pull request body.
   A failed batch keeps the branches it already accepted.
4. Every worktree starts at the protected `fork` branch, never at the caller's
   checked-out commit. Branch names carry a fresh per-run token, so a rerun
   with the same `--name` does not collide with an earlier batch.
5. It runs the configured worker command in that worktree, then runs the
   family's `Diff` cases with `family_lab.py`, which sends cargo to the build
   daemon. A non-zero lab exit status or a failing `Diff` case keeps that
   family out of the batch. It saves both command outputs beside the batch
   record.
6. It runs the configured review command, then checks the integrity hash in
   the worktree. A changed checker fails the batch; a failed review keeps
   that family out of it.
7. It commits a batch report on a batch branch, merges the accepted family
   branches into it, writes a pull request body with pass counts and failure
   classes, pushes the batch branch and calls the configured pull request
   command if one is set.

The pull request body, the JSON record and the saved command outputs live
under `target/porting-loop/`, which is ignored by git. The batch branch carries
the report and the delivery log, which are repository artefacts.

## Configuration

Both worker and reviewer must be commands; no service is built into the tool.

| Variable | Meaning |
| --- | --- |
| `NV_PORT_WORKER` | Command run in each family worktree. Required. |
| `NV_PORT_REVIEW` | Independent review command, run after the worker. Required. |
| `NV_PORT_PR` | Optional pull request command; it receives `--title`, `--body` and `--head`. The tool pushes the batch branch before calling it. Without it the body is written and printed. |
| `NV_PORT_CLOSE` | Command required by `shakedown` to close the review unmerged; it receives `--batch`, `--reason` and `--branch` (the rulebook branch). |
| `NV_PORT_CLAIM_DIR` | One claim directory shared by every worker. Separate clones of one repository share no git directory, so set it to a path they all see to exclude them from each other too. |
| `NV_PORT_BASE` | Ref every worktree starts at (default `fork`); set `origin/fork` when the local branch is stale. |
| `NV_PORT_CLAIM_AGE` | Seconds before a family claim is treated as stale and reclaimed (default 3600). |

Each worker and reviewer receives `NV_PORT_FAMILY`, `NV_PORT_BATCH`,
`NV_PORT_WORKTREE`, `NV_PORT_QUEUE_FILE` (that family's queue rows),
`NV_PORT_ADDRESSES`, `NV_PORT_SHAKEDOWN` and the caller's environment. The build
limits come from the build daemon's `NV_BUILD_*` variables. A pull request
command may use `GITHUB_TOKEN` from its own environment; the tool never prints
it and never puts it in a report.

## Shakedown and weekly rate

`shakedown` runs the first batch with `NV_PORT_SHAKEDOWN=1`, records it as
`closed unmerged`, opens no pull request and merges no worker branch. It
requires `NV_PORT_CLOSE`. It writes a rule for each failure class and each
rejection the batch actually showed into the shakedown section of
`PORTING-RULEBOOK.md`, commits that update on a `porting/shakedown/<name>`
branch, and leaves the branch unmerged. The branch name is printed, written to
the batch record, passed to the close command as `--branch` and pushed when
`NV_PORT_PR` is set. The standing rules are edited by hand
from that evidence before scaling.

The delivery log is `docs/fork/porting-batches/delivered.jsonl`, committed on
each batch branch. An address is recorded only when an accepted family's worktree
shows it `reference-verified` and the queue at the start of the batch did not,
so a function verified earlier is never re-dated by a later batch. `weekly` reads the queue and that log and
prints a table of delivered verified functions per day for the last `--days`
days ending today. A verified address with no log entry is credited to the day
it is first seen; a log entry whose address is no longer in the queue is
reported as a dropped record. A clean checkout or a fresh clone keeps the
committed log and the same report.

## Worker and reviewer commands

`scripts/fork/porting_agent.py` provides both commands for a model gateway:
`NV_PORT_WORKER="python3 -I /path/to/scripts/fork/porting_agent.py work"` and
`NV_PORT_REVIEW="python3 -I /path/to/scripts/fork/porting_agent.py review"`. The worker makes one
writing gateway run per family (the prompt is the rulebook, the family's queue rows and how to type
and register a function with its `Diff` case), applies the run's staged diff to `NV_PORT_WORKTREE`
and commits it. The reviewer judges that commit read-only, must be another model than the worker,
and exits 0 only on a `VERDICT: ACCEPT` line. Only public material is sent: queue rows are addresses
and names, never decompiled code. Models, the gateway command, the writing-run slot count
(`NV_PORT_SLOTS`, default 2) and the per-run metrics file (`NV_PORT_METRICS`, one JSON line per run
with model, seconds, status and outcome) are environment; see the script's docstring.
