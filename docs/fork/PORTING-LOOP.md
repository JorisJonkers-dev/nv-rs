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
3. It takes an exclusive claim per family and creates one git worktree for it.
   A family held by another worker is skipped; claims are released when the
   batch ends.
4. It runs the configured worker command in that worktree, then runs the
   family's `Diff` cases with `family_lab.py`, which sends cargo to the build
   daemon. It saves both command outputs beside the batch record.
5. It runs the configured review command, then checks the integrity hash in
   the worktree. A changed checker rejects the batch; a failed review keeps
   that family out of it.
6. It commits a batch report on a batch branch, merges the accepted family
   branches into it, writes a pull request body with pass counts and failure
   classes, and calls the configured pull request command if one is set.

The batch report, the pull request body, the saved command outputs and the
delivery log live under `target/porting-loop/`, which is ignored by git. The
batch branch carries the report and the accepted work.

## Configuration

Both worker and reviewer must be commands; no service is built into the tool.

| Variable | Meaning |
| --- | --- |
| `NV_PORT_WORKER` | Command run in each family worktree. Required. |
| `NV_PORT_REVIEW` | Independent review command, run after the worker. Required. |
| `NV_PORT_PR` | Optional pull request command; it receives `--title`, `--body` and `--head`. Without it the body is written and printed. |
| `NV_PORT_CLOSE` | Optional command to close a shakedown review unmerged; it receives `--batch` and `--reason`. |

Each worker and reviewer receives `NV_PORT_FAMILY`, `NV_PORT_BATCH`,
`NV_PORT_WORKTREE`, `NV_PORT_QUEUE_FILE` (that family's queue rows),
`NV_PORT_ADDRESSES`, `NV_PORT_SHAKEDOWN` and the caller's environment. The build
limits come from the build daemon's `NV_BUILD_*` variables. A pull request
command may use `GITHUB_TOKEN` from its own environment; the tool never prints
it and never puts it in a report.

## Shakedown and weekly rate

`shakedown` runs the first batch with `NV_PORT_SHAKEDOWN=1`, records it as
`closed unmerged`, opens no pull request and merges no worker branch. It
rewrites only the shakedown section of `PORTING-RULEBOOK.md` with the counts and
failure classes it observed, so the standing rules can be fixed from evidence
before scaling.

Each accepted function is appended to the delivery log with its date.
`weekly` reads the queue to decide which addresses are verified, joins them to
that log, and prints a table of delivered verified functions per day for the
last `--days` days. Functions delivered on an earlier date, or not shown as
verified by the ledger, are not counted.
