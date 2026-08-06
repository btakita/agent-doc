"""Peer handles: a subprocess speaking the line-delimited JSON peer contract.

The orchestrator only ever sees `Peer`. Whether the process behind it is a Rust
example binary or the in-repo Python reference implementation is a registry
detail, which is what lets the harness be tested with no binding toolchains
installed at all.
"""
from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

PROTOCOL_VERSION = 1


class PeerError(RuntimeError):
    """A peer rejected a request, died, or answered unintelligibly.

    Carries the peer name so a red run names a culprit rather than a transcript
    step — the whole reason to run pairwise before running the full set.
    """

    def __init__(self, peer: str, message: str, stderr: str = "") -> None:
        detail = f"[{peer}] {message}"
        if stderr.strip():
            detail += f"\n--- {peer} stderr ---\n{stderr.rstrip()}"
        super().__init__(detail)
        self.peer = peer


@dataclass
class PeerSpec:
    """How to launch one binding's interop peer."""

    name: str
    command: list[str]
    cwd: Path | None = None
    env: dict[str, str] = field(default_factory=dict)


class Peer:
    """A live peer process.

    Requests are strictly synchronous: write a line, read a line. The contract
    forbids buffering for exactly this reason — an async peer would make delivery
    order a scheduling outcome, and delivery order is the thing the scenario is
    supposed to own.
    """

    def __init__(self, spec: PeerSpec, peer_id: int) -> None:
        self.name = spec.name
        self.peer_id = peer_id
        self._spec = spec
        self._proc: subprocess.Popen[str] | None = None
        self.binding_version: str | None = None

    def __enter__(self) -> "Peer":
        self.start()
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    def start(self) -> None:
        import os

        env = {**os.environ, **self._spec.env}
        try:
            self._proc = subprocess.Popen(
                self._spec.command,
                cwd=str(self._spec.cwd) if self._spec.cwd else None,
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
            )
        except FileNotFoundError as exc:
            raise PeerError(self.name, f"could not launch {self._spec.command!r}: {exc}") from exc

        reply = self.request({"cmd": "hello", "peer": self.peer_id, "protocol_version": PROTOCOL_VERSION})
        self.binding_version = f"{reply.get('binding', '?')} {reply.get('version', '?')}"

    def request(self, payload: dict[str, Any]) -> dict[str, Any]:
        proc = self._proc
        if proc is None or proc.stdin is None or proc.stdout is None:
            raise PeerError(self.name, "peer is not running")

        try:
            proc.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
            proc.stdin.flush()
        except BrokenPipeError as exc:
            raise PeerError(self.name, f"died before reading {payload.get('cmd')!r}", self._drain_stderr()) from exc

        line = proc.stdout.readline()
        if not line:
            raise PeerError(
                self.name,
                f"closed stdout without answering {payload.get('cmd')!r} (exit {proc.poll()})",
                self._drain_stderr(),
            )
        try:
            reply = json.loads(line)
        except json.JSONDecodeError as exc:
            raise PeerError(self.name, f"answered {payload.get('cmd')!r} with non-JSON: {line!r}", self._drain_stderr()) from exc

        if not isinstance(reply, dict):
            raise PeerError(self.name, f"answered {payload.get('cmd')!r} with {type(reply).__name__}, want object")
        if not reply.get("ok"):
            raise PeerError(self.name, f"rejected {payload.get('cmd')!r}: {reply.get('error', '<no error given>')}")
        return reply

    # -- contract commands ---------------------------------------------------

    def local_set(self, node: int, key: str | None, state: dict[str, Any], at: int) -> dict[str, Any]:
        reply = self.request({"cmd": "local_set", "node": node, "key": key, "state": state, "at": at})
        frame = reply.get("frame")
        if not isinstance(frame, dict):
            raise PeerError(self.name, "local_set returned no frame")
        return frame

    def deliver(self, frame: dict[str, Any], at: int) -> int:
        reply = self.request({"cmd": "deliver", "frame": frame, "at": at})
        applied = reply.get("applied")
        if not isinstance(applied, int):
            raise PeerError(self.name, f"deliver returned applied={applied!r}, want an integer")
        return applied

    def snapshot(self) -> list[dict[str, Any]]:
        reply = self.request({"cmd": "snapshot"})
        cells = reply.get("cells")
        if not isinstance(cells, list):
            raise PeerError(self.name, f"snapshot returned cells={cells!r}, want a list")
        return cells

    def close(self) -> None:
        proc = self._proc
        if proc is None:
            return
        try:
            if proc.poll() is None:
                self.request({"cmd": "bye"})
        except PeerError:
            pass  # closing down; a peer that will not say goodbye gets killed below
        finally:
            self._shutdown(proc)
            self._proc = None

    def _shutdown(self, proc: subprocess.Popen[str]) -> None:
        for stream in (proc.stdin, proc.stdout, proc.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)

    def _drain_stderr(self) -> str:
        proc = self._proc
        if proc is None or proc.stderr is None:
            return ""
        try:
            return proc.stderr.read() or ""
        except (OSError, ValueError):
            return ""
