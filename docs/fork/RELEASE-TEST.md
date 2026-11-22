# Release test

A release of the fork is cut only after this test passes on a real machine. The release pull request
(release-please, `chore(fork): release X.Y.Z`) stays open until then. The results go into that pull request
and into the release test ticket as numbers and verdicts only: no game data, no screenshots, no paths of the
machine.

Every step that runs the game through the tool goes through `nv-rs-dev scene` ([SCENE.md](SCENE.md)): under the machine-wide
game lock, with the memory guard (docs/fork/BENCH.md).

## 1. Build at the release commit

The release commit is the head of the release pull request.

```sh
git -C ~/Projects/JorisJonkers-dev/nv-rs fetch origin
git -C ~/Projects/JorisJonkers-dev/nv-rs worktree add --detach \
  ~/Projects/JorisJonkers-dev/worktrees/nv-rs/release-X.Y.Z <release PR head>
cd ~/Projects/JorisJonkers-dev/worktrees/nv-rs/release-X.Y.Z
export NV_DEV_TARGET=~/.cache/nv-rs/target-release
nv-rs-dev build --opt
```

Pass: the build ends with `rc=0`. Record the commit.

## 2. The quick bench suite, three runs

```sh
for i in 1 2 3; do nv-rs-dev bench --suite quick --out ~/.local/state/nv-rs/release-X.Y.Z/quick-$i; done
```

Pass: each run ends with exit status 0, `noisy` is false in each `summary.json`, and the three runs agree within the noise
figures of SCENE.md. Record per run: wall seconds, the suite table (fps, p99, 1% low).

## 3. A new game to the first town, by hand

The tool places the player itself and does not play the opening. Start the game as a player would
(`nv-rs-dev run`, on a free display), start a new game, walk out of Doc Mitchell's house and to Goodsprings. Pass: the screens
are reached, the player walks, and the log has no `stopped:` line (`nv-rs-dev smoke --seconds 300` prints the checklist of
the same log). Record the time and the checklist.

## 4. No hitch flags

The hitch recorder writes a run summary with flags for every real run. Pass: no run of steps 3 and 5 has a hitch
flag (`hitches_per_min_high`, `p99_over_20ms`, `slow_windows`): the verdict's `no hitch flags` check, or the
`flags` list of the run summary (`nv-rs-dev runs`). Record the hitch count and the hitches a minute of each run.

## 5. Save round trip, by hand

Save in one game, quit, start again, choose Continue on the main menu, and compare the position. Pass: the save file is
written and the loaded position is within 50 units of the saved one. Record the distance and the size of the save. The
tool does not do this step (no real input).

## Results

Post the table on the release pull request and on the release test ticket:

| Step | Verdict | Numbers |
| --- | --- | --- |
| 1. Build | | commit |
| 2. Quick suite x3 | | wall s, fps, p99, 1% low, path |
| 3. New game to the first town (by hand) | | time, checklist |
| 4. Hitch flags | | hitches, hitches a minute |
| 5. Save round trip (by hand) | | distance, save size |

Merge the release pull request only when every step passes. If a step fails, file a ticket for the cause and
keep the release held.
