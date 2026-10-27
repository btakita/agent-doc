#!/usr/bin/env python3
"""VS Code remote smoke test (`#vscoderemoteparity`, editors/SPEC.md section 13).

Run this ON the workspace host: a Coder workspace, a Remote-SSH host, or any
machine serving a code-server or VS Code Server workspace extension host. It
checks, in order:

1. install      `agent-doc plugin install vscode` reaches a workspace extension
                host (a `remote-ssh:<root>` or `code-server` target), and the
                installed manifest declares `"extensionKind": ["workspace"]`.
2. register     the controller holds a VS Code registration for the document
                whose activation receipt has `role=workspace`.
3. remote-host  the registered pid is a server-side extension host process
                (`--type=extensionHost` under a VS Code Server or code-server).
4. tmux-attach  a tmux client attached to the project session descends from
                the same server (the extension's `agent-doc` terminal).
5. remote-name  that client carries `AGENT_DOC_VSCODE_REMOTE_NAME`, the
                `vscode.env.remoteName` observation.
6. layout-sync  the controller accepted a surface observation from the editor
                that shows the document, and `reliable-sync-status --check`
                finds no divergence between visible documents and tmux panes.

Steps 2-6 need the operator: connect VS Code (Coder Remote, Remote-SSH, or a
code-server browser tab) to this host, open the document, and run an Agent Doc
command that attaches the IDE terminal. The script prints what it waits for
and polls until `--timeout`. Each step prints PASS, FAIL or SKIP; the exit
status is 1 when any step failed.

`--self-test` checks the parsing helpers without a workspace.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

RECEIPT_PREFIX = "agent_doc_activation_receipt/v1"
RECEIPT_KEYS = ("role", "jvm", "shell", "spi", "impl_version", "impl_sha256", "generation", "epoch")
REMOTE_NAME_ENV = "AGENT_DOC_VSCODE_REMOTE_NAME"
EXTENSION_ID = "btakita.agent-doc"
SERVER_ROOT_NAMES = (".vscode-server", ".vscode-server-insiders", ".vscodium-server", ".cursor-server")
STEPS = ("install", "register", "remote-host", "tmux-attach", "remote-name", "layout-sync")


# --- pure helpers (covered by --self-test) ---------------------------------


def parse_receipt(line: Optional[str]) -> Optional[Dict[str, str]]:
    """Fields of an activation receipt line, or None when it is not one."""
    if not line or not line.startswith(RECEIPT_PREFIX + " "):
        return None
    fields: Dict[str, str] = {}
    for field in line[len(RECEIPT_PREFIX) + 1 :].split(" "):
        key, sep, value = field.partition("=")
        if not sep or not key or not value or key in fields:
            return None
        fields[key] = value
    if any(key not in fields for key in RECEIPT_KEYS):
        return None
    return fields


def vscode_registration(status: dict, document: str) -> Optional[dict]:
    """The newest VS Code registration of `document` in a status response."""
    matches = [
        registration
        for registration in status.get("registrations", [])
        if registration.get("editor_kind") == "vscode"
        and os.path.realpath(registration.get("path", "")) == os.path.realpath(document)
    ]
    return max(matches, key=lambda registration: registration.get("timestamp_ms", 0), default=None)


def workspace_target_lines(plugin_list: str) -> List[str]:
    """`agent-doc plugin list` lines naming a workspace extension host."""
    return [
        line.strip()
        for line in plugin_list.splitlines()
        if line.startswith("vscode") and ("remote-ssh:" in line or line.rstrip().endswith("code-server"))
    ]


def is_server_extension_host(argv: List[str]) -> bool:
    """A VS Code Server / code-server extension host (the desktop one is an Electron utility)."""
    return "--type=extensionHost" in argv and server_root_of(argv) is not None


def server_root_of(argv: List[str]) -> Optional[str]:
    """The server install an argv runs from, or None for a desktop process."""
    for arg in argv:
        for name in SERVER_ROOT_NAMES:
            marker = "/" + name + "/"
            if marker in arg:
                return arg[: arg.index(marker) + len(marker) - 1]
        match = re.search(r"^(.*/code-server[^/]*)/(lib|out|bin)/", arg)
        if match:
            return match.group(1)
    return None


def project_tmux_session(config_text: str) -> Optional[str]:
    """Top-level `tmux_session` from `.agent-doc/config.toml`."""
    for line in config_text.splitlines():
        if line.strip().startswith("["):
            break
        match = re.match(r'\s*tmux_session\s*=\s*"([^"]+)"', line)
        if match:
            return match.group(1)
    return None


def environ_value(environ: bytes, key: str) -> Optional[str]:
    for entry in environ.split(b"\0"):
        name, sep, value = entry.partition(b"=")
        if sep and name.decode(errors="replace") == key:
            return value.decode(errors="replace")
    return None


# --- host access ------------------------------------------------------------


def proc_argv(pid: int) -> List[str]:
    try:
        raw = Path(f"/proc/{pid}/cmdline").read_bytes()
    except OSError:
        return []
    return [part.decode(errors="replace") for part in raw.split(b"\0") if part]


def proc_parent(pid: int) -> Optional[int]:
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return None
    fields = stat[stat.rindex(")") + 2 :].split()
    parent = int(fields[1])
    return parent if parent > 0 else None


def proc_ancestors(pid: int) -> List[int]:
    chain = []
    current: Optional[int] = pid
    while current and current not in chain and len(chain) < 64:
        chain.append(current)
        current = proc_parent(current)
    return chain


class Smoke:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.project = Path(args.project).resolve()
        self.document = str((self.project / args.doc).resolve())
        self.results: List[Tuple[str, str, str]] = []
        self.registration: Optional[dict] = None
        self.server_root: Optional[str] = None
        self.client_pid: Optional[int] = None

    def run(self, argv: List[str], check: bool = False) -> subprocess.CompletedProcess:
        return subprocess.run(
            argv, cwd=self.project, capture_output=True, text=True, check=check
        )

    def agent_doc(self, *argv: str) -> subprocess.CompletedProcess:
        return self.run([self.args.agent_doc, *argv])

    def record(self, step: str, verdict: str, detail: str) -> None:
        self.results.append((step, verdict, detail))
        print(f"{verdict:4} {step}: {detail}", flush=True)

    def wait_for(self, prompt: str, probe: Callable[[], Optional[str]]) -> Tuple[Optional[str], str]:
        """Poll `probe` until it returns a detail string or the timeout passes."""
        print(f"     waiting: {prompt}", flush=True)
        deadline = time.monotonic() + self.args.timeout
        last = "nothing observed"
        while True:
            try:
                detail = probe()
                if detail is not None:
                    return detail, last
            except RuntimeError as error:
                last = str(error)
            if time.monotonic() >= deadline:
                return None, last
            time.sleep(self.args.poll)

    def status(self) -> dict:
        result = self.agent_doc("reliable-sync-status", "--json", "--project-root", str(self.project))
        if result.returncode != 0:
            raise RuntimeError(f"reliable-sync-status exited {result.returncode}: {result.stderr.strip()}")
        return json.loads(result.stdout)

    # --- steps ---

    def step_install(self) -> None:
        if self.args.install != "skip":
            argv = ["plugin", "install", "vscode"] + (["--local"] if self.args.install == "local" else [])
            result = self.agent_doc(*argv)
            if result.returncode != 0:
                self.record("install", "FAIL", f"`agent-doc {' '.join(argv)}` exited {result.returncode}: {result.stderr.strip()}")
                return
        listing = self.agent_doc("plugin", "list").stdout
        lines = workspace_target_lines(listing)
        if not lines:
            self.record("install", "FAIL", "no workspace extension host lists the extension "
                        "(connect VS Code to this host once so its server is installed, or install code-server)")
            return
        manifests = self.installed_manifests()
        kinds = {tuple(json.loads(Path(m).read_text()).get("extensionKind") or ()) for m in manifests}
        if manifests and kinds != {("workspace",)}:
            self.record("install", "FAIL", f"installed manifests declare extensionKind {sorted(kinds)}: {manifests}")
            return
        self.record("install", "PASS", "; ".join(lines) + (f" ({len(manifests)} manifest(s) declare workspace)" if manifests else ""))

    def installed_manifests(self) -> List[str]:
        home = Path.home()
        roots = [os.environ.get("VSCODE_AGENT_FOLDER", "")] + [str(home / name) for name in SERVER_ROOT_NAMES]
        data_home = os.environ.get("XDG_DATA_HOME") or str(home / ".local/share")
        dirs = [os.path.join(root, "extensions") for root in roots if root]
        dirs.append(os.path.join(data_home, "code-server/extensions"))
        found: List[str] = []
        for directory in dirs:
            found.extend(sorted(glob.glob(os.path.join(directory, EXTENSION_ID + "-*", "package.json"))))
        return found

    def step_register(self) -> None:
        def probe() -> Optional[str]:
            registration = vscode_registration(self.status(), self.document)
            if registration is None:
                raise RuntimeError("no VS Code registration for the document")
            receipt = parse_receipt(registration.get("activation_receipt"))
            if receipt is None:
                raise RuntimeError(f"registration pid={registration.get('pid')} has no readable activation receipt "
                                   f"(extension {registration.get('editor_version')} predates receipts?)")
            if receipt["role"] != "workspace":
                raise RuntimeError(f"receipt role={receipt['role']}")
            self.registration = registration
            return (f"pid={registration['pid']} v{registration['editor_version']} "
                    f"generation={receipt['generation']} impl={receipt['impl_version']}")

        detail, last = self.wait_for(f"open {self.args.doc} in VS Code connected to this host "
                                     "(reload the window after an install)", probe)
        self.record("register", "PASS" if detail else "FAIL", detail or last)

    def step_remote_host(self) -> None:
        if not self.registration:
            self.record("remote-host", "SKIP", "no registration")
            return
        pid = int(self.registration["pid"])
        argv = proc_argv(pid)
        if not argv:
            self.record("remote-host", "FAIL", f"pid {pid} is not a process on this host")
            return
        if not is_server_extension_host(argv):
            self.record("remote-host", "FAIL", f"pid {pid} is not a server extension host: {' '.join(argv)[:200]}")
            return
        self.server_root = server_root_of(argv)
        self.record("remote-host", "PASS", f"pid {pid} is the workspace extension host of {self.server_root}")

    def step_tmux_attach(self) -> None:
        if not self.server_root:
            self.record("tmux-attach", "SKIP", "no server extension host")
            return
        config = self.project / ".agent-doc" / "config.toml"
        session = project_tmux_session(config.read_text()) if config.exists() else None
        if not session:
            self.record("tmux-attach", "FAIL", f"no tmux_session in {config} (`agent-doc session set <NAME>`)")
            return

        def probe() -> Optional[str]:
            listed = self.run(["tmux", "list-clients", "-t", session, "-F", "#{client_pid}"])
            if listed.returncode != 0:
                raise RuntimeError(f"tmux list-clients -t {session}: {listed.stderr.strip()}")
            for line in listed.stdout.split():
                client = int(line)
                for ancestor in proc_ancestors(client):
                    if server_root_of(proc_argv(ancestor)) == self.server_root:
                        self.client_pid = client
                        return f"client pid {client} on session {session} runs under {self.server_root}"
            raise RuntimeError(f"no client of {session} runs under {self.server_root}")

        detail, last = self.wait_for("run Agent Doc: Run or Claim so the extension attaches its agent-doc terminal", probe)
        self.record("tmux-attach", "PASS" if detail else "FAIL", detail or last)

    def step_remote_name(self) -> None:
        if not self.client_pid:
            self.record("remote-name", "SKIP", "no attached client")
            return
        try:
            environ = Path(f"/proc/{self.client_pid}/environ").read_bytes()
        except OSError as error:
            self.record("remote-name", "FAIL", f"cannot read the client's environment: {error}")
            return
        name = environ_value(environ, REMOTE_NAME_ENV)
        if not name:
            self.record("remote-name", "FAIL", f"{REMOTE_NAME_ENV} is unset in the client; close the agent-doc "
                        "terminal so the extension creates it again, then rerun")
        elif self.args.expect_remote_name and name != self.args.expect_remote_name:
            self.record("remote-name", "FAIL", f"remoteName={name}, expected {self.args.expect_remote_name}")
        else:
            self.record("remote-name", "PASS", f"remoteName={name}")

    def step_layout_sync(self) -> None:
        def probe() -> Optional[str]:
            observation = ((self.status().get("surface_sync") or {}).get("last_observation")) or {}
            visible = [os.path.realpath(path) for path in observation.get("visible", [])]
            if not observation.get("accepted") or os.path.realpath(self.document) not in visible:
                raise RuntimeError("no accepted surface observation shows the document")
            check = self.agent_doc("reliable-sync-status", "--check", "--project-root", str(self.project))
            if check.returncode != 0:
                raise RuntimeError(f"reliable-sync-status --check exited {check.returncode}: {check.stdout.strip()[-300:]}")
            return f"observation from {observation.get('client_id')} shows the document; no pane divergence"

        detail, last = self.wait_for("keep the document visible; run Agent Doc: Sync Layout", probe)
        self.record("layout-sync", "PASS" if detail else "FAIL", detail or last)

    def main(self) -> int:
        selected = [step.strip() for step in self.args.steps.split(",") if step.strip()]
        unknown = [step for step in selected if step not in STEPS]
        if unknown:
            print(f"unknown steps: {unknown}; known: {', '.join(STEPS)}", file=sys.stderr)
            return 2
        for step in STEPS:
            if step in selected:
                getattr(self, "step_" + step.replace("-", "_"))()
        failed = [step for step, verdict, _ in self.results if verdict == "FAIL"]
        print(f"vscode-remote-smoke: {len(self.results) - len(failed)} of {len(self.results)} steps did not fail"
              + (f"; failed: {', '.join(failed)}" if failed else ""))
        return 1 if failed else 0


# --- self-test --------------------------------------------------------------


def self_test() -> int:
    line = ("agent_doc_activation_receipt/v1 role=workspace jvm=6f1c shell=ab12 spi=0 "
            "impl_version=0.35.500 impl_sha256=cd34 generation=2 epoch=2")
    receipt = parse_receipt(line)
    assert receipt and receipt["role"] == "workspace" and receipt["generation"] == "2", receipt
    assert parse_receipt(line.replace(" epoch=2", "")) is None
    assert parse_receipt(line + " role=backend") is None
    assert parse_receipt("agent_doc_activation_receipt/v2 role=workspace") is None
    assert parse_receipt(None) is None

    status = {"registrations": [
        {"editor_kind": "jetbrains", "path": "/w/a.md", "timestamp_ms": 9},
        {"editor_kind": "vscode", "path": "/w/a.md", "timestamp_ms": 1, "pid": 1},
        {"editor_kind": "vscode", "path": "/w/a.md", "timestamp_ms": 5, "pid": 2},
        {"editor_kind": "vscode", "path": "/w/b.md", "timestamp_ms": 7, "pid": 3},
    ]}
    assert vscode_registration(status, "/w/a.md")["pid"] == 2
    assert vscode_registration(status, "/w/c.md") is None

    listing = ("jetbrains  0.2.900  /x\nvscode     btakita.agent-doc@0.2.84\n"
               "vscode     btakita.agent-doc@0.2.84  remote-ssh:/home/u/.vscode-server\n"
               "vscode     btakita.agent-doc@0.2.84  code-server\n")
    assert workspace_target_lines(listing) == [
        "vscode     btakita.agent-doc@0.2.84  remote-ssh:/home/u/.vscode-server",
        "vscode     btakita.agent-doc@0.2.84  code-server",
    ]

    remote = ["/home/u/.vscode-server/bin/abc/node", "/home/u/.vscode-server/bin/abc/out/bootstrap-fork",
              "--type=extensionHost", "--transformURIs"]
    assert is_server_extension_host(remote) and server_root_of(remote) == "/home/u/.vscode-server"
    code_server = ["/usr/lib/code-server/lib/node", "/usr/lib/code-server/out/bootstrap-fork", "--type=extensionHost"]
    assert is_server_extension_host(code_server) and server_root_of(code_server) == "/usr/lib/code-server"
    desktop = ["/usr/share/code/code", "--type=utility", "--utility-sub-type=node.mojom.NodeService"]
    assert not is_server_extension_host(desktop) and server_root_of(desktop) is None
    pty = ["/home/u/.vscode-server/bin/abc/node", "/home/u/.vscode-server/bin/abc/out/bootstrap-fork", "--type=ptyHost"]
    assert not is_server_extension_host(pty) and server_root_of(pty) == "/home/u/.vscode-server"

    assert project_tmux_session('tmux_session = "work"\n[terminal]\ntmux_session = "no"\n') == "work"
    assert project_tmux_session('[terminal]\ntmux_session = "no"\n') is None
    assert environ_value(b"A=1\0AGENT_DOC_VSCODE_REMOTE_NAME=ssh-remote\0", REMOTE_NAME_ENV) == "ssh-remote"
    assert environ_value(b"A=1\0", REMOTE_NAME_ENV) is None
    print("vscode-remote-smoke self-test: ok")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--self-test", action="store_true", help="check the parsing helpers and exit")
    parser.add_argument("--project", default=".", help="project root on this host (default: .)")
    parser.add_argument("--doc", help="session document, relative to --project")
    parser.add_argument("--install", choices=("release", "local", "skip"), default="release",
                        help="install the extension from the latest release, the local build, or not at all")
    parser.add_argument("--steps", default=",".join(STEPS), help="comma-separated subset of " + ",".join(STEPS))
    parser.add_argument("--timeout", type=float, default=300.0, help="seconds to wait for each operator step")
    parser.add_argument("--poll", type=float, default=1.0, help="seconds between polls")
    parser.add_argument("--expect-remote-name", default="", help="require this vscode.env.remoteName")
    parser.add_argument("--agent-doc", default="agent-doc", help="agent-doc binary (default: agent-doc on PATH)")
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    if not args.doc:
        parser.error("--doc is required")
    return Smoke(args).main()


if __name__ == "__main__":
    sys.exit(main())
