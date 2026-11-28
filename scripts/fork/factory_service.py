#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""The porting factory as a long-running service: resume, tick, back off, watch for a stall, summarise the day.

    python3 -I scripts/fork/factory_service.py run      [--queue DIR] [--state DIR] [--tick COMMAND]
    python3 -I scripts/fork/factory_service.py watchdog [--queue DIR] [--state DIR] [--stall SECONDS]
    python3 -I scripts/fork/factory_service.py summary  [--queue DIR] [--state DIR] [--disk PATH]
    python3 -I scripts/fork/factory_service.py status   [--queue DIR] [--state DIR]

`run` is what the systemd unit starts (scripts/fork/systemd/). The queue (`factory_queue.py`) is a directory of files, so
it survives a reboot: on start, an attempt left `verifying` by the service's own earlier run goes back to `queued`
(there is one verifier, and it was just started). Then it runs the tick command in a loop. The default tick is
`factory_queue.py verify`: it prints `null` when nothing waits (the service sleeps), exits 3 when a review had no
readable answer (rate limited), and any other failure is a failure; a limited or failed tick backs off exponentially up
to an hour, so the loop never spins on a rate limit.

`watchdog` (a timer) looks for a stall: attempts waiting (`queued`, `verifying` or `retry`) and no change to any attempt
for the stall time (1 h). It writes a diagnosis file and runs the notify command (NV_FACTORY_NOTIFY, with the message as
its argument) at most once per six hours, and exits 1 so the unit shows failed. It never restarts the service: a stall
caused by a rate limit would only start over. `summary` (a timer) writes the last day's numbers: accepted, rejected,
retries, blocked attempts, the queue and the disk use.
"""
import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

FORK = Path(__file__).resolve().parent
sys.path.insert(0, str(FORK))
import factory_queue as fq

STALL_SECONDS = 3600
DAY = 86400
IDLE_SECONDS = 60
BACKOFF_BASE = 60
BACKOFF_CAP = 3600
RENOTIFY = 6 * 3600
LIMITED = 3
PENDING = ("queued", "verifying", "retry")
DEFAULT_TICK = f"{shlex.quote(sys.executable)} -I {shlex.quote(str(FORK / 'factory_queue.py'))} verify"

Runner = Callable[[list[str]], tuple[int, str]]
Sleeper = Callable[[float], None]


def run(argv: list[str]) -> tuple[int, str]:
    """(exit status, standard output); standard error goes to the service's own (the journal), so it never reads as an answer."""
    try:
        done = subprocess.run(argv, stdout=subprocess.PIPE, text=True, check=False)
    except OSError as exc:
        return 127, f"{argv[0]}: {exc}"
    return done.returncode, done.stdout


def state_dir() -> Path:
    base = os.environ.get("XDG_STATE_HOME") or str(Path.home() / ".local" / "state")
    return Path(base) / "nv-factory"


def read_json(path: Path) -> dict[str, Any]:
    try:
        data: Any = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}  # type: ignore[return-value]


def write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temp.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(temp, path)


def recover(queue: fq.Queue) -> list[str]:
    """Put attempts the previous run left `verifying` back in the queue; the ids."""
    back: list[str] = []
    for attempt in queue.all():
        if attempt["state"] == "verifying":
            attempt["state"] = "queued"
            queue.save(attempt)
            back.append(attempt["id"])
        queue.release(attempt["id"])
    return back


def backoff(failures: int) -> int:
    """Seconds to wait after `failures` limited or failed ticks in a row."""
    return 0 if failures <= 0 else min(BACKOFF_BASE * 2 ** min(failures - 1, 6), BACKOFF_CAP)


def tick(command: str, runner: Runner = run) -> str:
    """One run of the tick command: `worked`, `idle`, `limited` or `failed`."""
    code, output = runner(shlex.split(command))
    if code == LIMITED:
        return "limited"
    if code != 0:
        return "failed"
    return "idle" if output.strip().splitlines()[-1:] == ["null"] else "worked"


def serve(queue: fq.Queue, state: Path, command: str, runner: Runner = run, sleeper: Sleeper = time.sleep,
          clock: Callable[[], float] = time.time, more: Callable[[], bool] = lambda: True) -> int:
    """The loop; `more` ends it (tests). Returns the number of ticks."""
    recover(queue)
    failures = ticks = 0
    while more():
        result = tick(command, runner)
        ticks += 1
        failures = failures + 1 if result in ("limited", "failed") else 0
        pause = backoff(failures) or (IDLE_SECONDS if result == "idle" else 0)
        write_json(state / "service.json", {"last_tick": clock(), "result": result, "failures": failures, "sleep": pause})
        if pause:
            sleeper(pause)
    return ticks


def counts(queue: fq.Queue) -> dict[str, int]:
    found: dict[str, int] = dict.fromkeys(fq.STATES, 0)
    for attempt in queue.all():
        found[attempt["state"]] = found.get(attempt["state"], 0) + 1
    return found


def last_change(queue: fq.Queue) -> float | None:
    """When any attempt last changed (the newest attempt file), or None for an empty queue."""
    stamps = [path.stat().st_mtime for path in queue.directory.glob("*.json")]
    return max(stamps) if stamps else None


