#!/usr/bin/env python3
"""The engine's component map: check it, print it, turn it into the CI matrix.

Reads .fork/components.toml and `cargo metadata --no-deps` (offline: it works when the registry proxy refuses).

  components.py check                     every workspace crate in exactly one component; `requires` cover the real
                                          dependencies; no cycle; every test file of a split crate is in a job.
                                          Exit 1 on an error.
  components.py graph [--timings FILE]    layers, crates, what `cargo test -p` compiles for each component, an
                                          ESTIMATE of the compile cost and (if a timing file is given) the measured
                                          test seconds
  components.py matrix --event EVENT [--base SHA --head SHA | --files-from FILE] [--impact FILE] [--all-tests]
                                          JSON for the workflow matrix: {"include": [...], "components": [...],
                                          "reason": "...", "impact": {...}}; EVENT pull_request runs only the affected
                                          components, any other event runs all of them. With --impact (the nightly's
                                          test-impact.json, scripts/fork/test_impact.py) a job runs only when a changed
                                          source file is one its tests execute; a missing, old or foreign map, or
                                          --all-tests, falls back (docs/fork/CI.md, "Test impact").
  components.py impact-plan               JSON for the nightly collection: one entry per job with the test commands

Stdlib only; Python 3.11 or newer (tomllib).
"""
import argparse
import fnmatch
import hashlib
import json
import os
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timedelta, timezone

# A change to one of these can change what every component does: every component runs.
GLOBAL_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
GLOBAL_PREFIXES = (".cargo/",)
# The fork's own files that steer the builds (the rest of the fork's files cannot change what the builds verify).
STEERING = {
    ".fork/components.toml", ".fork/linux-ok.txt", ".github/workflows/fork-build.yml",
    "scripts/fork/components.py", "scripts/fork/run-component.sh", "scripts/fork/test_archive.py", "scripts/fork/libtest_summary.py",
    "scripts/fork/clippy_annotations.py", "scripts/fork/linux_probe.py", "scripts/fork/gpu_gate.py", "scripts/fork/vkd3d_gate.py",
}
# Platform-relevant code: a pull request that changes it runs the Windows jobs too (otherwise a pull request runs Linux
# only; pushes to the integration branch run the Windows twins only for it, the schedule and a manual run run every OS).
# Only code that Windows alone can verify counts: a file with a cfg on windows, macos or unix, the Windows device, window
# and platform layers, the launcher and Play (their Windows build), and the toolchain and Cargo files. General engine code
# (engine-core, engine-typed, engine-bs, engine-fallout-*) runs on Linux; its Windows twins run in the nightly.
PLATFORM_CRATES = ("d3d9-*", "engine-platform", "fork-window", "nv-launcher", "fork-play")
PLATFORM_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rust-toolchain", "deny.toml"}  # by file name, in any directory
MACOS_CRATES = ("d3d9-*", "engine-platform", "fork-window", "fork-api", "fork-play", "fork-gfx")
MACOS_CFG = re.compile(r'cfg(?:_attr)?!?\([^\n]*\bmacos\b|target_os\s*=\s*"macos"|target_vendor\s*=\s*"apple"')
PLATFORM_CFG = re.compile(r'cfg(?:_attr)?!?\([^\n]*\b(?:windows|macos|unix)\b|target_os\s*=\s*"(?:windows|macos|linux)"|target_family\s*=\s*"(?:windows|unix)"')
DOCS = re.compile(r"^(docs/|LICENSE$|.*\.md$)")
# Estimates, from the cold Windows runs of 2026-11-13 (docs/fork/CI.md): seconds per 100 000 lines.
S_LIB, S_TEST, S_EXT = 4.6, 18.0, 35.0
WINDOWS_TIMEOUT, LINUX_TIMEOUT = 180, 90

# Test impact (scripts/fork/test_impact.py): the nightly map from each job to the source files its tests execute.
IMPACT_VERSION = 1
IMPACT_MAX_AGE_DAYS = 3
# A file with a macro in it can change the code of every file that calls the macro, and the line attribution of an
# expansion is not the definition's: such a file is selected by crate, as without a map.
IMPACT_MACRO = re.compile(r"\bmacro_rules!|#\[\s*proc_macro")
# Not measured by the nightly (they need the Nix shell, a display or a sound server): selected by crate, as without a map.
IMPACT_SKIP = {"rendering", "platform", "ds-mix", "window-layer"}


def sh(*cmd):
    done = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if done.returncode:
        sys.exit(f"{' '.join(cmd)} failed:\n{done.stderr[-1500:]}")
    return done.stdout


def read_list(path):
    """One entry per line; '#' starts a comment. A missing file is an empty list."""
    try:
        with open(path, encoding="utf-8") as fh:
            return [x for x in (line.split("#")[0].strip() for line in fh) if x]
    except FileNotFoundError:
        return []


