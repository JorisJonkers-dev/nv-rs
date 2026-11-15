# CI speed: measurements

Numbers only. Source: the 40 most recent `Pipeline` runs on 2026-11-15 (job timings of 6 of them, 4 pull request runs and 2 pushes to `fork`), read with `gh run view --json jobs`.

## Wall time of a whole run (created to updated)

| Run kind | Runs | Median | Min | Max |
| --- | ---: | ---: | ---: | ---: |
| push to `fork` | 9 | 22.1 min | 17.8 min | 23.8 min |
| pull request that builds | 5 | 21.3 min | 17.6 min | 23.2 min |
| pull request, documentation or fork-owned files only | 19 | 1.1 min | 0.9 min | 1.5 min |

Queue time of the first jobs: 0 to 2 s. The component jobs start 20 to 50 s after the run (they wait for `Plan components`, 17 to 25 s, plus the runner start). `Pipeline Complete` runs 1 to 12 s after the last job.

## Critical path (job durations in seconds, range over the 6 runs)

| Job | Seconds |
| --- | ---: |
| typed shader family (Windows) | 1004 to 1329 (compile 297 + tests 973 in run 43760013111) |
| typed draw, render, lights, particles | 630 to 808 |
| typed geometry and pathing | 393 to 744 |
| typed scene queries | 492 to 762 |
| typed runtime | 311 to 523 |
| typed animation | 270 to 471 |
| aggregator | 228 to 381 |
| Clippy (whole workspace, Windows) | 352 to 511 |
| Linux probe (informational) | 185 to 249 |
| every other component job | 17 to 334 |
| Format | 58 to 70 |
| Repository checks | 39 to 56 |
| Verify sync | 12 to 17 |

The slowest job is the typed shader family in every run; the run ends 1 to 5 s after it. The next slowest job is 40 to 55 % of it.

## Per-step cost inside a job (run 43760013111)

| Step | Seconds |
| --- | ---: |
| checkout | 10 to 21 |
| `rustup toolchain install` | 8 to 11 |
| Swatinem/rust-cache restore | 15 to 20 |
| rendering (Windows) test step | 57 |
| typed draw test step | 590 |
| typed shader test step | 1271 |
| Clippy step | 301 |

The rust-cache is saved only on `fork` pushes (`save-if`), keyed per component and OS; pull requests restore it.

## Counts

| | |
| --- | ---: |
| jobs in a full run | 33 |
| Windows jobs in a full run | 15 |
| test binary `typed_shader`: tests | 307 |

## After: typed shader family in three shards (PR 73 run, full build)

| | Seconds |
| --- | ---: |
| run wall time | 902 (15.0 min; before: median 21.3 min, range 17.6 to 23.2) |
| typed shader family shard 3/3 | 711 |
| typed shader family shard 2/3 | 522 |
| typed scene queries (now the slowest job) | 835 |
| typed draw | 638 |
| typed geometry and pathing | 574 |
| Clippy | 466 |