def stalled(queue: fq.Queue, now: float, limit: float = STALL_SECONDS) -> bool:
    """Attempts wait and nothing has changed for `limit` seconds."""
    changed = last_change(queue)
    waiting = sum(counts(queue)[s] for s in PENDING)
    return waiting > 0 and changed is not None and now - changed > limit


def disk_line(path: str) -> str:
    usage = shutil.disk_usage(path)
    return f"disk {path}: {usage.used / 2**30:.0f} GiB used of {usage.total / 2**30:.0f} GiB, {usage.free / 2**30:.0f} GiB free"


def diagnose(queue: fq.Queue, state: Path, now: float, disk: str) -> str:
    changed = last_change(queue)
    lines = [f"queue: {json.dumps(counts(queue), sort_keys=True)}",
             f"last change: {'never' if changed is None else f'{now - changed:.0f} s ago'}",
             f"service: {json.dumps(read_json(state / 'service.json'), sort_keys=True)}", disk_line(disk)]
    lines += [f"{a['id']} {a['state']} retries={a['retries']} infra={a['infra']}" for a in queue.all()
              if a["state"] in PENDING]
    return "\n".join(lines) + "\n"


def notify(command: str | None, message: str, runner: Runner = run) -> None:
    if command:
        runner([*shlex.split(command), message])


def watchdog(queue: fq.Queue, state: Path, now: float, limit: float = STALL_SECONDS, disk: str = "/",
             notifier: str | None = None, runner: Runner = run) -> int:
    """0 when the queue moves or is empty, 1 on a stall; the alert goes out once per RENOTIFY."""
    if not stalled(queue, now, limit):
        return 0
    memory = read_json(state / "watchdog.json")
    if now - float(memory.get("alerted", 0)) > RENOTIFY:
        report = diagnose(queue, state, now, disk)
        (state / "diagnosis").mkdir(parents=True, exist_ok=True)
        (state / "diagnosis" / f"stalled-{int(now)}.txt").write_text(report, encoding="utf-8")
        write_json(state / "watchdog.json", {"alerted": now})
        notify(notifier, f"porting factory: no progress for {limit / 3600:g} h; see {state / 'diagnosis'}", runner)
    return 1


def summary(queue: fq.Queue, now: float, disk: str = "/") -> str:
    """The last day's numbers."""
    day: list[dict[str, Any]] = []
    for a in queue.all():
        try:
            if now - queue.path(a["id"]).stat().st_mtime <= DAY:
                day.append(a)
        except OSError:
            continue  # the attempt file went away since the listing
    accepted = sum(a["state"] == "accepted" for a in day)
    rejected = [a for a in day if a["state"] == "rejected"]
    head = (f"porting factory, last 24 h: {accepted} accepted, {len(rejected)} rejected, "
            f"{sum(a['retries'] for a in day)} retries, {sum(a['infra'] for a in day)} verifier failures")
    lines = [head,
             f"queue now: {json.dumps(counts(queue), sort_keys=True)}", disk_line(disk)]
    lines += [f"blocked: {a['id']}: {(a['feedback'] or ['no feedback'])[0]}" for a in rejected]
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None, runner: Runner = run, sleeper: Sleeper = time.sleep,
         clock: Callable[[], float] = time.time, more: Callable[[], bool] = lambda: True, out: Any = sys.stdout) -> int:
    parser = argparse.ArgumentParser(description=(__doc__ or "").split("\n")[0])
    parser.add_argument("command", choices=("run", "watchdog", "summary", "status"))
    parser.add_argument("--queue", default=str(Path.home() / ".cache" / "nv-workers" / "factory-queue"))
    parser.add_argument("--state", default=str(state_dir()))
    parser.add_argument("--tick", default=os.environ.get("NV_FACTORY_TICK", DEFAULT_TICK))
    parser.add_argument("--stall", type=float, default=STALL_SECONDS)
    parser.add_argument("--disk", default=str(Path.home()))
    args = parser.parse_args(argv)
    queue, state = fq.Queue(args.queue), Path(args.state)
    notifier = os.environ.get("NV_FACTORY_NOTIFY")
    if args.command == "run":
        serve(queue, state, args.tick, runner, sleeper, clock, more)
        return 0
    if args.command == "watchdog":
        code = watchdog(queue, state, clock(), args.stall, args.disk, notifier, runner)
        print(diagnose(queue, state, clock(), args.disk) if code else "ok", file=out, end="" if code else "\n")
        return code
    if args.command == "summary":
        text = summary(queue, clock(), args.disk)
        (state / "summary").mkdir(parents=True, exist_ok=True)
        (state / "summary" / f"{time.strftime('%Y-%m-%d', time.gmtime(clock()))}.txt").write_text(text, encoding="utf-8")
        notify(notifier, text.splitlines()[0], runner)
        print(text, file=out, end="")
        return 0
    print(diagnose(queue, state, clock(), args.disk), file=out, end="")
    return 0


if __name__ == "__main__":  # pragma: no cover - exercised by the subprocess test
    sys.exit(main())
