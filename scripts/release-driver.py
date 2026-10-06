#!/usr/bin/env python3
"""Run and report the irreversible and local phases of an agent-doc release."""

from __future__ import annotations

import argparse
import io
import re
import subprocess
import sys
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TextIO


ROOT = Path(__file__).resolve().parent.parent
VERSION_RE = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?\Z")


@dataclass(frozen=True)
class PhaseResult:
    name: str
    status: str
    elapsed_seconds: float
    returncode: int


Runner = Callable[[Sequence[str]], int]
Clock = Callable[[], float]


def subprocess_runner(command: Sequence[str]) -> int:
    return subprocess.run(command, cwd=ROOT, check=False).returncode


def timed_phase(
    name: str,
    commands: Sequence[Sequence[str]],
    *,
    runner: Runner,
    clock: Clock,
    stream: TextIO,
) -> PhaseResult:
    started = clock()
    returncode = 0
    for command in commands:
        returncode = runner(command)
        if returncode != 0:
            break
    elapsed = clock() - started
    status = "complete" if returncode == 0 else "failed"
    print(
        f"release phase {name}: {status} elapsed={elapsed:.3f}s",
        file=stream,
        flush=True,
    )
    return PhaseResult(name, status, elapsed, returncode)


def run_release(
    version: str,
    *,
    make_command: str = "make",
    runner: Runner = subprocess_runner,
    clock: Clock = time.monotonic,
    stream: TextIO = sys.stdout,
) -> int:
    if not VERSION_RE.fullmatch(version):
        print(f"release: invalid version {version!r}", file=sys.stderr)
        return 2

    tag = f"v{version}"
    publish = timed_phase(
        "tag-publish-handoff",
        (["git", "tag", tag], ["git", "push", "origin", "main", tag]),
        runner=runner,
        clock=clock,
        stream=stream,
    )
    if publish.returncode != 0:
        print(
            "release summary: tag_publish_handoff=failed local_install_full=not_started",
            file=stream,
            flush=True,
        )
        return publish.returncode

    install = timed_phase(
        "local-install-full",
        ([make_command, "install-full"],),
        runner=runner,
        clock=clock,
        stream=stream,
    )
    print(
        "release summary: "
        f"tag_publish_handoff=complete local_install_full={install.status}",
        file=stream,
        flush=True,
    )
    return install.returncode


def self_test() -> None:
    commands: list[list[str]] = []
    times = iter((10.0, 12.25, 20.0, 25.5))
    output = io.StringIO()

    def success(command: Sequence[str]) -> int:
        commands.append(list(command))
        return 0

    assert run_release(
        "1.2.3",
        make_command="gmake",
        runner=success,
        clock=lambda: next(times),
        stream=output,
    ) == 0
    assert commands == [
        ["git", "tag", "v1.2.3"],
        ["git", "push", "origin", "main", "v1.2.3"],
        ["gmake", "install-full"],
    ]
    assert output.getvalue().splitlines() == [
        "release phase tag-publish-handoff: complete elapsed=2.250s",
        "release phase local-install-full: complete elapsed=5.500s",
        "release summary: tag_publish_handoff=complete local_install_full=complete",
    ]

    commands.clear()
    times = iter((1.0, 2.0, 3.0, 7.0))
    output = io.StringIO()

    def install_fails(command: Sequence[str]) -> int:
        commands.append(list(command))
        return 23 if list(command) == ["make", "install-full"] else 0

    assert run_release(
        "1.2.4",
        runner=install_fails,
        clock=lambda: next(times),
        stream=output,
    ) == 23
    assert "tag_publish_handoff=complete local_install_full=failed" in output.getvalue()
    assert commands[-1] == ["make", "install-full"]

    commands.clear()
    times = iter((4.0, 6.0))
    output = io.StringIO()

    def push_fails(command: Sequence[str]) -> int:
        commands.append(list(command))
        return 17 if list(command)[:2] == ["git", "push"] else 0

    assert run_release(
        "1.2.5",
        runner=push_fails,
        clock=lambda: next(times),
        stream=output,
    ) == 17
    assert commands == [
        ["git", "tag", "v1.2.5"],
        ["git", "push", "origin", "main", "v1.2.5"],
    ]
    assert "tag_publish_handoff=failed local_install_full=not_started" in output.getvalue()

    assert run_release("not-a-version", runner=success) == 2
    print("release-driver self-test: ok")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version")
    parser.add_argument("--make", default="make", dest="make_command")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.version is None:
        parser.error("--version is required unless --self-test is used")
    return run_release(args.version, make_command=args.make_command)


if __name__ == "__main__":
    raise SystemExit(main())
