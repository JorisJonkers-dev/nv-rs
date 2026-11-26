<!-- SPDX-License-Identifier: GPL-3.0-only -->
# Porting rulebook

The rules every porting batch follows. The shakedown section is written by
`scripts/fork/porting_loop.py` from what the first batch showed; the standing
rules here are edited by hand when a batch shows a gap in them.

## Standing rules

1. Work comes only from the ledger's `queue` command. There is no hand-written
   work list and no second source of tasks.
2. One family per worktree. A family claim is held from the start of its work
   until the batch ends, so two workers never write the same family.
3. Every build goes through the build daemon. The family's `Diff` cases run
   through `family_lab.py`, whose client command owns cargo under the shared
   limits.
4. A worker may read the checker files but may not change them. The integrity
   hash is checked at the start of a batch and again in each worktree before
   that family's work is accepted.
5. The worker and the reviewer are configured commands. A family is accepted
   only with both a passing worker run and a passing independent review.
6. Each batch pull request states the `Diff` pass counts and the ledger's
   failure classes for the functions it offered.
7. A rejected function records one of the ledger's failure classes: `real`,
   `float-only`, `uninitialised read`, `hang` or `fault`.
8. The first 100 functions are a shakedown batch. It is reviewed, closed
   unmerged, and used to fix this rulebook before any batch is scaled up.

## Shakedown

<!-- shakedown:start -->
_No shakedown has been recorded yet._
<!-- shakedown:end -->