def impact_usable(impact, is_ancestor, now=None, max_age_days=IMPACT_MAX_AGE_DAYS):
    """(map, reason): the test-impact map when it can be trusted for this commit, else (None, why not). is_ancestor(sha)
    tells whether the map's commit is an ancestor of the commit under test."""
    if not impact:
        return None, "no test-impact map (the nightly has not made one, or it could not be read)"
    if not isinstance(impact, dict) or impact.get("version") != IMPACT_VERSION or not isinstance(impact.get("jobs"), dict) \
            or not isinstance(impact.get("known"), list) or not impact.get("commit") or not impact.get("generated"):
        return None, "the test-impact map has an unknown format"
    try:
        made = datetime.fromisoformat(str(impact["generated"]).replace("Z", "+00:00"))
        if made.tzinfo is None:
            made = made.replace(tzinfo=timezone.utc)
    except ValueError:
        return None, "the test-impact map has no readable date"
    now = now or datetime.now(timezone.utc)
    if now - made > timedelta(days=max_age_days):
        return None, f"the test-impact map is older than {max_age_days} days ({impact['generated']})"
    if not is_ancestor(impact["commit"]):
        return None, f"the test-impact map was made at {str(impact['commit'])[:10]}, which this commit does not descend from"
    return impact, f"test-impact map of {str(impact['commit'])[:10]} ({impact['generated']})"


