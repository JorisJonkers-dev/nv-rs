<!-- SPDX-License-Identifier: GPL-3.0-only -->
# Unchanged shader constants (nv-rs#934)

Fork-only. A `Set*ShaderConstant*` whose registers already hold the command's words leaves the
wgpu device's register file alone and does not make the stage's uniform block due; the device writes
that block only when a register changed or the shader, viewport or other header state changed.
The draw capture path keeps its image copies in a thread-local pool, copies matrix bytes into a
caller's buffer, and a constant attribute's thirteen-word call passes its stack words as a slice
like the other short calls. The image pool holds at most eight finished copies; an image nested
deeper than eight open images takes a fresh vector. The order of the game's calls and the recorded
stream are unchanged.

`a_float_constant_set_copies_only_the_registers_that_changed`,
`an_integer_or_boolean_constant_set_copies_only_the_words_that_changed` and
`images_reuse_their_scratch_copy` build their inputs from scratch. The shader family's
`Diff::new` case `set_shader_constants` covers the typed function whose call words changed.

## Gates left to the maintainer

No game runs were available with this change. Run the two record streams with the switch off, in
the `vanilla` and `fixes` presets, and compare them byte for byte; then run the scene pixel gate
and the ABBA bench with `NV_PERF=1`:

```
export NV_REPEATABLE=300 NV_SOUND=out
# Repeat for NV_PRESET=vanilla and NV_PRESET=fixes, with NV_UNCAP unset.
NV_PRESET=vanilla NV_D3D9_RECORD=/tmp/constants-before.stream nv-rs-dev run
NV_PRESET=vanilla NV_D3D9_RECORD=/tmp/constants-after.stream nv-rs-dev run
cmp /tmp/constants-before.stream /tmp/constants-after.stream

nv-rs-scene run --viewer BIN --game INSTALL --places docs/fork/scenes/bench-quick.toml --out /tmp/constant-pics-before
nv-rs-scene run --viewer BIN --game INSTALL --places docs/fork/scenes/bench-quick.toml --out /tmp/constant-pics-before2
nv-rs-scene run --viewer BIN --game INSTALL --places docs/fork/scenes/bench-quick.toml --out /tmp/constant-pics-after
nv-rs-scene compare --noise /tmp/constant-pics-before2 /tmp/constant-pics-before /tmp/constant-pics-after

NV_PERF=1 nv-rs-scene bench --lock GAME.LOCK --viewer BIN --game DIR --places docs/fork/scenes/bench-quick.toml --out /tmp/constants-a1
NV_PERF=1 nv-rs-scene bench --lock GAME.LOCK --viewer BIN --game DIR --places docs/fork/scenes/bench-quick.toml --out /tmp/constants-b1
NV_PERF=1 nv-rs-scene bench --lock GAME.LOCK --viewer BIN --game DIR --places docs/fork/scenes/bench-quick.toml --out /tmp/constants-b2
NV_PERF=1 nv-rs-scene bench --lock GAME.LOCK --viewer BIN --game DIR --places docs/fork/scenes/bench-quick.toml --out /tmp/constants-a2
nv-rs-scene bench-compare /tmp/constants-a1/soak/bench.json /tmp/constants-b1/soak/bench.json
nv-rs-scene bench-compare /tmp/constants-a2/soak/bench.json /tmp/constants-b2/soak/bench.json
```

`NV_PERF_FILE=path` writes one line per frame for the main-thread timeline; the bench's `viewer.log`
prints the per-frame category means and the thread-busy line. The pass bar is lower main-thread time
on one bench scene and no regression beyond noise on any.
