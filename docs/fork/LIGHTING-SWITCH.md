# The lighting switch with the graphics overlay

Fork-only note on how upstream's Shift+F11 lighting switch (slaterain/nv-rs#1099, replayed as the commit with
`Upstream-Commit: a89c7c60a87fc7fa550d0307a179377262b8c9c3`) and the fork's own lighting work are combined. Rule of the
combination: one implementation of each thing. Where the two sides did the same job, the better one stays and the
other is gone; where each had something the other lacks, they are joined.

## What the player gets

- Shift+F11 cycles the new lighting while playing (sun shadows and ambient occlusion, sun shadows, ambient occlusion,
  neither), with a three second note and a log line. F11 stays the frame counter.
- The graphics overlay (F10) turns the same two parts from its `ao` and `sun_shadows` settings while the game runs and
  writes them to the settings file. The key does not write the file (upstream's rule): the file decides how the next
  game starts.
- The overlay's other live settings (`ao.quality`, `ao.strength`, shadow map size, fill light ...) apply as before.

## Decisions, collision by collision

| Collision | Upstream | Fork | Decision |
|---|---|---|---|
| Who builds the occlusion's pipelines | A worker thread of the device builds them ahead of the first switch-on (`lighting_ahead`, `scene_light::make`); no frame waits for the driver | `Gpu::encode` built them inline in the first frame that ran the step (a hitch of the driver's compile time), with its own copy of the pipeline code | Upstream's. The fork's inline copy is removed: `encode` calls the same `make` helpers when nothing was made ahead (a device where the switch is not allowed). One implementation of each pipeline |
| Ambient occlusion quality (`ao.quality`) | None: one fixed search | Pipelines made at the quality; changed in the overlay, they are made again | Joined. Pipelines are made at a quality (`shader_at`); a thing made ahead carries the quality it was made at, and `Gpu::take` drops what was made at another, so a quality change while the worker is busy never leaves a stale pipeline. `Device::scene_light` drops the whole step when the quality changed, as before |
| The sun's lookup in the occlusion (`light`) | None (occlusion ignores the sun) | The apply pass has a variant that reads the sun's lookup, so the occlusion leaves the direct light alone | Joined. The apply pipeline's key gains the `light` flag; the worker is asked for both variants of a scene target, so switching the sun shadows on makes no pipeline in a frame |
| Sun shadows' maps and pipelines | Made once on the lighting thread, kept for the run, restarted on switch-on (`Gpu::restart`); thin caster pipeline made at start | Shadow map size is a setting; thin pipeline made on first use | Upstream's structure on the fork's map size: `map_bytes` and `restart` are the same maps the overlay sizes. The fork's lazy `make_thin_pipeline` is removed (same pipeline, made on the worker) |
| Starting state | `Parts` from `bAmbientOcclusion` / `bSunShadows` | The graphics settings resolve these (`fork_graphics::apply`) before the host reads the choice | Same path: the host passes the resolved choice to `want_new_lighting`, so a preset or `graphics.toml` that turns a part on starts the game with it on |
| Switch while the overlay is open | Key only | Overlay only edits settings | `scene_light::set_wanted`: when the overlay changes its value for a part the wanted parts change (as the key does); an unrelated setting never resets what Shift+F11 chose |
| The video memory budget | None | Every texture and buffer is charged and refused past the budget | The depth surface remade readable by the worker is charged like the surface it replaces |
| Notes and the counter | Notes share the F11 counter's overlay drawn ahead | The F10 panel is its own painter | Both stay: they draw different things (a three second note, an editing panel) and share no code that could be merged |
| Logging | `eprintln!` | `fork_log` with a level and target | `fork_log::info!` with target `host` |

## Not changed

With both parts off at start and no switch, a frame is what it was: `want_new_lighting` installs nothing extra and
the device commands are the same (checked by recording, see the pull request).

## The trailer

The commit carries `Upstream-Late-Commit:` and not `Upstream-Commit:`. The replay point is the newest
`Upstream-Commit:` trailer on the branch (`scripts/fork/replay-upstream.sh`, `verify-sync.sh`), and this commit
is older than the sync that precedes it: with the usual trailer the next replay would start from it again and replay the
commits after it a second time.