class Model:
    def __init__(self, cfg, meta, root=None, linux_ok=(), overlay=()):
        """cfg: the parsed components.toml; meta: cargo metadata (no deps); root: where the crate directories
        are on disk (default: the workspace root of the metadata); linux_ok: crates listed in linux-ok.txt;
        overlay: the fork's own paths (.fork/overlay-paths)."""
        self.cfg = cfg
        self.root = (root or meta["workspace_root"]).replace("\\", "/")
        mroot = meta["workspace_root"].replace("\\", "/")
        self.pk = {p["name"]: p for p in meta["packages"]}
        self.dirs = {}
        for n, p in self.pk.items():
            rel = os.path.relpath(os.path.dirname(p["manifest_path"].replace("\\", "/")), mroot).replace("\\", "/")
            self.dirs[rel] = n
        self.linux_ok, self.overlay = set(linux_ok), list(overlay)
        # Normal and build dependencies (what a library build needs) and dev-dependencies (what a crate's own
        # tests additionally link): the fallout crates dev-depend on each other, so the two differ.
        self.deps = {n: {d["name"] for d in p["dependencies"] if d["name"] in self.pk and d.get("kind") != "dev"}
                     for n, p in self.pk.items()}
        self.dev = {n: {d["name"] for d in p["dependencies"] if d["name"] in self.pk and d.get("kind") == "dev"}
                    for n, p in self.pk.items()}
        self.comps = {c["name"]: dict(c, crates=list(c.get("crates", []))) for c in cfg.get("component", [])}
        self.rules = cfg.get("rule", [])
        self.owner, self.by_rule = {}, set()
        self.errors, self.warnings = [], []
        self._assign()

    def _assign(self):
        for name, c in self.comps.items():
            for crate in c["crates"]:
                if crate not in self.pk:
                    self.errors.append(f"{name}: lists {crate}, which is not a workspace crate")
                elif crate in self.owner:
                    self.errors.append(f"{crate} is in both {self.owner[crate]} and {name}")
                else:
                    self.owner[crate] = name
        for crate in sorted(set(self.pk) - set(self.owner)):
            rule = next((r for r in self.rules if fnmatch.fnmatch(crate, r["match"])), None)
            if rule:
                self.assign_by_rule(crate, rule)
            else:
                self.errors.append(f"{crate} belongs to no component (add it to .fork/components.toml)")

    def assign_by_rule(self, crate, rule):
        target = rule["component"]
        if target not in self.comps:
            self.errors.append(f"rule {rule['match']}: unknown component {target}")
            return
        self.owner[crate] = target
        self.by_rule.add(crate)
        self.comps[target]["crates"].append(crate)
        self.warnings.append(f"{crate} is not listed in .fork/components.toml: assigned by the rule {rule['match']} to {target}")

    # ---- the checks -------------------------------------------------------------------------------------------

    def req_closure(self, comp):
        out, todo = set(), list(self.comps[comp].get("requires", []))
        while todo:
            c = todo.pop()
            if c not in out:
                out.add(c)
                todo += self.comps[c].get("requires", [])
        return out

    def check(self):
        for name, c in self.comps.items():
            for r in c.get("requires", []):
                if r not in self.comps:
                    self.errors.append(f"{name}: requires unknown component {r}")
        if any("requires unknown" in e for e in self.errors):
            return
        for name in self.comps:
            if name in self.req_closure(name):
                self.errors.append(f"{name}: requires itself (cycle)")
        if any("cycle" in e for e in self.errors):
            return
        for name, c in self.comps.items():
            allowed = {name} | self.req_closure(name)
            for crate in c["crates"]:
                for d in sorted(self.deps.get(crate, ())):
                    if self.owner.get(d) not in allowed:
                        self.errors.append(
                            f"{name}: {crate} depends on {d} (component {self.owner.get(d)}), which {name} does not require")
        if self.errors:
            return
        names = set()
        for name, c in self.comps.items():
            if c.get("runs_on") not in ("windows", "both"):
                self.errors.append(f"{name}: runs_on must be \"windows\" or \"both\"")
            if c.get("windows") not in (None, "required", "nightly"):
                self.errors.append(f"{name}: windows must be \"required\" or \"nightly\"")
            if not c["crates"] and "commands" not in c:
                self.errors.append(f"{name}: has no crates and no commands")
            self.check_jobs(name, c)
        if self.errors:
            return
        for name in self.comps:
            for e in self.entries(name):
                if e["name"] in names:
                    self.errors.append(f"two jobs are named {e['name']!r}")
                names.add(e["name"])

    @staticmethod
    def flags(run, flag):
        parts = run.split()
        return [parts[i + 1] for i, x in enumerate(parts) if x == flag and i + 1 < len(parts)]

    def tests_of(self, crate):
        d = next((k for k, v in self.dirs.items() if v == crate), None)
        tdir = os.path.join(self.root, d, "tests") if d else ""
        out = set()
        if os.path.isdir(tdir):
            for f in os.listdir(tdir):
                if f.endswith(".rs"):
                    out.add(f[:-3])
                elif os.path.isfile(os.path.join(tdir, f, "main.rs")):
                    out.add(f)
        return out

    def check_jobs(self, name, c):
        """A component split into jobs must name every crate and every integration-test file of the crates it splits."""
        jobs = c.get("job", [])
        if not jobs:
            return
        named, crates, optional = {}, set(), {}
        for job in jobs:
            for o in job.get("optional_tests", []):
                crate, _, t = o.partition("/")
                optional.setdefault(crate, set()).add(t)
            if not job.get("name") or not job.get("args"):
                self.errors.append(f"{name}: every job needs a name and args")
            if int(job.get("shards", 1)) > 1:
                # Shards run from the archive of the component's group (run-component.sh splits the tests of all the job's binaries):
                # a job that cannot be archived (a --lib or --doc run, several crates) cannot be sharded.
                if self.archivable(c, job) is None:
                    self.errors.append(f"{name}: job {job.get('name')!r} has shards, so it must run only `-p <one crate> --test ...` commands")
            for run in job.get("args", []):
                crates.update(self.flags(run, "-p"))
                for crate in self.flags(run, "-p"):
                    named.setdefault(crate, set()).update(self.flags(run, "--test"))
        for crate in sorted(set(c["crates"]) - crates):
            self.errors.append(f"{name}: crate {crate} is in no job (add it to a job of {name})")
        for crate in sorted(crates - set(c["crates"])):
            self.errors.append(f"{name}: a job tests {crate}, which is not a crate of {name}")
        for crate, tests in named.items():
            if not tests:
                continue  # a crate run whole (`-p host`) needs no list
            have = self.tests_of(crate)
            for t in sorted(have - tests - optional.get(crate, set())):
                self.errors.append(f"{name}: {crate}/tests/{t}.rs is in no job (add it to a job of {name})")
            for t in sorted(tests - have):
                self.errors.append(f"{name}: a job names --test {t}, but {crate}/tests/{t}.rs does not exist")
        # Targets that `--lib --test X` would leave out unless some job names them.
        for crate in named:
            d = next((k for k, v in self.dirs.items() if v == crate), None)
            for extra in ("examples", "benches", "src/bin"):
                if named[crate] and d and os.path.isdir(os.path.join(self.root, d, extra)):
                    self.errors.append(f"{name}: {crate}/{extra} exists, and a split job list does not run it")

    # ---- graph ------------------------------------------------------------------------------------------------

    def closure_of(self, crates, tests=True):
        """What cargo compiles to test these crates: themselves, their dev-dependencies (when `tests`) and all
        normal dependencies of those (dev-dependencies are not transitive)."""
        crates = list(crates)
        seen, todo = set(), crates + ([d for c in crates for d in self.dev[c]] if tests else [])
        while todo:
            c = todo.pop()
            if c not in seen:
                seen.add(c)
                todo += self.deps[c]
        return seen

    def layer(self, comp):
        r = self.comps[comp].get("requires", [])
        return 0 if not r else 1 + max(self.layer(x) for x in r)

    def workspace_features(self):
        """Features of each crate that some workspace member turns on in its Cargo.toml. `cargo test --workspace`
        unifies them; `cargo test -p X` does not, so the job must switch them on itself."""
        out = {}
        for p in self.pk.values():
            for d in p["dependencies"]:
                if d["name"] in self.pk:
                    out.setdefault(d["name"], set()).update(d.get("features", []))
        return out

    def missing_features(self, selected):
        """`crate/feature` flags a `-p` job needs so its crates are built with the features the whole workspace gives them."""
        have = {}
        for c in self.closure_of(selected, tests=False) | set(selected):
            for d in self.pk[c]["dependencies"]:
                if d["name"] in self.pk and (d.get("kind") != "dev" or c in selected):
                    have.setdefault(d["name"], set()).update(d.get("features", []))
        ws = self.workspace_features()
        return sorted(f"{c}/{f}" for c in selected for f in ws.get(c, set()) - have.get(c, set())
                      if f in self.pk[c].get("features", []))

    # ---- what a change reaches ----------------------------------------------------------------------------------

    def fork_owned(self, f):
        return any(f == o or (o.endswith("/") and f.startswith(o)) for o in self.overlay)

    def affected(self, files):
        """(everything, reason, changed crates, components whose own paths changed)"""
        crates, by_path, everything, reason = set(), set(), False, ""
        for f in files:
            hit = [n for n, c in self.comps.items() if any(f.startswith(p) for p in c.get("paths", []))]
            if hit:
                by_path.update(hit)
                continue
            if f in GLOBAL_FILES or f.startswith(GLOBAL_PREFIXES) or f in STEERING:
                everything, reason = True, reason or f"{f} steers the builds"
                continue
            if f.startswith("vendor/"):
                v = f.split("/")[1]
                users = {n for n, p in self.pk.items() if any(d["name"] in {v, v.removesuffix("-hal")} for d in p["dependencies"])}
                if users:
                    crates |= users
                else:
                    everything, reason = True, reason or f"vendored crate {v}"
                continue
            best = max((d for d in self.dirs if d != "." and (f == d or f.startswith(d + "/"))), key=len, default=None)
            if best:
                crates.add(self.dirs[best])
            elif DOCS.match(f) or self.fork_owned(f) or f.startswith(".github/"):
                continue  # cannot change what the builds verify
            else:
                everything, reason = True, reason or f"{f} is outside every crate"
        return everything, reason, crates, by_path

    def windows_crates(self):
        """The crates of the components that must build and test on Windows: `windows = "required"` or `runs_on = "windows"`."""
        return sorted({x for c in self.comps.values() if c.get("windows") == "required" or c["runs_on"] == "windows" for x in c["crates"]})

    def platform_relevant(self, files, macos=False):
        """Does a change touch code that only Windows can verify (macos=True: only macOS): a platform crate, a Cargo or toolchain
        file, or a Rust file that has a cfg on windows, unix or macos?"""
        crates, cfg = (MACOS_CRATES, MACOS_CFG) if macos else (PLATFORM_CRATES, PLATFORM_CFG)
        for f in files:
            if (os.path.basename(f) in PLATFORM_FILES or f.startswith(GLOBAL_PREFIXES)) and not macos:
                return True
            d = max((d for d in self.dirs if d != "." and f.startswith(d + "/")), key=len, default=None)
            if d and any(fnmatch.fnmatch(self.dirs[d], g) for g in crates):
                return True
            path = os.path.join(self.root, f)
            if f.endswith(".rs") and os.path.isfile(path):
                with open(path, encoding="utf-8", errors="replace") as fh:
                    if cfg.search(fh.read()):
                        return True
        return False

    def job_tests(self, c, job):
        """The integration-test binaries a job runs, as (crate, test)."""
        out = set()
        for run in (job or {}).get("args", []):
            for crate in self.flags(run, "-p"):
                out.update((crate, t) for t in self.flags(run, "--test"))
            for o in (job or {}).get("optional_tests", []):
                crate, _, t = o.partition("/")
                if crate in self.flags(run, "-p"):
                    out.add((crate, t))
        return out

    def split_tests(self, files):
        """(the files that remain, {(crate, test)}): a changed integration-test file that a component job lists by name
        reaches that job only, not every job that links the crate. Library code changes still reach all of them."""
        named = {t for c in self.comps.values() for job in c.get("job", []) for t in self.job_tests(c, job)}
        rest, tests = [], set()
        for f in files:
            m = re.match(r"^(.+?)/tests/([^/]+?)(?:\.rs|/.*)?$", f)
            crate = self.dirs.get(m.group(1)) if m else None
            if crate and (crate, m.group(2)) in named:
                tests.add((crate, m.group(2)))
            else:
                rest.append(f)
        return rest, tests

    @staticmethod
    def job_key(comp, job):
        """The name of a job in the test-impact map: the component, or `component / job` (without shard and OS)."""
        return comp if job is None else f"{comp} / {job['name']}"

    def impact_split(self, files, impact, exists=None):
        """(mapped, rest): the changed files the test-impact map can select by, and the files that are selected by crate.
        A file is mapped only when the nightly saw it (a new file is not in the map), it still exists (a deleted or renamed
        file is not), it is Rust source of a workspace crate, it is not a build script or a file that defines a macro, and no
        component names it as its own path."""
        exists = exists or (lambda f: os.path.isfile(os.path.join(self.root, f)))
        known = set(impact["known"])
        # A file that no measured job executed (types and constants only, a cfg that the Linux build leaves out, a file of
        # an unmeasured component) cannot select a job by the map: it selects by crate.
        executed = {f for j in impact["jobs"].values() for f in j["files"]}
        mapped, rest = set(), []
        for f in files:
            ok = f in known and f in executed and f.endswith(".rs") and not f.startswith("vendor/") and os.path.basename(f) != "build.rs" \
                and exists(f) and not any(f.startswith(p) for c in self.comps.values() for p in c.get("paths", [])) \
                and any(f.startswith(d + "/") for d in self.dirs if d != ".")
            if ok:
                try:
                    with open(os.path.join(self.root, f), encoding="utf-8", errors="replace") as fh:
                        text = fh.read()
                        ok = not IMPACT_MACRO.search(text) and not PLATFORM_CFG.search(text) and "cfg(target_" not in text
                except OSError:
                    ok = False
            if ok:
                mapped.add(f)
            else:
                rest.append(f)
        return mapped, rest

    def required_os(self, c):
        """The OS the nightly measures a component on: Linux, where the general suite runs (the tests are platform-agnostic).
        A component that is Windows only is not measured (None); a job that cannot pass on Linux is left out of the map."""
        return None if c["runs_on"] == "windows" else "ubuntu-latest"

    def impact_jobs(self):
        """What the nightly measures: one entry per job (a shard is not a job), the test commands without the
        compile settings, on the OS of the required job. Jobs that need the Nix shell or a display are not measured."""
        out = []
        for name, c in self.comps.items():
            if name in IMPACT_SKIP:
                continue
            for job in (c.get("job") or [None]):
                if "commands" in c:
                    cmds = list(c["commands"])
                elif job is None:
                    cmds = [self.test_command(" ".join(f"-p {x}" for x in c["crates"]))] + list(c.get("extra", []))
                else:
                    cmds = [self.test_command(r + self.optional_tests(job, r)) for r in job["args"]]
                cmds = [x for x in cmds if x.startswith("cargo test ")]
                if not cmds:
                    continue
                osn = self.required_os(c)
                if osn is None or (job or {}).get("os") == "windows":
                    continue
                key = self.job_key(name, job)
                out.append({"key": key, "component": name, "os": osn, "cache": name,
                            "name": f"Impact: {key}",
                            "workspaces": self.workspaces(cmds), "runs": cmds})
        return out

    # ---- jobs -------------------------------------------------------------------------------------------------

    def job_crates(self, c, job):
        if job is None:
            return list(c["crates"])
        return [x for run in job["args"] for x in self.flags(run, "-p")]

    def optional_tests(self, job, run):
        """`--test X` for each optional test of the job that exists in a crate this command runs with a test list.
        Upstream adds a test file in one commit and the fork's map cannot name it before that commit is replayed,
        so a name listed under `optional_tests` ("crate/test") is run once the file exists and ignored until then."""
        crates = self.flags(run, "-p")
        out = ""
        for o in job.get("optional_tests", []):
            crate, _, t = o.partition("/")
            if crate in crates and self.flags(run, "--test") and t in self.tests_of(crate):
                out += f" --test {t}"
        return out

    def test_command(self, run):
        flags = self.missing_features(self.flags(run, "-p"))
        return f"cargo test --locked {run}" + (f" --features {','.join(flags)}" if flags else "")

    def entries(self, comp, seeds=None, tests=(), windows=True, windows_full=True, mapped=frozenset(), mapped_crates=frozenset(), impact_jobs=None):
        """Matrix entries of one component. seeds: the changed crates of a pull request (None: run everything); tests: the
        changed (crate, test) files that select a job by name; windows: False leaves the Windows jobs out; windows_full: False
        leaves out the Windows twin of a component with `windows = "nightly"` (it runs in the full run and on a push that
        changes platform code only). mapped: changed files the test-impact map selects by (impact_jobs: its jobs), mapped_crates:
        their crates, which select a job the map has no entry for."""
        c = self.comps[comp]
        linux_ready = bool(c["crates"]) and set(c["crates"]) <= self.linux_ok
        # os, label, informational
        targets = [("windows-latest", "Windows", False)]
        if c["runs_on"] == "both":
            if c.get("linux") == "required":
                # A component of Linux-only tests (cfg(unix)): a required Linux job, no Windows job.
                targets = [("ubuntu-latest", "Linux", False)]
            elif linux_ready and c.get("windows") == "nightly":
                # Linux is the gate; the Windows twin is required too, but only where it is asked for (windows_full).
                targets = [("ubuntu-latest", "Linux", False)] + ([("windows-latest", "Windows", False)] if windows_full else [])
            elif linux_ready and c.get("windows") != "required":
                targets = [("ubuntu-latest", "Linux", False)]
            else:
                targets.append(("ubuntu-latest", "Linux, informational", not linux_ready))
        if not windows:
            targets = [t for t in targets if t[0] != "windows-latest"]
        elif seeds is not None:
            # A pull request that runs Windows does not also run the informational Linux twin: it cannot fail the run and
            # only finds out which crates could move to Linux, which the push to the integration branch and the schedule do.
            targets = [t for t in targets if not t[2]] or targets
        result = []
        for job in (c.get("job") or [None]):
            if seeds is not None:
                closure = self.closure_of(self.job_crates(c, job))
                hit = bool(closure & seeds) or bool(self.job_tests(c, job) & set(tests))
                if not hit and mapped:
                    data = (impact_jobs or {}).get(self.job_key(comp, job))
                    hit = bool(mapped & set(data["files"])) if data else bool(closure & mapped_crates)
                if not hit:
                    continue
            job_targets = targets
            if (job or {}).get("os") == "windows":
                # A job that stays on Windows (it needs it: cfg(windows) code, or a family that only Windows reproduces bit for bit).
                job_targets = [("windows-latest", "Windows", False)] if windows else []
            if "commands" in c:
                cmds = list(c["commands"])
            elif job is None:
                cmds = [self.test_command(" ".join(f"-p {x}" for x in c["crates"]))] + list(c.get("extra", []))
            else:
                flags = "".join(f" --config {c}" for c in job.get("config", []))
                cmds = [self.test_command(r + self.optional_tests(job, r)) + flags for r in job["args"]]
            shards = int((job or {}).get("shards", 1))
            arch = self.archivable(c, job) if "commands" not in c else None
            for shard in range(1, shards + 1):
                label = comp if job is None else f"{comp} / {job['name']}" + (f" {shard}/{shards}" if shards > 1 else "")
                got = self.shard_entries(comp, label, cmds, job_targets, f"{shard}/{shards}" if shards > 1 else "")
                for e in got:
                    if arch:
                        e["_arch"] = arch
                result += got
        return result

    def archivable(self, c, job):
        """(crate, tests, --config flags) of a job that only runs `-p <one crate> --test ...` commands: its test binaries can come
        from an archive that one job built for every such job of the component with the same crate and flags. None otherwise
        (a `--lib` or `--doc` run, several crates, or no job list)."""
        if not job:
            return None
        crates, tests = set(), []
        for run in job["args"]:
            if re.search(r"--(lib|doc|bins?|examples?|tests|benches)\b", run) or not self.flags(run, "--test"):
                return None
            crates.update(self.flags(run, "-p"))
            tests += self.flags(run, "--test") + [t for t in re.findall(r"--test (\S+)", self.optional_tests(job, run))]
        if len(crates) != 1:
            return None
        return (crates.pop(), tuple(dict.fromkeys(tests)), tuple(job.get("config", [])))

    def archived(self, include):
        """Entries of the same component, OS, crate and --config that run test binaries get one archive: a `build` job compiles the
        union of their binaries once (the compile is most of a typed job's time) and each of them runs its own from the
        archive. A group of one entry compiles for itself as before. Returns (entries, builds)."""
        groups = {}
        for e in include:
            a = e.pop("_arch", None)
            if a:
                groups.setdefault((e["component"], e["os"], a[0], a[2]), []).append((e, a[1]))
        builds = []
        for (comp, os_name, crate, config), members in groups.items():
            if len(members) < 2:
                continue
            tests = list(dict.fromkeys(t for _, ts in members for t in ts))
            ident = f"{comp}-{crate}-{os_name.split('-')[0]}-" + hashlib.sha1("|".join(config).encode()).hexdigest()[:6]
            args = f"-p {crate} " + " ".join(f"--test {t}" for t in tests) + "".join(f" --config {x}" for x in config)
            builds.append({"id": ident, "name": f"Build: {comp} {crate} tests ({os_name.split('-')[0].title()})", "component": comp, "cache": "engine-closure",
                           "os": os_name, "timeout": WINDOWS_TIMEOUT if os_name == "windows-latest" else LINUX_TIMEOUT,
                           "workspaces": ". -> target", "args": args, "jobs": len(members)})
            for e, _ in members:
                e["archive"] = ident
        for e in include:
            e.pop("_arch", None)
            e.setdefault("archive", "")
        return include, builds

    def shard_entries(self, comp, label, cmds, targets, shard):
        result = []
        for os_name, os_label, info in targets:
            result.append({
                "name": f"Component: {label} ({os_label})",
                "shard": shard,
                "component": comp, "cache": comp, "os": os_name, "informational": info,
                "workspaces": self.workspaces(cmds),
                "timeout": WINDOWS_TIMEOUT if os_name == "windows-latest" else LINUX_TIMEOUT,
                "script": "\n".join(cmds)})
        return result

    @staticmethod
    def workspaces(cmds):
        """What rust-cache keeps: the root workspace, and the separate ones a command names with --manifest-path."""
        out = [". -> target"]
        for cmd in cmds:
            for m in re.findall(r"--manifest-path (\S+)/Cargo\.toml", cmd):
                if f"{m} -> target" not in out:
                    out.append(f"{m} -> target")
        return "\n".join(out[1:] or out)

    def plan(self, run_all=True, files=(), all_os=True, windows_full=None, impact=None, exists=None):
        """all_os: False (a pull request without platform-relevant changes) leaves the Windows jobs out. windows_full: whether
        the Windows twins of `windows = "nightly"` components run (default: when everything runs). impact: a usable
        test-impact map: a job whose tests the map knows runs only when a changed file is one it executes."""
        if windows_full is None:
            windows_full = run_all
        by_path, reason, seeds, tests = set(), "every component runs on this event", None, set()
        mapped, mapped_crates, info = set(), set(), {"used": False, "mapped_files": 0, "crate_level_files": 0}
        if not run_all:
            files, tests = self.split_tests(files)
            if impact:
                mapped, files = self.impact_split(files, impact, exists)
                mapped_crates = self.affected(sorted(mapped))[2]
                info = {"used": True, "mapped_files": len(mapped), "crate_level_files": len(files)}
            everything, why, changed, by_path = self.affected(files)
            if everything:
                reason = why
                mapped = set()
            else:
                seeds = changed
                reason = f"{len(changed)} changed crate(s), {len(tests)} changed test file(s), {len(mapped)} changed source file(s) selected by the test-impact map and {len(by_path)} component path(s) in {len(files)} other changed file(s)"
        job_data = (impact or {}).get("jobs") or {}
        include, status = [], []
        for name, c in self.comps.items():
            force = seeds is None or c.get("pr") == "always" or name in by_path
            args = (None if force else seeds, tests)
            kw = {"mapped": mapped, "mapped_crates": mapped_crates, "impact_jobs": job_data}
            got = self.entries(name, *args, all_os, windows_full, **kw)
            include += got
            status.append({"component": name, "tested": bool(got), "jobs": len(got),
                           "why": "" if got else "no crate of its test closure changed" if all_os or not self.entries(name, *args, **kw) else "Windows only, and no platform code changed"})
        include, builds = self.archived(self.grouped(include))
        return {"include": include, "builds": builds, "components": status, "reason": reason, "impact": info}

    def grouped(self, include):
        """Components that name a `group` share one Windows job per group (checkout, toolchain and cache restore cost about
        50 s a job, more than the tests of a small crate): their commands run one after the other in it. Linux jobs stay one per
        component (they start in 20 s) and a split job (`job`) stays as it is."""
        out, groups = [], {}
        for e in include:
            g = self.comps[e["component"]].get("group")
            if not g or e["os"] != "windows-latest" or e["informational"] or e["shard"] or "Windows" not in e["name"] or " / " in e["name"]:
                out.append(e)
            elif g not in groups:
                groups[g] = dict(e, name=f"Component: {g} (Windows)", component=g, cache=f"group-{g}")
                out.append(groups[g])
            else:
                m = groups[g]
                m["script"] += "\n" + e["script"]
                m["timeout"] = max(m["timeout"], e["timeout"])
                m["workspaces"] = "\n".join(dict.fromkeys(m["workspaces"].split("\n") + e["workspaces"].split("\n")))
        return out


