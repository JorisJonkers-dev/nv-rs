<!-- SPDX-License-Identifier: GPL-3.0-only -->
# Porting rulebook

The rules every porting batch follows. The shakedown section is written by
`scripts/fork/porting_loop.py` from what the first batch showed; the standing
rules here are edited by hand when a batch shows a gap in them.

## Standing rules

1. Work comes only from the ledger's `queue` command. There is no hand-written
   work list and no second source of tasks.
2. One family per worktree. A family claim is held from the start of its work
   until the batch ends, so two workers never write the same family. Claims are
   shared by every worktree of a clone and expire after a configured age; a
   skipped or reclaimed family is named in the batch report and pull request.
3. Every build goes through the build daemon. The family's `Diff` cases run
   through `family_lab.py`, whose client command owns cargo under the shared
   limits.
4. A worker may read the checker files but may not change them. The integrity
   hash is checked at the start of a batch and again in each worktree before
   that family's work is accepted.
5. The worker and the reviewer are configured commands. A family is accepted
   only with a passing worker run, a passing `Diff` lab run with no failing
   cases, and a passing independent review. A failed batch keeps the branches
   it already accepted.
6. Each batch pull request states the `Diff` pass counts and the ledger's
   failure classes for the functions it offered.
7. A rejected function records one of the ledger's failure classes: `real`,
   `float-only`, `uninitialised read`, `hang` or `fault`.
8. The first 100 functions are a shakedown batch. It is reviewed, closed
   unmerged with a configured close command, and used to fix this rulebook
   before any batch is scaled up. Its rulebook update is committed on its own
   shakedown branch, never merged as part of a working batch.

9. Model rotation follows the shakedown and first batches' evidence. `hy4-flash` is the reliable worker;
   `minimax-m3`, `qwen3.6-35b-a3b` and `deepseek-v4.1-flash` are the spread. Models that stalled
   (`nemotron` ultra: no output for the whole timeout) are left out of the worker rotation, and so is any model
   whose run ends in a 524, a zero-token answer or a 402: a 402 means a non-free model slipped into the list,
   and a free-only rotation is checked with `model-gateway models --free` before a batch. A 429 or a
   "free deployment limited" answer moves the family to the next model and parks the limited one.
10. A worker's output is limited to the typed crate and the typed suites (`crates/engine-typed/src`,
    `crates/engine/tests/typed_*.rs`). The loop drops any other path from a run before it commits.
11. A suite uses the Diff harness's real types (`Case`, `Diff`, `Input` from `engine_core::typed::diff`, the
    `leaf` family and the family's own module); a Diff failure such as `cannot find type Case in this scope` is
    a missing `use`, not a reason to write a harness. The worker prompt carries one complete typed function with
    its registration and Diff case from an existing family, `facegen::sizes`, as the pattern to copy.

## Shakedown

<!-- shakedown:start -->
_No shakedown has been recorded yet._
<!-- shakedown:end -->
