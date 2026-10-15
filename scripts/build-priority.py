#!/usr/bin/env python3
"""Run agent-doc's own builds and tests below interactive work (`#keepidesnappy`).

`make check` is a full Cargo build, nextest, three JetBrains Gradle builds, and
a parallel TLC sweep. With several worktrees checking at once (2026-10-15: ~11
concurrent checks, load average 115-427 on 32 cores) they inherited the nice
level of the tmux server that launched them -- `-4` on CachyOS, whose ananicy
rules class tmux as `Doc-View` -- while the IDE ran at `+9` (`Heavy_CPU`). The
builds out-ranked the editor the operator was typing into.

The Makefile calls this at parse time:

* `apply --pid <make pid>` raises the make process to `--nice` (default 19) and
  the idle I/O class, so every recipe, compiler, test, and Gradle/TLC JVM it
  forks inherits the lower priority. It only ever *lowers* priority: a process
  already nicer than requested is left alone, and an unprivileged caller could
  not raise priority back anyway. Every failure is reported in the status
  string and never fails the build.
* `jobs --cpus N` sizes Cargo / nextest / Gradle / TLC parallelism from the
  cores that are actually free (`cpus - headroom - load1`), clamped between a
  floor that keeps progress and a ceiling that always leaves headroom.

Opt out with `AGENT_DOC_BUILD_PRIORITY=normal`; tune with `BUILD_NICE`,
`BUILD_IONICE_CLASS`, `BUILD_JOBS`, `CARGO_BUILD_JOBS`, `NEXTEST_TEST_THREADS`,
`GRADLE_MAX_WORKERS`, `TLA_JOBS`, `TLA_WORKERS` (see docs/development/building.md).
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import List, Optional, Tuple

ROOT = Path(__file__).resolve().parent.parent
IONICE_IDLE = 3
_IONICE_NAMES = {"none": 0, "realtime": 1, "best-effort": 2, "idle": 3}


def read_nice(pid: int) -> Optional[int]:
    """Niceness of `pid`, or None when it is gone or unreadable."""
    try:
        return os.getpriority(os.PRIO_PROCESS, pid)
    except (OSError, AttributeError):
        return None


def read_ionice_class(pid: int) -> Optional[int]:
    """I/O scheduling class of `pid` (0 none, 1 realtime, 2 best-effort, 3 idle)."""
    tool = shutil.which("ionice")
    if tool is None:
        return None
    try:
        out = subprocess.run(
            [tool, "-p", str(pid)],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            universal_newlines=True,
            check=True,
        ).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    head = out.strip().split(":", 1)[0].strip().lower()
    return _IONICE_NAMES.get(head)


def apply(pid: int, nice: int, ionice_class: int) -> str:
    """Lower `pid`'s CPU and I/O priority; return a one-line status, never raise."""
    parts = ["applied pid={}".format(pid)]
    before = read_nice(pid)
    if before is None:
        return "skipped pid={} reason=no_such_process".format(pid)
    target = max(before, min(nice, 19))
    if target == before:
        parts.append("nice={}(kept)".format(before))
    else:
        try:
            os.setpriority(os.PRIO_PROCESS, pid, target)
            parts.append("nice={}->{}".format(before, read_nice(pid)))
        except OSError as error:
            parts.append("nice={}(error:{})".format(before, error.errno))
    io_before = read_ionice_class(pid)
    if io_before is None:
        parts.append("ionice=unavailable")
    elif io_before in (ionice_class, IONICE_IDLE):
        # Already where requested, or already idle: never move a process out
        # of the idle class.
        parts.append("ionice={}(kept)".format(io_before))
    else:
        parts.append(_set_ionice(pid, io_before, ionice_class))
    return " ".join(parts)