def load(toml_path=".fork/components.toml", metadata=None, linux_ok=".fork/linux-ok.txt", overlay=".fork/overlay-paths"):
    with open(toml_path, "rb") as fh:
        cfg = tomllib.load(fh)
    if metadata:
        with open(metadata, encoding="utf-8") as fh:
            meta = json.load(fh)
    else:
        meta = json.loads(sh("cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"))
    # A saved metadata names the workspace root of the machine that made it: the crate directories are here.
    return Model(cfg, meta, root=os.getcwd() if metadata else None, linux_ok=read_list(linux_ok), overlay=read_list(overlay))


def lines_of(model, crates, cache={}):  # noqa: B006 (a cache on purpose)
    total = 0
    for c in crates:
        if c not in cache:
            d = os.path.join(model.root, next(k for k, v in model.dirs.items() if v == c))
            n = 0
            for base, _, names in os.walk(d):
                for nm in names:
                    if nm.endswith(".rs"):
                        with open(os.path.join(base, nm), errors="replace") as fh:
                            n += sum(1 for _ in fh)
            cache[c] = n
        total += cache[c]
    return total


def graph_rows(model, timings=None):
    timings = timings or {}
    rows = []
    for name in sorted(model.comps, key=lambda n: (model.layer(n), n)):
        c = model.comps[name]
        lib, clo = model.closure_of(c["crates"], tests=False), model.closure_of(c["crates"])
        loc_clo, loc_own = lines_of(model, clo), lines_of(model, c["crates"])
        ext = S_EXT if clo & {"d3d9-wgpu", "engine-platform"} else 0
        est = loc_clo / 1e5 * S_LIB + loc_own / 1e5 * S_TEST + ext
        t = sum(v for k, v in timings.items() if k.split("::")[0] in c["crates"])
        rows.append((model.layer(name), name, c, len(lib), lines_of(model, lib), len(clo), loc_clo, est, t))
    return rows


