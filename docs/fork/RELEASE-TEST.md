# Release test

A release of the fork is cut only after this test passes on a real machine. The release pull request
(release-please, `chore(fork): release X.Y.Z`) stays open until then. The results go into that pull request
and into the release test ticket as numbers and verdicts only: no game data, no screenshots, no paths of the
machine.

Every step that runs the game goes through `nv-rs-dev bench` (see [BENCH.md](BENCH.md)): under the machine-wide
bench lock, in the nested headless session (nothing on the desktop), with the VRAM watchdog at 50%.

```sh
export NV_VRAM_LIMIT_PCT=50
W="--resolution 2560x1440 --vram-limit-pct 50 --nested headless"
```

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
for i in 1 2 3; do nv-rs-dev bench suite --quick $W; done
```

Unlimited frame rate (the default). Pass: each run's verdict is `PASS`, none is noisy, each is under its
120 s budget. Record per run: wall seconds, play-screen fps, p99 and 1% low, and the player's path.

## 3. A new game to the first town, real input

```sh
nv-rs-dev bench run goodsprings $W
```

The start-up movies, the main menu, a new game, the first room; the opening's quest stages free the player; a
console teleport to the first town; there the measured script walks and turns with real keys and the mouse.
A new game cannot walk to the first town without playing the whole opening, so the teleport stands in for
that walk. Pass: verdict `PASS` (the screens reached, the player placed, walked, and the camera turned).
Record the wall seconds, the play-screen fps and the path.

## 4. No hitch flags

The hitch recorder writes a run summary with flags for every run. Pass: no run of steps 2 and 3 has a hitch
flag (`hitches_per_min_high`, `p99_over_20ms`, `slow_windows`): the verdict's `no hitch flags` check, or the
`flags` list of the run summary (`nv-rs-dev runs`). Record the hitch count and the hitches a minute of each run.

## 5. Save round trip

Save in one session, quit, load in a new session, and compare. A bench run keeps the saves it writes in its own
folder (`nv-rs-dev` sets `NV_SAVE_OUT` to `~/.local/state/nv-rs/bench/saves-<time>-<pid>`), not in the port's saves
folder. For the load, the save is copied into the port's saves folder, and that folder is put back as it was
afterwards, so the saves already there are not touched.

```sh
SAVES="${XDG_DATA_HOME:-$HOME/.local/share}/nv-rs/userdata/Documents/My Games/FalloutNV/Saves"
nv-rs-dev bench run save $W
SAVED=$(ls -td ~/.local/state/nv-rs/bench/saves-*/ | head -1)
ls -l "$SAVED"nvbench-roundtrip.fos                # the save is on disk
KEEP=$(mktemp -d); cp -a "$SAVES"/. "$KEEP"/
cp "$SAVED"nvbench-roundtrip.* "$SAVES"/
nv-rs-dev bench run continue $W                    # Continue on the main menu loads the newest save: this one
find "$SAVES" -mindepth 1 -delete; cp -a "$KEEP"/. "$SAVES"/
grep -h '# driver: state' "$(ls -td ~/.local/state/nv-rs/bench/save-*/ | head -1)viewer.log" \
  "$(ls -td ~/.local/state/nv-rs/bench/continue-*/ | head -1)viewer.log"
```

`save` starts a new game, frees the player, goes to the first town, walks a little, notes the position, and saves
with the console (`save nvbench-roundtrip`). `continue` starts the game again, picks Continue on the main menu, and
notes the position once the play screen holds. Pass: both verdicts `PASS`, the save file was written, and the loaded
position is within 50 units of the saved one. Record the distance and the size of the save.

## Results

Post the table on the release pull request and on the release test ticket:

| Step | Verdict | Numbers |
| --- | --- | --- |
| 1. Build | | commit |
| 2. Quick suite x3 | | wall s, fps, p99, 1% low, path |
| 3. New game to the first town | | wall s, fps, path |
| 4. Hitch flags | | hitches, hitches a minute |
| 5. Save round trip | | distance, save size |

Merge the release pull request only when every step passes. If a step fails, file a ticket for the cause and
keep the release held.
