#!/usr/bin/env python3
"""The engine's component map: check it, print it, turn it into the CI matrix.

Reads .fork/components.toml and `cargo metadata --no-deps` (offline: it works when the registry proxy refuses).

  components.py check                     every workspace crate in exactly one component; `requires` cover the real
                                          dependencies; no cycle; every test file of a split crate is in a job.
                                          Exit 1 on an error.
  components.py graph [--timings FILE]    layers, crates, what `cargo test -p` compiles for each component, an
                                          ESTIMATE of the compile cost and (if a timing file is given) the measured
                                          test seconds
  components.py matrix --event EVENT [--base SHA --head SHA | --files-from FILE]
                                          JSON for the workflow matrix: {"include": [...], "components": [...],
                                          "reason": "..."}; EVENT pull_request runs only the affected components,
                                          any other event runs all of them

Stdlib only; Python 3.11 or newer (tomllib).
"""
import argparse
import fnmatch
import json
import os
import re
import subprocess
import sys
import tomllib

# A change to one of these can change what every component does: every component runs.
GLOBAL_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
GLOBAL_PREFIXES = (".cargo/",)
# The fork's own files that steer the builds (the rest of the fork's files cannot change what the builds verify).
STEERING = {
    ".fork/components.toml", ".fork/linux-ok.txt", ".github/workflows/fork-build.yml",
    "scripts/fork/components.py", "scripts/fork/run-component.sh", "scripts/fork/libtest_summary.py",
    "scripts/fork/clippy_annotations.py", "scripts/fork/linux_probe.py",
}
DOCS = re.compile(r"^(docs/|LICENSE$|.*\.md$)")
# Estimates, from the cold Windows runs of 2026-11-13 (docs/fork/CI.md): seconds per 100 000 lines.
S_LIB, S_TEST, S_EXT = 4.6, 18.0, 35.0
WINDOWS_TIMEOUT, LINUX_TIMEOUT = 180, 90


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
        named, crates = {}, set()
        for job in jobs:
            if not job.get("name") or not job.get("args"):
                self.errors.append(f"{name}: every job needs a name and args")
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
            for t in sorted(have - tests):
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

    # ---- jobs -------------------------------------------------------------------------------------------------

    def job_crates(self, c, job):
        if job is None:
            return list(c["crates"])
        return [x for run in job["args"] for x in self.flags(run, "-p")]

    def test_command(self, run):
        flags = self.missing_features(self.flags(run, "-p"))
        return f"cargo test --locked {run}" + (f" --features {','.join(flags)}" if flags else "")

    def entries(self, comp, seeds=None):
        """Matrix entries of one component. seeds: the changed crates of a pull request (None: run everything)."""
        c = self.comps[comp]
        linux_ready = bool(c["crates"]) and set(c["crates"]) <= self.linux_ok
        # os, label, informational
        targets = [("windows-latest", "Windows", False)]
        if c["runs_on"] == "both":
            if linux_ready and c.get("windows") != "required":
                targets = [("ubuntu-latest", "Linux", False)]
            else:
                targets.append(("ubuntu-latest", "Linux, informational", not linux_ready))
        result = []
        for job in (c.get("job") or [None]):
            if seeds is not None and not (self.closure_of(self.job_crates(c, job)) & seeds):
                continue
            if "commands" in c:
                cmds = list(c["commands"])
            elif job is None:
                cmds = [self.test_command(" ".join(f"-p {x}" for x in c["crates"]))] + list(c.get("extra", []))
            else:
                cmds = [self.test_command(r) for r in job["args"]]
            label = comp if job is None else f"{comp} / {job['name']}"
            for os_name, os_label, info in targets:
                result.append({
                    "name": f"Component: {label} ({os_label})",
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

    def plan(self, run_all=True, files=()):
        by_path, reason, seeds = set(), "every component runs on this event", None
        if not run_all:
            everything, why, changed, by_path = self.affected(files)
            if everything:
                reason = why
            else:
                seeds = changed
                reason = f"{len(changed)} changed crate(s) and {len(by_path)} component path(s) in {len(files)} changed file(s)"
        include, status = [], []
        for name, c in self.comps.items():
            force = seeds is None or c.get("pr") == "always" or name in by_path
            got = self.entries(name, None if force else seeds)
            include += got
            status.append({"component": name, "tested": bool(got), "jobs": len(got),
                           "why": "" if got else "no crate of its test closure changed"})
        return {"include": include, "components": status, "reason": reason}


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


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["check", "graph", "matrix"])
    ap.add_argument("--toml", default=".fork/components.toml")
    ap.add_argument("--metadata", help="a saved `cargo metadata --no-deps` instead of running cargo")
    ap.add_argument("--timings")
    ap.add_argument("--event", default="push")
    ap.add_argument("--base")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--files-from")
    a = ap.parse_args()
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
    else:
        files = []
        if a.event == "pull_request":
            if a.files_from:
                files = read_list(a.files_from)
            elif a.base:
                files = [f for f in sh("git", "diff", "--name-only", f"{a.base}...{a.head}").split("\n") if f]
            else:
                sys.exit("pull_request needs --base or --files-from")
        json.dump(model.plan(a.event != "pull_request", files), sys.stdout, indent=1)
        print()


if __name__ == "__main__":
    main()