def _set_ionice(pid: int, before: int, ionice_class: int) -> str:
    tool = shutil.which("ionice")
    if tool is None:
        return "ionice=unavailable"
    try:
        subprocess.run(
            [tool, "-c", str(ionice_class), "-p", str(pid)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return "ionice={}(error)".format(before)
    return "ionice={}->{}".format(before, read_ionice_class(pid))


def plan_jobs(cpus: int, load1: float) -> Tuple[int, int, int]:
    """Return (jobs, floor, ceiling) for a machine with `cpus` cores at `load1`.

    Headroom of an eighth of the cores (at least one) is never handed to a
    build. Under load the build gets the cores that are actually idle, but
    never fewer than the floor (at least two) so a busy machine still makes
    progress -- the low nice level, not starvation, keeps it out of the way.
    """
    cpus = max(1, int(cpus))
    headroom = max(1, cpus // 8)
    ceiling = max(1, cpus - headroom)
    floor = min(ceiling, max(2, cpus // 8))
    free = cpus - headroom - int(round(max(0.0, load1)))
    return max(floor, min(ceiling, free)), floor, ceiling


def current_load1() -> float:
    try:
        return os.getloadavg()[0]
    except (OSError, AttributeError):
        return 0.0


# --- Self-test ---------------------------------------------------------------


def _expect(condition: bool, message: str, failures: List[str]) -> None:
    if not condition:
        failures.append(message)


def _probe_make(env_overrides: dict) -> dict:
    """Run the Makefile's `build-priority-probe` target and parse its report."""
    env = dict(os.environ)
    for key in (
        "AGENT_DOC_BUILD_PRIORITY",
        "BUILD_NICE",
        "BUILD_IONICE_CLASS",
        "BUILD_JOBS",
        "CARGO_BUILD_JOBS",
        "NEXTEST_TEST_THREADS",
        "GRADLE_MAX_WORKERS",
        "TLA_JOBS",
        "TLA_WORKERS",
        "MAKEFLAGS",
        "MAKELEVEL",
        "MFLAGS",
    ):
        env.pop(key, None)
    env.update(env_overrides)
    out = subprocess.run(
        ["make", "--no-print-directory", "-s", "-C", str(ROOT), "build-priority-probe"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        universal_newlines=True,
        env=env,
        check=True,
    ).stdout
    report = {}
    for line in out.splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            report[key.strip()] = value.strip()
    return report


def self_test() -> int:
    failures: List[str] = []

    # Job sizing: idle machine keeps headroom, loaded machine drops to the floor.
    jobs, floor, ceiling = plan_jobs(32, 0.0)
    _expect((jobs, floor, ceiling) == (28, 4, 28), "idle 32-core plan {}".format((jobs, floor, ceiling)), failures)
    _expect(plan_jobs(32, 77.0)[0] == 4, "loaded 32-core machine must drop to the floor", failures)
    _expect(plan_jobs(32, 10.0)[0] == 18, "partially loaded 32-core machine gets the idle cores", failures)
    _expect(plan_jobs(1, 0.0)[0] == 1, "a single core still gets one job", failures)
    _expect(plan_jobs(4, 0.0)[0] == 3, "4 cores keep one core of headroom", failures)
    _expect(plan_jobs(4, 50.0)[0] == 2, "4 loaded cores keep two jobs of progress", failures)

    # apply(): lowers a child's priority and never raises it back.
    own = read_nice(os.getpid())
    child = subprocess.Popen(["sleep", "30"])
    try:
        status = apply(child.pid, 19, IONICE_IDLE)
        _expect(read_nice(child.pid) == 19, "apply must leave the child at nice 19 ({})".format(status), failures)
        io_class = read_ionice_class(child.pid)
        if io_class is not None:
            _expect(io_class == IONICE_IDLE, "apply must move the child to idle I/O ({})".format(status), failures)
        status = apply(child.pid, 5, IONICE_IDLE)
        _expect(read_nice(child.pid) == 19, "apply must never raise priority ({})".format(status), failures)
        _expect("(kept)" in status, "a no-op apply reports kept: {}".format(status), failures)
    finally:
        child.kill()
        child.wait()
    _expect(apply(2 ** 22 + 7, 19, IONICE_IDLE).startswith("skipped"), "a vanished pid is skipped", failures)

    # Makefile wiring: the default lowers make itself, and every recipe inherits it.
    if shutil.which("make") is not None:
        report = _probe_make({})
        _expect(report.get("priority", "").startswith("applied"), "default make must apply: {}".format(report), failures)
        _expect(
            "pid={}".format(report.get("make_pid")) in report.get("priority", ""),
            "apply must target the make process itself: {}".format(report),
            failures,
        )
        _expect(report.get("recipe_nice") == "19", "a recipe must run at nice 19: {}".format(report), failures)
        # The status line is the only discriminating proof when this self-test
        # already runs at nice 19 (as it does under `make check`, whose own make
        # was lowered first): a recipe inherits 19 then whether or not the
        # probe's make applied anything.
        _expect(
            re.search(r"\bnice=(?:-?\d+->19|19\(kept\))", report.get("priority", "")) is not None,
            "apply must report the make process at nice 19: {}".format(report),
            failures,
        )
        if own == 19:
            print("build-priority self-test: note: caller already at nice 19; recipe-nice checks rely on the apply status")
        io_class = report.get("recipe_ionice", "")
        if io_class not in ("", "unavailable"):
            _expect(io_class.startswith("idle"), "a recipe must run in the idle I/O class: {}".format(report), failures)
        for key in ("cargo_build_jobs", "nextest_test_threads", "gradle_max_workers", "tla_workers"):
            _expect(report.get(key, "").isdigit(), "{} must be exported as a number: {}".format(key, report), failures)
        _expect(
            "--priority=low" in report.get("gradle_flags", ""),
            "Gradle must run a low-priority daemon: {}".format(report),
            failures,
        )

        report = _probe_make({"AGENT_DOC_BUILD_PRIORITY": "normal"})
        _expect(report.get("priority") == "disabled", "normal must not touch priority: {}".format(report), failures)
        _expect(
            report.get("recipe_nice") == str(own),
            "normal must leave recipes at the caller's nice {}: {}".format(own, report),
            failures,
        )
        _expect(
            "--priority=low" not in report.get("gradle_flags", ""),
            "normal must not request a low-priority Gradle daemon: {}".format(report),
            failures,
        )

        report = _probe_make({"CARGO_BUILD_JOBS": "3", "GRADLE_MAX_WORKERS": "2"})
        _expect(report.get("cargo_build_jobs") == "3", "an explicit CARGO_BUILD_JOBS wins: {}".format(report), failures)
        _expect(report.get("gradle_max_workers") == "2", "an explicit GRADLE_MAX_WORKERS wins: {}".format(report), failures)

    if failures:
        for failure in failures:
            print("build-priority self-test FAILED: " + failure, file=sys.stderr)
        return 1
    print("build-priority self-test: ok")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--self-test", action="store_true", help="run the wiring and sizing regressions")
    sub = parser.add_subparsers(dest="command")
    apply_parser = sub.add_parser("apply", help="lower a process's CPU and I/O priority")
    apply_parser.add_argument("--pid", type=int, required=True)
    apply_parser.add_argument("--nice", type=int, default=19)
    apply_parser.add_argument("--ionice-class", type=int, default=IONICE_IDLE)
    jobs_parser = sub.add_parser("jobs", help="print a parallelism level that leaves headroom")
    jobs_parser.add_argument("--cpus", type=int, default=os.cpu_count() or 4)
    jobs_parser.add_argument("--load", type=float, default=None)
    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()
    if args.command == "apply":
        print(apply(args.pid, args.nice, args.ionice_class))
        return 0
    if args.command == "jobs":
        load = current_load1() if args.load is None else args.load
        print(plan_jobs(args.cpus, load)[0])
        return 0
    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
