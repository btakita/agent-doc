#!/usr/bin/env python3
"""Qualify a host for agent-doc's Zed integration (local or Zed remote development).

Zed remote development runs language servers on the remote host, from the
remote worktree's login-shell PATH. This script checks, on the target host,
what `zed-lsp` needs there:

  path        `agent-doc` resolves on the login-shell PATH Zed uses
  version     the target's `agent-doc --version` (and whether it matches local)
  lsp         `agent-doc zed-lsp` answers an LSP initialize/shutdown/exit exchange
  handoff     the binary validates an in-place binary-pickup handoff file
  controller  with --project, the project controller resolves on the target host
  extension   the agent-doc extension version this checkout ships (informational)

Without --remote it checks this machine. With --remote HOST it runs every
target-side check through `ssh HOST`. It never starts a controller, opens a
session document, or changes state on either host.

See runbooks/zed-remote.md for what this proves and what it does not.
"""

from __future__ import annotations

import argparse
import json
import re
import shlex
import subprocess
import sys
from pathlib import Path

HANDOFF_SCHEMA = "agent_doc_zed_lsp_handoff/v1"
HANDOFF_CHECK_ENV = "AGENT_DOC_ZED_LSP_HANDOFF_CHECK"
TIMEOUT_SECONDS = 60


class Target:
    """Run commands on this machine or, with `remote`, through ssh."""

    def __init__(self, remote: str | None):
        self.remote = remote

    @property
    def label(self) -> str:
        return f"remote:{self.remote}" if self.remote else "local"

    def shell(self, script: str, stdin: bytes | None = None) -> subprocess.CompletedProcess:
        # Zed resolves language servers from the worktree's login-shell
        # environment, so every target command runs in a login shell.
        if self.remote:
            argv = ["ssh", "-o", "BatchMode=yes", self.remote, f"sh -lc {shlex.quote(script)}"]
        else:
            argv = ["sh", "-lc", script]
        return subprocess.run(argv, input=stdin, capture_output=True, timeout=TIMEOUT_SECONDS)


def frame(message: dict) -> bytes:
    body = json.dumps(message).encode()
    return f"Content-Length: {len(body)}\r\n\r\n".encode() + body


def frames(stream: bytes) -> list[dict]:
    messages = []
    while stream:
        header, separator, rest = stream.partition(b"\r\n\r\n")
        if not separator:
            break
        match = re.search(rb"Content-Length:\s*(\d+)", header)
        if match is None:
            break
        length = int(match.group(1))
        messages.append(json.loads(rest[:length]))
        stream = rest[length:]
    return messages


def version_of(output: bytes) -> str | None:
    match = re.search(rb"(\d+\.\d+\.\d+)", output)
    return match.group(1).decode() if match else None


