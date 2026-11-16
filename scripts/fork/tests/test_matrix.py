"""Tests of scripts/fork/local/matrix.py (nv-rs-dev matrix, compare and clean).

No game, no GPU and no cargo: a fake `nv-rs-dev` stands in for the build, the tests and the bench runner, and fake ssh and
rsync shims run the "remote" host on this machine. Run: python3 -I -m unittest discover -s scripts/fork/tests -v
"""
import argparse
import contextlib
import io
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOCAL = HERE.parent / "local"
sys.path.insert(0, str(LOCAL))
import matrix  # noqa: E402

FAKE_DEV = r"""#!/usr/bin/env bash
# build: copy the checkout's marker file to the target dir; ci-lite: pass unless the marker says "fail-tests";
# bench run: write a summary whose fps comes from the binary ("slow" in it halves the fps).
set -u
cmd="$1"; shift
case "$cmd" in
  build) mkdir -p "$NV_DEV_TARGET/play"; cp "$NV_DEV_REPO/marker" "$NV_DEV_TARGET/play/nv-viewer"; chmod +x "$NV_DEV_TARGET/play/nv-viewer"
         grep -q fail-build "$NV_DEV_REPO/marker" && exit 1; exit 0 ;;
  ci-lite) grep -q fail-tests "$NV_DEV_REPO/marker" && { echo "boom"; exit 1; }; exit 0 ;;
  bench) [ "$1" = run ] && [ "${2:-}" = --help ] && { echo "usage: bench run"; exit 0; }
         sc="$2"; d="$NV_BENCH_DIR/$sc-20260101T000000"; mkdir -p "$d"
         fps=100; grep -q slow "$NV_BENCH_VIEWER" && fps=50
         printf '{"name":"%s","scenario":"%s","settings":{"mode":"capped"},"metrics":{"frame_time":{"fps":%s,"low1_fps":60,"p99_ms":12.0,"max_ms":30},"memory":{"rss_peak_mib":2000,"vram_peak_mib":3000,"gtt_peak_mib":500,"rss_growth_mib_per_min":0.5},"wgpu_errors":0,"stops":0,"untranslated_addresses":0,"skipped_draws":0},"verdict":{"ok":true,"checks":[]}}' "$sc-x" "$sc" "$fps" > "$d/summary.json"
         exit 0 ;;
esac
"""