def cmd_graph(model, a):
    timings = json.load(open(a.timings))["tests"] if a.timings else {}
    print("| Layer | Component | Crates | Library closure | Test closure (what `cargo test -p` compiles) | Estimated compile | Test seconds (measured upstream) | Runs on |")
    print("| ---: | --- | ---: | --- | --- | ---: | ---: | --- |")
    for layer, name, c, nl, ll, nc, lc, est, t in graph_rows(model, timings):
        print(f"| {layer} | {name} | {len(c['crates'])} | {nl} crates, {ll / 1000:.0f}k lines | {nc} crates, {lc / 1000:.0f}k lines "
              f"| ~{est:.0f} s | {t:.0f} | {c['runs_on']} |")


FIXTURE_NOTE = ("cargo metadata --no-deps of the workspace, reduced to what scripts/fork/components.py reads (name, manifest, "
                "features, dependencies: workspace ones with kind and the features they switch on, external ones by name and "
                "kind only). Refresh: see docs/fork/CI.md.")


def reduce_metadata(meta):
    """`cargo metadata --no-deps` reduced to the stored workspace graph (scripts/fork/tests/fixtures/workspace-graph.json):
    the root as /ws, the packages in manifest order, their feature names, and their dependencies in cargo's order (a
    workspace crate's with kind and features, an external one's with kind only)."""
    root = meta["workspace_root"].rstrip("/")
    names = {p["name"] for p in meta["packages"]}
    packages = []
    for p in meta["packages"]:
        deps = []
        for d in p["dependencies"]:
            if d["name"] in names and d.get("path"):
                deps.append({"features": list(d.get("features", [])), "kind": d.get("kind"), "name": d["name"]})
            else:
                deps.append({"kind": d.get("kind"), "name": d["name"]})
        manifest = p["manifest_path"]
        if manifest.startswith(root + "/"):
            manifest = "/ws/" + manifest[len(root) + 1:]
        packages.append({"dependencies": deps, "features": sorted(p.get("features", {})), "manifest_path": manifest,
                         "name": p["name"]})
    packages.sort(key=lambda p: p["manifest_path"])
    return {"_note": FIXTURE_NOTE, "packages": packages, "workspace_root": "/ws"}