def check(target: Target, binary: str, project: str | None, local_version: str | None) -> list[dict]:
    results: list[dict] = []

    def record(name: str, ok: bool | None, detail: str) -> None:
        results.append({"check": name, "ok": ok, "detail": detail})

    found = target.shell(f"command -v {shlex.quote(binary)}")
    path = found.stdout.decode().strip()
    record("path", found.returncode == 0 and bool(path), path or found.stderr.decode().strip() or "not found")
    if found.returncode != 0 or not path:
        return results
    quoted = shlex.quote(path)

    reported = target.shell(f"{quoted} --version")
    target_version = version_of(reported.stdout)
    if target_version is None:
        record("version", False, reported.stderr.decode().strip() or "no version reported")
    elif local_version is None or target.remote is None:
        record("version", True, target_version)
    else:
        record(
            "version",
            target_version == local_version,
            f"target {target_version}, local {local_version}",
        )

    exchange = b"".join(
        frame(message)
        for message in (
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "method": "initialized", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "shutdown"},
            {"jsonrpc": "2.0", "method": "exit"},
        )
    )
    served = target.shell(f"{quoted} zed-lsp", stdin=exchange)
    replies = {reply.get("id"): reply for reply in frames(served.stdout)}
    server = replies.get(1, {}).get("result", {}).get("serverInfo", {})
    lsp_ok = (
        served.returncode == 0
        and server.get("name") == "agent-doc-zed-lsp"
        and 2 in replies
        and (target_version is None or server.get("version") == target_version)
    )
    record(
        "lsp",
        lsp_ok,
        f"serverInfo={server or None} exit={served.returncode}"
        + (f" stderr={served.stderr.decode().strip()!r}" if not lsp_ok else ""),
    )

    handoff = json.dumps(
        {
            "schema": HANDOFF_SCHEMA,
            "from_pid": 0,
            "from_version": "qualification",
            "next_request_id": 1,
            "shutdown_requested": False,
            "pending_input_b64": "",
            "documents": [],
        }
    )
    probe = target.shell(
        "file=$(mktemp) && trap 'rm -f \"$file\"' EXIT && cat > \"$file\" && "
        f"{HANDOFF_CHECK_ENV}=\"$file\" {quoted} zed-lsp </dev/null",
        stdin=handoff.encode(),
    )
    verdict = probe.stdout.decode().strip()
    record(
        "handoff",
        probe.returncode == 0 and verdict.startswith(f"{HANDOFF_SCHEMA} ok"),
        verdict or probe.stderr.decode().strip() or f"exit {probe.returncode}, no verdict",
    )

    if project:
        # Read-only: without --ensure, status never launches a controller.
        resolved = target.shell(f"cd {shlex.quote(project)} && pwd -P")
        expected_root = resolved.stdout.decode().strip()
        status = target.shell(f"{quoted} controller status --project-root {shlex.quote(project)}")
        try:
            report = json.loads(status.stdout)
        except json.JSONDecodeError:
            record("controller", False, status.stderr.decode().strip() or "no status JSON")
        else:
            root = report.get("project_root")
            socket = report.get("socket_path") or ""
            # The controller zed-lsp talks to must belong to this project on
            # this host; a walk-up to another root (e.g. $HOME) is a failure.
            record(
                "controller",
                status.returncode == 0
                and bool(expected_root)
                and root == expected_root
                and socket.startswith(expected_root + "/"),
                f"expected_root={expected_root or None} project_root={root} socket={socket} "
                f"active={report.get('active')}",
            )
    else:
        record("controller", None, "skipped: pass --project to check controller locality")
    return results


def shipped_extension_version(root: Path) -> str | None:
    manifest = root / "editors/zed/extension.toml"
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', manifest.read_text()) if manifest.exists() else None
    return match.group(1) if match else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--remote", metavar="SSH_HOST", help="check this ssh host instead of this machine")
    parser.add_argument("--project", metavar="PATH", help="project root on the target to check controller locality for")
    parser.add_argument("--binary", default="agent-doc", help="command name Zed resolves (default: agent-doc)")
    parser.add_argument("--json", action="store_true", help="print the results as JSON")
    args = parser.parse_args()

    local_version = None
    if args.remote:
        try:
            local = subprocess.run([args.binary, "--version"], capture_output=True, timeout=TIMEOUT_SECONDS)
            local_version = version_of(local.stdout)
        except OSError:
            local_version = None

    target = Target(args.remote)
    results = check(target, args.binary, args.project, local_version)
    extension = shipped_extension_version(Path(__file__).resolve().parent.parent)
    results.append(
        {
            "check": "extension",
            "ok": None,
            "detail": f"this checkout ships agent-doc Zed extension {extension}; Zed syncs the installed "
            "(or dev) extension to the remote itself, which this script cannot observe",
        }
    )

    failed = [result for result in results if result["ok"] is False]
    if args.json:
        print(json.dumps({"target": target.label, "results": results, "qualified": not failed}, indent=2))
    else:
        for result in results:
            mark = {True: "ok  ", False: "FAIL", None: "info"}[result["ok"]]
            print(f"[{mark}] {result['check']}: {result['detail']}")
        print(f"{target.label}: {'qualified' if not failed else 'NOT qualified'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
