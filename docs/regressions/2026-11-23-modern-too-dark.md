# The modern preset lost shapes and texture in shade and at night

- Issue: nv-rs#772
- Since: #769 (01f216aa)
- Cause: #769 cut the game's flat fill light to 0.25 outdoors. The fill was the only light on surfaces turned away from the sun and most of the light at night, so they went close to black.
- Why it was not caught: the picture case of #769 measured only the mean of sunlit ground.
- Fix: light from the sky by surface direction, in shadow too; ambient occlusion outdoors dims only that sky light; the moon's direct light follows its phase, fade and height; starlight is the night fill.
- Tests: `light_fails_a_picture_that_has_lost_its_detail_not_one_that_is_only_dark` (`crates/fork-scene/src/bin/nv-rs-scene.rs`), `a_dark_block_counts_as_lost_only_when_it_shows_nothing` (`crates/fork-scene/src/diff.rs`), `the_sky_light_is_brighter_from_above_and_keeps_a_wall_at_the_flat_value`, `day_and_night_take_their_own_shares`, `the_moon_light_follows_phase_fade_and_height`, `the_moon_path_stays_below_its_highest_climb`, `a_moonless_night_is_starlight_alone` (`crates/engine-fallout-env/src/units/fallout_shared_sky/sun.rs`), `the_occlusion_leaves_the_direct_light` (`crates/d3d9-wgpu/src/scene_light.rs`)
- Picture case: `nv-rs-scene light ORIGINAL MODERN --max-lost-detail 5` fails on the #769 values (Hoover +39.4 points, night Goodsprings +10.8, night Strip +9.0).
