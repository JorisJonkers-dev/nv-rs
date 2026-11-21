# SPDX-License-Identifier: GPL-3.0-only
"""Labels for game runs, so several runs on one machine can be told apart in a system monitor (docs/fork/RUN-LABELS.md).

A label is `<kind>-<scenario or row>-<variant>`, for example `bench-views-lake-mead-lod0151`. It shows up as:

- the process name in Mission Center, which lists the file name of `/proc/PID/exe`: the viewer is started through a hard
  link (or, where a link is not possible, a copy) named after the label. A symlink does not do it, the monitor sees the
  target. The kernel's `comm` (15 characters) is taken from the same file name, so `ps` and `top` show it too;
- a systemd scope `nvgame-<label>-<n>.scope` in `nv-games.slice` with a description, for `systemd-cgtop` and
  `systemctl --user status`;
- `GAMERUN_LABEL` in the run's environment (the same variables as the `gamerun` wrapper).

Everything that reads the machine takes its paths as arguments, so the tests use temporary folders.
"""
import os
import re
import shutil
from pathlib import Path

SLICE = "nv-games.slice"
SCOPE_PREFIX = "nvgame-"
MAX_LEN = 100
_VALID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,%d}$" % (MAX_LEN - 1))
# A game folder with one of these names says nothing: the folder above it does.
_GENERIC = {"game", "data", "fallout-new-vegas", "fallout new vegas"}


def valid(label):
    return bool(label) and bool(_VALID.match(label))


def part(text):
    """One piece of a label: lower case, letters and digits, other runs of characters become one hyphen."""
    return re.sub(r"[^a-z0-9.]+", "-", str(text).lower()).strip("-.")


def variant_of(game, preset=None):
    """The variant of a bench run: the game folder's name (its parent's when the folder's own name is generic)."""
    p = Path(str(game)).resolve() if game else None
    name = p.name if p else ""
    if p and name.lower() in _GENERIC:
        name = p.parent.name
    pieces = [part(name)] if part(name) else []
    if preset and preset != "fixes":
        pieces.append(part(preset))
    return "-".join(pieces)


def bench_label(scenario, game=None, preset=None, asked=None, env=None):
    """The label of a bench run. A caller's label (`asked`, else NV_BENCH_LABEL in `env`) is used as given when it is valid
    and raises ValueError when it is not; otherwise `bench-<scenario>-<variant>`."""
    env = os.environ if env is None else env
    asked = asked or env.get("NV_BENCH_LABEL") or None
    if asked:
        if not valid(asked):
            raise ValueError(f"bad run label {asked!r}: letters, digits, . _ - and at most {MAX_LEN} characters, starting with a letter or digit")
        return asked
    pieces = ["bench", part(scenario)] + [v for v in [variant_of(game, preset)] if v]
    return "-".join(p for p in pieces if p)[:MAX_LEN].rstrip("-.")


def scope_unit(label, n):
    return f"{SCOPE_PREFIX}{label}-{n}.scope"


def scope_args(label, n, who=None, cwd=None):
    """Extra systemd-run arguments that name the scope, put it in the games slice and describe it."""
    desc = f"game run {label}" + (f", started by {who}" if who else "") + (f" from {cwd}" if cwd else "")
    return [f"--slice={SLICE}", f"--unit={scope_unit(label, n)}", f"--description={desc}"]


def is_elf(path):
    try:
        with open(path, "rb") as f:
            return f.read(4) == b"\x7fELF"
    except OSError:
        return False


class NamedExe:
    """The program under another file name: a hard link next to the real file (so libraries and data found beside the
    program are found), else a copy (a reflink where the file system has them) in `fallback_dir`. `path` is what to run;
    `close()` removes it. When nothing works `path` is the original and `how` is "none"."""

    def __init__(self, exe, label, fallback_dir=None):
        self.exe, self.label, self.how, self.path = Path(exe), label, "none", str(exe)
        self._made = None
        self._dir = None
        real = self.exe.resolve()
        if not valid(label) or not is_elf(real) or real.name in ("coreutils", "busybox", "toybox"):
            return
        cand = real.parent / label
        try:
            if cand.exists() or cand.is_symlink():
                if not cand.samefile(real):
                    return
                cand.unlink()  # the leftover of a run that ended badly
            os.link(real, cand)
            self._made, self.how, self.path = cand, "link", str(cand)
            return
        except OSError:
            pass
        base = Path(fallback_dir or Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache") / "gamerun")
        try:
            base.mkdir(parents=True, exist_ok=True)
            self._dir = Path(__import__("tempfile").mkdtemp(prefix=label + ".", dir=base))
            target = self._dir / label
            shutil.copy2(real, target)  # copy-on-write where the file system has it (btrfs), else a plain copy
            self._made, self.how, self.path = target, "copy", str(target)
        except OSError:
            if self._dir:
                shutil.rmtree(self._dir, ignore_errors=True)
            self._dir = None

    def close(self):
        if self._made is not None:
            try:
                self._made.unlink()
            except OSError:
                pass
            self._made = None
        if self._dir is not None:
            shutil.rmtree(self._dir, ignore_errors=True)
            self._dir = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


def in_game_scope(pid, proc="/proc"):
    """Whether the process is the first process of a labelled run's scope: its cgroup is a `nvgame-*.scope` and its parent's is
    not. A labelled viewer no longer has the comm `nv-viewer`, so the check for other viewers (lock.other_viewers) asks this."""
    def unit(p):
        try:
            for line in (Path(proc) / str(p) / "cgroup").read_text().splitlines():
                last = line.rsplit("/", 1)[-1]
                if last.startswith(SCOPE_PREFIX) and last.endswith(".scope"):
                    return last
        except OSError:
            pass
        return None
    mine = unit(pid)
    if not mine:
        return False
    try:
        ppid = int((Path(proc) / str(pid) / "stat").read_text().rsplit(")", 1)[1].split()[1])
    except (OSError, ValueError, IndexError):
        return True
    return unit(ppid) != mine
