# ADR-0022: The uncapped switch runs a fixed simulation step

> A fork decision record kept under docs/fork/adr/ because docs/adr/ is upstream-owned. It takes the next free number after ADR-0021.

**Status:** Proposed (design for the maintainer's decision on nv-rs#916 and nv-rs#933, 2026-11-26)
**Date:** 2026-11-26
**Issues:** nv-rs#916 (the performance programme), nv-rs#933 (this record), nv-rs#932 (the frame-model seam it builds on)

## Context

The game paces frames with `BSTimer`'s step (`00aa4ee0`). For a real-time timer the step sleeps out any gap under 10 ms and reports at least 10 ms; three timers step in each frame, so the original tops out near 133 frames a second. `NV_UNCAP` replaces that step with one timed on the microsecond clock. It removes the cap but gives the game a different `fDelta` on every frame: at 500 frames a second that is 2 ms.

Systems written for 10 ms and more then change their behaviour with the frame rate. A jump and a landing cover a different height per step; the slow-motion ramp moves by a different share of its rate; a held body is pulled once per frame and is pulled harder the shorter the frame. The existing companion fixes (`NV_HF_LANDING`, `NV_HF_SLOWMO`, `NV_GRAB_PUSH`) correct those effects one by one, but they do not make the game's clock and the simulation itself frame-rate independent.

Havok is a separate clock. It steps the world 16 ms at a time whatever the frame rate, using its own accumulator (`012677b0`) and `MAX_TIME` (`01267b38`), and draws bodies from the scene graph's nodes. `NV_PHYS_BLEND` already draws each free body between two Havok steps, so a frame with no step does not freeze it.

## Decision

1. With `NV_UNCAP` on, the main timer (`TimeGlobal`, `011f6394`) reports whole 1/60 s steps. An exact accumulator adds each frame's microseconds multiplied by 60, takes out whole steps and carries the rest in millionths of a step. A frame that does not complete a step reports zero seconds and only draws; the wall time it took is carried to the next step. The first step after a disable or a fixed step still answers as the translation does.
2. The game clock must differ from wall time by at most 0.5 % after 60 simulated seconds, at 60, 144, 300 and 500 frames a second. With the exact carry the difference after 60 s is under one step, about 0.03 %; the bound is checked by unit tests that drive the timer at each rate.
3. A frame takes at most the whole fixed steps that fit in the translation's 166 ms cap (nine), so a pause or a stalled thread cannot fast-forward the game. The wall time past the cap is not dropped: it stays carried, and the following frames take it, so the game clock still ends at the wall time.
4. The carried fraction is published for `NV_PHYS_BLEND`: a drawing-only frame draws the body that much of a fixed step later (in game seconds, times the time multiplier), capped just under the end of Havok's last step where there is nothing to blend towards, instead of holding it still. Havok's 16 ms step, its accumulator and the nodes game logic reads are untouched.
5. The other timers step once a frame as before and keep the uncapped step's own timing. Menus keep their frame limit; menu animation seconds still come from real elapsed time. `iLastTime` moves only with a reported step, so the game's other clocks that read it keep agreeing with `fDelta`. The slow-motion ramp does not move on a frame with no step; with the switch off the translation never reports a zero step there, so its behaviour is unchanged.
6. With `NV_UNCAP` off, `00aa4ee0` is not overridden at all. The translation, the `vanilla` preset and the `fixes` preset run as before, and record streams stay byte-identical.

## Consequences

- A frame with no completed step draws the same simulated state as the step before it, with physics bodies interpolated. Game time moves in 1/60 s increments, so the display can lead the simulation by up to one step; that is the usual trade for a fixed step and is bounded by one step.
- Landing, jump height, slow motion and grab push see the same step length at every frame rate, so the companion fixes have no rate-dependent correction to make while this switch is on.
- `NV_PHYS_BLEND` and `NV_UNCAP` are separate rows. Without `NV_PHYS_BLEND`, physics bodies are drawn where the scene graph puts them on drawing-only frames; the game's seconds stay fixed-stepped.
- No game runs were available for this decision. The clock bound and the step sequence at the four rates are unit-tested on the timer from inputs built in the test; the maintainer's machine runs the record-stream and pixel gates.
- Upstream edits to `crates/host/src/uncap.rs` and `crates/host/src/phys_blend.rs` conflict at replay; a person merges them.