def sh(*cmd, cwd=None):
    return subprocess.run(cmd, cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()


class Env(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="nv-rs-matrix-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        root = self.tmp / "Projects" / "owner"
        self.repo = root / "nv-rs"
        self.repo.mkdir(parents=True)
        sh("git", "init", "-q", "-b", "fork", cwd=self.repo)
        sh("git", "config", "user.email", "t@example.com", cwd=self.repo)
        sh("git", "config", "user.name", "t", cwd=self.repo)
        (self.repo / "marker").write_text("fast")
        sh("git", "add", "-A", cwd=self.repo)
        sh("git", "commit", "-qm", "base", cwd=self.repo)
        sh("git", "checkout", "-qb", "slowbranch", cwd=self.repo)
        (self.repo / "marker").write_text("slow")
        sh("git", "commit", "-qam", "slow", cwd=self.repo)
        sh("git", "checkout", "-qb", "badbuild", "fork", cwd=self.repo)
        (self.repo / "marker").write_text("fail-build")
        sh("git", "commit", "-qam", "bad", cwd=self.repo)
        sh("git", "checkout", "-q", "fork", cwd=self.repo)
        sh("git", "branch", "same", "fork", cwd=self.repo)
        self.dev = self.tmp / "fake-dev"
        self.dev.write_text(FAKE_DEV)
        self.dev.chmod(0o755)
        self.cache, self.state = self.tmp / ".cache" / "nv-rs", self.tmp / ".state" / "nv-rs"
        saved = {k: getattr(matrix, k) for k in ("CACHE", "STATE", "TARGET", "BIN_CACHE", "DEV", "HOME", "HOSTS_FILE", "SSH", "RSYNC")}
        self.addCleanup(lambda: [setattr(matrix, k, v) for k, v in saved.items()])
        matrix.CACHE, matrix.STATE, matrix.TARGET = self.cache, self.state, self.cache / "target"
        matrix.BIN_CACHE, matrix.DEV, matrix.HOME = self.cache / "matrix-bin", str(self.dev), self.tmp
        old = dict(os.environ)
        self.addCleanup(lambda: (os.environ.clear(), os.environ.update(old)))
        os.environ.update(NV_DEV_REPO=str(self.repo), NV_MATRIX_DEV=str(self.dev))

    def args(self, **kw):
        base = dict(spec=None, ref=None, baseline=None, config=None, scenario=None, host=None, no_test=False, no_bench=False, scale=None,
                    json=False, out=None, dry_run=False, local_only=False, spec_json=None)
        base.update(kw)
        return argparse.Namespace(**base)

    def run_matrix(self, **kw):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()):
            rc = matrix.cmd_matrix(self.args(**kw))
        return rc, buf.getvalue()


class SpecTests(Env):
    def test_spec_file_and_flags(self):
        p = self.tmp / "s.toml"
        p.write_text('refs = ["fork", "pr:5"]\nscenarios = ["boot"]\nscale = 0.5\n[thresholds]\nfps = 3\n[[config]]\nname = "lo"\nresolution = "1280x720"\nenv = { A = "1" }\n')
        s = matrix.load_spec(p, self.args(config=["mode=uncapped,resolution=800x600"], scenario=["newgame"]))
        self.assertEqual(s["baseline"], "fork")
        self.assertEqual([c["name"] for c in s["configs"]], ["lo", "offscreen-uncapped-800x600"])
        self.assertEqual(s["scenarios"], ["newgame"])
        self.assertEqual(s["thresholds"], {"fps": 3.0})
        self.assertEqual(s["configs"][0]["env"], {"A": "1"})

    def test_baseline_is_added_and_errors(self):
        s = matrix.load_spec(None, self.args(ref=["a"], baseline="b"))
        self.assertEqual(s["refs"], ["b", "a"])
        with self.assertRaises(matrix.Fail):
            matrix.load_spec(None, self.args())
        with self.assertRaises(matrix.Fail):
            matrix.load_spec(None, self.args(ref=["a"], config=["session=wayland"]))
        with self.assertRaises(matrix.Fail):
            matrix.load_spec(None, self.args(ref=["a"], config=["bogus=1"]))
        with self.assertRaises(matrix.Fail):
            matrix.load_spec(None, self.args(ref=["a"], config=["resolution=big"]))

    def test_resolve_ref(self):
        label, sha = matrix.resolve_ref(self.repo, "slowbranch")
        self.assertTrue(label.startswith("slowbranch@"))
        self.assertEqual(sha, sh("git", "rev-parse", "slowbranch", cwd=self.repo))
        self.assertIsNone(matrix.resolve_ref(self.repo, "working")[1])
        with self.assertRaises(matrix.Fail):
            matrix.resolve_ref(self.repo, "no-such-ref")

    def test_worktree_layout(self):
        _, sha = matrix.resolve_ref(self.repo, "slowbranch")
        wt = matrix.ensure_worktree(self.repo, "slowbranch@x", sha)
        self.assertEqual(wt, self.tmp / "Projects" / "owner" / "worktrees" / "nv-rs" / "matrix-slowbranch-x")
        self.assertEqual((wt / "marker").read_text(), "slow")


class MatrixTests(Env):
    def test_regression_and_report(self):
        rc, out = self.run_matrix(ref=["fork", "slowbranch"], scenario=["boot"], out=str(self.tmp / "out"))
        self.assertEqual(rc, 1)
        self.assertIn("REGRESSION", out)
        o = self.tmp / "out"
        for f in ("matrix.md", "matrix.html", "matrix.json", "local/results.json"):
            self.assertTrue((o / f).exists(), f)
        self.assertIn("<table>", (o / "matrix.html").read_text())
        self.assertNotIn("<script", (o / "matrix.html").read_text())

    def test_no_regression_json_mode(self):
        rc, out = self.run_matrix(ref=["fork", "same"], scenario=["boot"], json=True, out=str(self.tmp / "o2"))
        self.assertEqual(rc, 0)
        j = json.loads(out)
        self.assertTrue(j["ok"])
        self.assertEqual(j["hosts"][0]["refs"]["fork"]["test"], "pass")

    def test_build_cached_per_commit(self):
        self.run_matrix(ref=["fork"], no_bench=True, out=str(self.tmp / "o3"))
        rc, out = self.run_matrix(ref=["fork"], no_bench=True, json=True, out=str(self.tmp / "o4"))
        self.assertEqual(json.loads(out)["hosts"][0]["refs"]["fork"]["build"], "cached")

    def test_build_failure_fails_run(self):
        rc, out = self.run_matrix(ref=["fork", "badbuild"], scenario=["boot"], json=True, out=str(self.tmp / "o5"))
        self.assertEqual(rc, 1)
        j = json.loads(out)
        self.assertEqual(j["hosts"][0]["refs"]["badbuild"]["build"], "failed")

    def test_session_skips_are_not_failures(self):
        rc, out = self.run_matrix(ref=["fork", "same"], scenario=["boot"], config=["name=nested,session=nested-wayland"], json=True, out=str(self.tmp / "o6"))
        self.assertEqual(rc, 0)
        self.assertIn("not compared", out)

    def test_dry_run_and_compare(self):
        rc, out = self.run_matrix(ref=["fork"], dry_run=True)
        self.assertEqual(rc, 0)
        self.assertEqual(json.loads(out)["spec"]["baseline"], "fork")
        c = argparse.Namespace(baseline_ref="fork", candidate_ref="slowbranch", scenario=["boot"], no_test=True, no_bench=False, scale=None,
                               json=True, out=str(self.tmp / "o7"), dry_run=False)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(matrix.cmd_compare(c), 1)
        self.assertIn("fps", json.loads(buf.getvalue())["comparisons"][0]["regressions"])


class HostTests(Env):
    def hosts_file(self, **host):
        p = self.tmp / "hosts.toml"
        lines = ["[host.box]"] + [f'{k} = "{v}"' for k, v in host.items()]
        p.write_text("\n".join(lines) + "\n")
        return p

    def test_hosts_file_validation(self):
        p = self.tmp / "bad.toml"
        p.write_text('[host.box]\nssh = "me@box"\n')
        with self.assertRaises(matrix.Fail):
            matrix.load_hosts(p)
        self.assertEqual(matrix.load_hosts(self.tmp / "missing.toml"), {})

    def test_remote_command(self):
        cmd = matrix.remote_commands("box", {"ssh": "me@box", "repo": "~/src/nv-rs", "game": "/g/FNV", "cache": "~/c"}, "/r/spec.json", "/r")
        self.assertIn("git fetch -q origin", cmd)
        self.assertIn("NV_GAME=/g/FNV", cmd)
        self.assertIn("--local-only --spec-json /r/spec.json", cmd)
        self.assertNotIn("git fetch", matrix.remote_commands("box", {"ssh": "x", "repo": "r", "sync": "rsync"}, "s", "o"))

    def test_dry_run_prints_remote_commands(self):
        matrix.HOSTS_FILE = self.hosts_file(ssh="me@box", repo="~/src/nv-rs", sync="rsync")
        rc, out = self.run_matrix(ref=["working"], host=["box"], dry_run=True)
        self.assertEqual(rc, 0)
        cmds = json.loads(out)["remote"][0]["dry_run"]
        self.assertTrue(any(c.startswith("rsync -a --exclude /target") for c in cmds))
        self.assertTrue(any("--include=results.json" in c and "'--exclude=*'" in c for c in cmds))

    @unittest.skipUnless(shutil.which("rsync"), "rsync is not installed")
    def test_remote_run_through_shims(self):
        remote = self.tmp / "remote" / "nv-rs"
        shim = self.tmp / "shim"
        shim.mkdir()
        (shim / "ssh").write_text('#!/usr/bin/env bash\nshift\nexec env -u NV_DEV_REPO bash -c "${1/#\\~/$HOME}"\n')
        (shim / "rsync").write_text('#!/usr/bin/env bash\nargs=(); for a in "$@"; do args+=("${a#*@box:}"); done; exec rsync "${args[@]}"\n')
        for f in shim.iterdir():
            f.chmod(f.stat().st_mode | stat.S_IEXEC)
        matrix.SSH, matrix.RSYNC = [str(shim / "ssh")], [str(shim / "rsync")]
        matrix.HOSTS_FILE = self.hosts_file(ssh="me@box", repo=str(remote), sync="rsync", cache=str(self.tmp / "remote" / "cache"))
        # the "remote" gets the tooling from the local checkout, by rsync, as a real host would
        scripts = self.repo / "scripts" / "fork"
        shutil.copytree(LOCAL, scripts / "local")
        shutil.copytree(HERE.parent / "bench", scripts / "bench")
        rc, out = self.run_matrix(ref=["working", "same"], scenario=["boot"], host=["box"], json=True, out=str(self.tmp / "o8"), no_test=True)
        self.assertEqual(rc, 0, out)
        self.assertEqual(json.loads(out)["hosts"][0]["host"], "box")
        self.assertTrue((self.tmp / "o8" / "box" / "results-box.json").exists())


class CleanTests(Env):
    def clean(self, **kw):
        buf = io.StringIO()
        ns = argparse.Namespace(all=False, yes=False, dry_run=False)
        ns.__dict__.update(kw)
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()):
            rc = matrix.cmd_clean(ns)
        return rc, buf.getvalue()

    def populate(self):
        (matrix.TARGET / "play").mkdir(parents=True)
        (matrix.TARGET / "play" / "nv-viewer").write_bytes(b"x" * 5000)
        (self.state / "bench").mkdir(parents=True)
        (self.state / "bench" / "run.json").write_text("{}")
        keep = self.tmp / "keep"
        keep.mkdir()
        (keep / "mine.txt").write_text("do not remove")
        return keep

    def test_sizes_first_and_nothing_removed_without_yes(self):
        self.populate()
        rc, out = self.clean()
        self.assertIn("cargo target dir", out)
        self.assertIn("nothing removed", out)
        self.assertTrue(matrix.TARGET.exists())

    def test_default_keeps_bench_runs_and_all_removes_them(self):
        keep = self.populate()
        rc, out = self.clean(yes=True)
        self.assertFalse(matrix.TARGET.exists())
        self.assertTrue((self.state / "bench" / "run.json").exists())
        rc, out = self.clean(yes=True, all=True)
        self.assertFalse((self.state / "bench").exists())
        self.assertTrue((keep / "mine.txt").exists())

    def test_matrix_worktree_removed_and_others_kept(self):
        _, sha = matrix.resolve_ref(self.repo, "slowbranch")
        wt = matrix.ensure_worktree(self.repo, "slowbranch@x", sha)
        other = self.tmp / "Projects" / "owner" / "worktrees" / "nv-rs" / "my-feature"
        sh("git", "worktree", "add", "-q", "--detach", str(other), "fork", cwd=self.repo)
        self.clean(yes=True)
        self.assertFalse(wt.exists())
        self.assertTrue(other.exists())

    def test_allowed_refuses_outside_paths_and_symlinks(self):
        self.assertFalse(matrix.allowed(self.tmp))
        self.assertFalse(matrix.allowed("/"))
        self.assertFalse(matrix.allowed(self.tmp / "keep"))
        link = self.cache / "link-nv-rs"
        link.parent.mkdir(parents=True)
        link.symlink_to(self.tmp)
        self.assertFalse(matrix.allowed(link))

    def test_human(self):
        self.assertEqual(matrix.human(512), "512 B")
        self.assertEqual(matrix.human(2048), "2.0 KiB")


if __name__ == "__main__":
    unittest.main()