def cmd_fixture(a, path="scripts/fork/tests/fixtures/workspace-graph.json"):
    """Writes the stored workspace graph from the live metadata (--write), or fails when it differs (the default)."""
    if a.metadata:
        with open(a.metadata, encoding="utf-8") as fh:
            meta = json.load(fh)
    else:
        meta = json.loads(sh("cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"))
    text = json.dumps(reduce_metadata(meta), indent=1, sort_keys=True) + "\n"
    if a.write:
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(text)
        print(f"wrote {path}")
        return
    with open(path, encoding="utf-8") as fh:
        stored = fh.read()
    if stored != text:
        print(f"::error title=components::{path} differs from the workspace: run `python3 -I scripts/fork/components.py fixture --write`",
              file=sys.stderr)
        sys.exit(1)
    print(f"ok: {path} equals the workspace")


def cmd_affected(model, a):
    """What the change against --base (default origin/fork), plus uncommitted work, reaches: the selection a pull request's CI makes."""
    if a.files_from:
        files = read_list(a.files_from)
    else:
        base = a.base or "origin/fork"
        files = sorted({f for f in sh("git", "diff", "--name-only", f"{base}...{a.head}").split("\n") + sh("git", "diff", "--name-only", "HEAD").split("\n") if f})
    everything, why, changed, by_path = model.affected(files)
    if a.crates:
        print("\n".join(sorted(changed)))
        return
    plan = model.plan(False, files)
    if a.commands:
        seen = []
        for e in plan["include"]:
            if e["os"] == a.os:
                seen += [c for c in e["script"].split("\n") if c and c not in seen]
        print("\n".join(seen))
        return
    print(f"files changed: {len(files)}")
    print("scope: " + (f"EVERYTHING ({why})" if everything else f"{len(changed)} changed crate(s), {len(by_path)} component path(s)"))
    print("changed crates: " + (", ".join(sorted(changed)) or "none"))
    for c in plan["components"]:
        print(f"  {'test' if c['tested'] else 'skip'} {c['component']}" + (f" ({c['jobs']} job(s))" if c["tested"] else ""))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["check", "graph", "matrix", "affected", "fixture", "platform", "windows-crates", "impact-plan"])
    ap.add_argument("--toml", default=".fork/components.toml")
    ap.add_argument("--metadata", help="a saved `cargo metadata --no-deps` instead of running cargo")
    ap.add_argument("--timings")
    ap.add_argument("--event", default="push")
    ap.add_argument("--base")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--files-from")
    ap.add_argument("--os", default="ubuntu-latest", help="affected: the runner OS whose commands --commands prints")
    ap.add_argument("--commands", action="store_true", help="affected: print the test commands of the affected jobs of --os (one per line)")
    ap.add_argument("--crates", action="store_true", help="affected: print only the changed crates (one per line)")
    ap.add_argument("--all-os", action="store_true", help="matrix: run the Windows jobs of a pull request too (the label ci:all-os)")
    ap.add_argument("--impact", help="matrix: the nightly's test-impact.json (scripts/fork/test_impact.py); a job runs only when a changed file is one its tests execute")
    ap.add_argument("--impact-max-age-days", type=int, default=IMPACT_MAX_AGE_DAYS, help="matrix: an older map is not used")
    ap.add_argument("--all-tests", action="store_true", help="matrix: run every component (the label ci:all-tests)")
    ap.add_argument("--macos", action="store_true", help="platform: ask about macOS instead of Windows")
    ap.add_argument("--write", action="store_true", help="fixture: write the stored workspace graph instead of checking it")
    a = ap.parse_args()
    if a.cmd == "fixture":
        cmd_fixture(a)
        return
    model = load(a.toml, a.metadata)
    model.check()
    for w in model.warnings:
        print(f"::warning title=components::{w}", file=sys.stderr)
    for e in model.errors:
        print(f"::error title=components::{e}", file=sys.stderr)
    if model.errors and a.cmd != "graph":
        sys.exit(1)
    if a.cmd == "check":
        print(f"ok: {len(model.pk)} crates in {len(model.comps)} components" + (f", {len(model.by_rule)} by rule" if model.by_rule else ""))
    elif a.cmd == "graph":
        cmd_graph(model, a)
    elif a.cmd == "affected":
        cmd_affected(model, a)
    elif a.cmd == "windows-crates":
        # The crates whose Windows build must stay required (windows = "required", or Windows only): the narrow Windows clippy.
        print(" ".join(f"-p {x}" for x in model.windows_crates()))
    elif a.cmd == "impact-plan":
        json.dump({"include": model.impact_jobs()}, sys.stdout, indent=1)
        print()
    elif a.cmd == "platform":
        # `true` when a change reaches code that only Windows or macOS can verify (see platform_relevant).
        files = read_list(a.files_from) if a.files_from else [f for f in sh("git", "diff", "--name-only", f"{a.base}...{a.head}").split("\n") if f]
        print("true" if model.platform_relevant(files, a.macos) else "false")
    else:
        # The schedule and a manual run (and a push or pull request with no base to compare with) run everything; a pull
        # request and a push to the integration branch run what the change reaches.
        full = a.event in ("schedule", "workflow_dispatch") or a.all_tests or not (a.files_from or (a.base and a.base.strip("0")))
        files = []
        if not full:
            files = read_list(a.files_from) if a.files_from else [f for f in sh("git", "diff", "--name-only", f"{a.base}...{a.head}").split("\n") if f]
        # A pull request runs Windows only for platform-relevant changes or with the label ci:all-os (--all-os); a push runs every OS.
        all_os = a.event != "pull_request" or a.all_os or full or model.platform_relevant(files)
        # The Windows twin of a `windows = "nightly"` component runs in the full run, and on a push to the integration branch
        # whose change is platform-relevant; a pull request gets its Linux job only (or the twin with the label ci:all-os).
        win_full = full or a.all_os or (a.event == "push" and model.platform_relevant(files))
        impact, why = None, "every component runs on this event" if full else "no --impact given"
        if a.impact and not full:
            try:
                with open(a.impact, encoding="utf-8") as fh:
                    loaded = json.load(fh)
            except (OSError, ValueError):
                loaded = None
            impact, why = impact_usable(loaded, lambda sha: subprocess.run(["git", "merge-base", "--is-ancestor", sha, a.head], capture_output=True, check=False).returncode == 0,
                                        max_age_days=a.impact_max_age_days)
            if not impact:
                print(f"::notice title=test impact::not used, selecting by crate: {why}", file=sys.stderr)
        elif a.all_tests:
            why = "the label ci:all-tests"
        plan = model.plan(full, files, all_os, win_full, impact)
        plan["impact"]["reason"] = why
        json.dump(plan, sys.stdout, indent=1)
        print()


if __name__ == "__main__":
    main()
