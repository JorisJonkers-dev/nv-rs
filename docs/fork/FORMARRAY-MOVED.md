<!-- SPDX-License-Identifier: GPL-3.0-only -->
# The form registry array, moved by default (nv-rs#951)

Fork-only. The `formarray` family (the registry array `011c54c4`, [`typed/form_array.rs`](../../crates/engine-core/src/typed/form_array.rs))
is in its K1 form (the array's element block in host memory, the guest heap as in stage 1) when `NV_TYPED` says nothing
(`typed::MOVED_BY_DEFAULT`). Rollbacks: `NV_TYPED=formarray` (stage 1), `formarray:freed` (K2), `formarray:dual`,
`formarray:off`. The family already charges the time-slice steps of the calls its moved form skips
(`Engine::typed_charge`, `crates/engine-typed/src/forms/array.rs`), as the lockfree family does (`LOCKFREE-MOVED.md`).

## Evidence (2026-11-27, goodsprings and `bench-quick.toml`, `NV_HAVOK_THREAD=0`, scene tool)

- Handle-renamed stream: goodsprings, 200 frames, repeatable. Stage 1 against the default (K1): byte-identical files
  (324,328,899 bytes); stage 1 against a second stage 1 and the default against a second default: byte-identical too.
- Dual run (`formarray:dual`, the five `bench-quick` places, 300 frames): exit status 0, 0 stops, 0 error lines.
- Shuffled schedules (`NV_THREADS=shuffle:1` through `:20`, five places, 200 frames): all 20 exit status 0, 0 stops, 0 error lines.
- Far moves soak (`soak-far-moves.toml`, 10 places, 300 frames, default): 10 of 10, exit status 0, 0 stops, 0 error lines.
- `engine-core` typed tests, `engine --test typed_form_array` and `typed_lockfree`.

Not done here: the step logs of stage 1, K1 and K2 for the whole run (the byte-identical stream already says the
threads hand over at the same steps) and the by-hand save round trip (RELEASE-TEST step 5). The array
is read by untyped code only through `form_array::slot`; the form map `011c54c0` and the editor id map `011c54c8` are
the next arrays of #951.
