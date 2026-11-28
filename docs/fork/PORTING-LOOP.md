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
python3 -I scripts/fork/porting_loop.py weekly --days 7 --since 2026-11-20
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
   When `NV_PORT_CLAIM_REPO` and a claim command are configured, it also takes
   the remote claim `porting-<family>` for `NV_PORT_CLAIM_ISSUE` through
   `claim acquire`, so workers on different machines or clones cannot take the
   same family. The lease is renewed before every worker attempt and released
   when the batch ends; a failed acquire skips the family like a held local
   claim. `claim list <repo>` shows the remote claims. The local claim files are
   the fallback and still apply when no claim command is configured.
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
   daemon. A non-zero worker exit, a non-zero lab exit or a failing `Diff` case
   retries the family up to `NV_PORT_ATTEMPTS` times in total (3 by default).
   Every retry resets that worktree to the base ref (`git reset --hard` and
   `git clean -fd`) and runs the worker with `NV_PORT_ATTEMPT` and
   `NV_PORT_FAILURE_FILE`; the file carries the failing case names and the first
   40 lines of error output from the previous attempt. A family that fails every
   attempt is rejected with the failure class of the last attempt. It saves all
   command outputs beside the batch record. The JSON record and the pull request
   body have an `## Attempts` section with the count and failure class of each
   attempt per family.
6. It runs the configured review command on the attempt that passed the worker,
   the checker and the Diff lab. A changed checker fails the batch; a failed
   review keeps that family out of it.
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
| `NV_PORT_CLAIM_CMD` | Optional command for remote claims; when unset, `claim` is looked up on PATH. Used only together with `NV_PORT_CLAIM_REPO`. |
| `NV_PORT_CLAIM_REPO` | `owner/name` slug passed to `claim acquire/renew/release/list`. Without it (or a claim command) claims are local files only. |
| `NV_PORT_CLAIM_ISSUE` | Issue number passed to `claim acquire` and `claim release`; required when a remote claim repo is configured. |
| `NV_PORT_CLAIM_LEASE` | Optional lease in seconds passed as `--lease-seconds` to `claim acquire`. |
| `NV_PORT_ATTEMPTS` | Total worker attempts per family after a worker, Diff or lab failure (default 3). |
| `NV_PORT_BASE` | Ref every worktree starts at (default `fork`); set `origin/fork` when the local branch is stale. |
| `NV_PORT_EXCLUDE` | Comma-separated family patterns (`fnmatch`) the batch never takes, for families another worker owns outside this clone's claims. |
| `NV_PORT_CHUNK` | Most functions per chunk (default 5; 0 keeps a family whole). A family's chunks run one after another in its worktree, each with its own worker, `Diff` run and review; a failing chunk is dropped and ends the family, keeping the chunks before it. |
| `NV_PORT_CHUNKS` | Chunks of one family taken in a batch (default 0, all of them). |
| `NV_PORT_CLAIM_AGE` | Seconds before a family claim is treated as stale and reclaimed (default 3600). |

Each worker and reviewer receives `NV_PORT_FAMILY`, `NV_PORT_BATCH`,
`NV_PORT_WORKTREE`, `NV_PORT_QUEUE_FILE` (that family's queue rows),
`NV_PORT_ADDRESSES`, `NV_PORT_SHAKEDOWN` and the caller's environment. A retry
also receives `NV_PORT_ATTEMPT` (1 for the first try) and, from the second try,
`NV_PORT_FAILURE_FILE` pointing at the previous attempt's failure. The build
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
committed log and the same report. With `--since DATE` (an ISO date), the report also gains a
`## Rate before and after DATE` section: the span from DATE through today and the same number
of days immediately before DATE, each with its verified-function count, span in days and rate
per hour (count divided by days times 24). Both spans are counted from the same delivery records
as the daily table, including records older than the table's `--days` window. Without `--since`
the report is unchanged.

## Worker and reviewer commands

`scripts/fork/porting_agent.py` provides both commands for a model gateway:
`NV_PORT_WORKER="python3 -I /path/to/scripts/fork/porting_agent.py work"` and
`NV_PORT_REVIEW="python3 -I /path/to/scripts/fork/porting_agent.py review"`. The worker makes one
writing gateway run per family (the prompt is the rulebook, the family's queue rows and how to type
and register a function with its `Diff` case), applies the run's staged diff to `NV_PORT_WORKTREE`
and commits it. On a retry it appends the section `Your previous attempt failed with this output`
and the text of `NV_PORT_FAILURE_FILE` to the prompt, so the next attempt knows which Diff cases
failed. The reviewer judges that commit read-only, must be another model than the worker,
and exits 0 only on a `VERDICT: ACCEPT` line. Only public material is sent: queue rows are addresses
and names, never decompiled code. Models, the gateway command, the writing-run slot count
(`NV_PORT_SLOTS`, default 2) and the per-run metrics file (`NV_PORT_METRICS`, one JSON line per run
with model, seconds, status and outcome) are environment; see the script's docstring.

`porting_agent.py probe` asks every configured model one cheap question and parks the ones that fail for an hour (they sort last in the rotation); run it before a batch.

## Parallel families and the rate report

`NV_PORT_MODEL_SLOTS` (default 20) is how many families run at once: each family still goes worker,
`Diff` through the family lab, then review, but families run in a thread pool. Claims, the report and the
merge stay single-threaded in result, and the report lists families in queue order. `NV_PORT_MODEL_SLOTS=1`
runs one family at a time. `NV_PORT_FAMILIES` (default unlimited) caps how many families a batch takes, so a count of rows
does not end up in one large family. A failure that ends the batch stops families not yet started.

`NV_PORT_PROVIDER_LIMIT` (default 4) bounds concurrent worker and review runs per provider, the part of the
model name before `/`, with one lock set per provider under `NV_PORT_SLOT_DIR`. It stacks with `NV_PORT_SLOTS`,
the global bound on writing runs.

The batch record and the pull request body carry a `## Rate` section: model runs, family lab seconds
(build and `Diff` run as one command, so one figure covers both), wall seconds, and verified functions
per hour (delivered addresses over wall hours). `weekly` prints verified functions per hour for its window next
to the per-day table, and `--since` adds the before-and-after comparison described above.
