#!/usr/bin/env python3
"""VS Code remote smoke test (`#vscoderemoteparity`, editors/SPEC.md section 13).

Run this ON the workspace host: a Coder workspace, a Remote-SSH host, or any
machine serving a code-server or VS Code Server workspace extension host. It
checks, in order:

1. install      `agent-doc plugin install vscode` reaches a workspace extension
                host (a `remote-ssh:<root>`, `code-server` or `vscode-web:<cli>`
                target), and the
                installed manifest declares `"extensionKind": ["workspace"]`.
2. register     the controller holds a VS Code registration for the document
                whose activation receipt has `role=workspace`.
3. remote-host  the registered pid is a server-side extension host process
                (`--type=extensionHost` under a VS Code Server, Coder's
                vscode-web, or code-server).
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
status is 1 when any step failed. A step whose prerequisite failed is skipped
(install -> register -> remote-host -> tmux-attach -> remote-name, and
register -> layout-sync), so the first FAIL is the one to fix. A wait that
times out says whether any VS Code server extension host is running here,
which tells "nobody connected VS Code" apart from "the extension did not
register the document".

`--doc` must be an agent-doc session document: the extension registers only
those. The script asks the agent-doc library (`agent-doc lib-path`) the same
question the extension asks and exits 2 before any step when the answer is no.
Without an operator, only `--steps install` can pass.

`--self-test` checks the parsing helpers without a workspace.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import shutil
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
# The step each step needs to have passed, when both were selected.
PREREQUISITE = {
    "register": "install",
    "remote-host": "register",
    "tmux-attach": "remote-host",
    "remote-name": "tmux-attach",
    "layout-sync": "register",
}
# code-server installs off PATH, as `agent-doc plugin install vscode` probes them
# (src/plugin.rs CODE_SERVER_OFF_PATH_CLIS): relative entries are under $HOME.
# Coder's code-server module installs into /tmp/code-server by default.
CODE_SERVER_OFF_PATH_CLIS = (".local/bin/code-server", "/tmp/code-server/bin/code-server")
# Coder's vscode-web module extracts the VS Code Server web build into
# /tmp/vscode-web by default (src/plugin.rs VSCODE_WEB_CLIS, `#vscodewebprefix`);
# its README's custom prefix example is ~/.vscode-web.
VSCODE_WEB_CLIS = ("/tmp/vscode-web/bin/code-server", ".vscode-web/bin/code-server")
SERVER_CLI_NAMES = ("code-server", "code-server-insiders", "codium-server", "cursor-server")
SESSION_DOC_PROBE = (
    "import ctypes, sys\n"
    "lib = ctypes.CDLL(sys.argv[1])\n"
    "probe = lib.agent_doc_is_session_document\n"
    "probe.argtypes = [ctypes.c_char_p]\n"
    "probe.restype = ctypes.c_int\n"
    "print(probe(sys.argv[2].encode()))\n"
)


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
        if line.startswith("vscode")
        and ("remote-ssh:" in line or "code-server:" in line or "vscode-web:" in line
             or line.rstrip().endswith("code-server"))
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
        match = re.search(r"^(.*/(?:code-server|\.?vscode-web)[^/]*)/(lib|out|bin)/", arg)
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


def prerequisite_failure(step: str, results: List[Tuple[str, str, str]]) -> Optional[str]:
    """Why `step` must be skipped: its prerequisite ran and did not pass."""
    needed = PREREQUISITE.get(step)
    for name, verdict, _ in results:
        if name == needed and verdict != "PASS":
            return f"{needed} did not pass"
    return None


def summary_line(results: List[Tuple[str, str, str]]) -> str:
    """One line counting each verdict; a skipped step is not a passed one."""
    def named(verdict: str) -> List[str]:
        return [step for step, seen, _ in results if seen == verdict]
    passed, skipped, failed = named("PASS"), named("SKIP"), named("FAIL")
    return (f"vscode-remote-smoke: {len(passed)} passed, {len(skipped)} skipped, {len(failed)} failed"
            f" of {len(results)} steps" + (f"; failed: {', '.join(failed)}" if failed else ""))


def install_failure_detail(hosts: List[str], install_mode: str) -> str:
    """Actionable detail for an install step that found no workspace listing."""
    if not hosts:
        return ("no workspace extension host is installed on this host: connect VS Code to it once "
                "(Coder Remote or Remote-SSH installs ~/.vscode-server), or install code-server")
    remedy = ("rerun without `--install skip`, or run `agent-doc plugin install vscode`"
              if install_mode == "skip"
              else "check the `agent-doc plugin install vscode` output above")
    return (f"the extension is not installed in any workspace extension host ({', '.join(hosts)}): "
            f"{remedy}, then reload the remote window")


def wait_failure_detail(last: str, running_hosts: List[Tuple[int, str]], what: str) -> str:
    """A timed-out operator wait, naming whether any VS Code is connected at all."""
    if not running_hosts:
        return (f"{last}; no VS Code server extension host is running on this host, so no VS Code "
                f"window is connected: connect VS Code to this workspace and {what}")
    hosts = ", ".join(f"pid {pid} under {root}" for pid, root in running_hosts)
    return (f"{last}; extension host(s) running ({hosts}): {what} in that window, "
            "and if it is already open check the Agent Doc output channel for an activation error")


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


def running_server_extension_hosts() -> List[Tuple[int, str]]:
    """(pid, server root) of every VS Code Server / code-server extension host here."""
    hosts = []
    for entry in Path("/proc").iterdir() if Path("/proc").is_dir() else []:
        if not entry.name.isdigit():
            continue
        argv = proc_argv(int(entry.name))
        if is_server_extension_host(argv):
            hosts.append((int(entry.name), server_root_of(argv) or "?"))
    return sorted(hosts)


def installed_workspace_hosts() -> List[str]:
    """Workspace extension hosts installed here, probed as `agent-doc plugin install vscode` does."""
    home = Path.home()
    hosts = []
    on_path = shutil.which("code-server")
    if on_path:
        hosts.append(f"code-server ({on_path})")
    else:
        for candidate in CODE_SERVER_OFF_PATH_CLIS:
            path = Path(candidate) if candidate.startswith("/") else home / candidate
            if os.access(path, os.X_OK) and path.is_file():
                hosts.append(f"code-server:{path}")
                break
    for candidate in VSCODE_WEB_CLIS:
        path = Path(candidate) if candidate.startswith("/") else home / candidate
        if os.access(path, os.X_OK) and path.is_file():
            hosts.append(f"vscode-web:{path}")
            break
    roots = [os.environ.get("VSCODE_AGENT_FOLDER", "")] + [str(home / name) for name in SERVER_ROOT_NAMES]
    seen = set()
    for root in roots:
        if not root or root in seen:
            continue
        seen.add(root)
        clis = [path for name in SERVER_CLI_NAMES
                for path in glob.glob(os.path.join(root, "bin", "*", "bin", name))
                + glob.glob(os.path.join(root, "cli", "servers", "*", "server", "bin", name))
                if os.access(path, os.X_OK)]
        if clis:
            hosts.append(f"remote-ssh:{root}")
    return hosts


def session_document_verdict(agent_doc: str, document: str) -> Tuple[Optional[bool], str]:
    """Whether the extension would treat `document` as a session document.

    Asks the agent-doc library the extension loads (`agent_doc_is_session_document`)
    in a child interpreter, so a library fault cannot take the smoke down. None
    when the question could not be asked.
    """
    try:
        lib = subprocess.run([agent_doc, "lib-path"], capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        return None, f"`{agent_doc} lib-path` did not run: {error}"
    library = lib.stdout.strip()
    if lib.returncode != 0 or not library:
        return None, f"`{agent_doc} lib-path` exited {lib.returncode}: {lib.stderr.strip()[-200:]}"
    try:
        probe = subprocess.run([sys.executable, "-I", "-c", SESSION_DOC_PROBE, library, document],
                               capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as error:
        return None, f"the session-document probe did not run: {error}"
    answer = probe.stdout.strip()
    if probe.returncode != 0 or answer not in ("0", "1"):
        return None, f"the session-document probe against {library} failed: {probe.stderr.strip()[-200:]}"
    return answer == "1", library


class Smoke:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.project = Path(args.project).resolve()
        self.document = str((self.project / (args.doc or "")).resolve())
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
            self.record("install", "FAIL", install_failure_detail(installed_workspace_hosts(), self.args.install))
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
        self.record("register", "PASS" if detail else "FAIL", detail or wait_failure_detail(
            last, running_server_extension_hosts(), f"open {self.args.doc}"))

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
        self.record("layout-sync", "PASS" if detail else "FAIL", detail or wait_failure_detail(
            last, running_server_extension_hosts(), f"keep {self.args.doc} visible and run Agent Doc: Sync Layout"))

    def document_preflight(self) -> Optional[str]:
        """Why the operator steps cannot pass for `--doc`, or None to go on."""
        if not os.path.isfile(self.document):
            return f"vscode-remote-smoke: --doc {self.args.doc}: no such file {self.document}"
        verdict, detail = session_document_verdict(self.args.agent_doc, self.document)
        if verdict is None:
            print(f"WARN document: cannot tell whether {self.args.doc} is a session document ({detail}); "
                  "continuing", flush=True)
            return None
        if not verdict:
            return (f"vscode-remote-smoke: --doc {self.args.doc} is not an agent-doc session document "
                    f"(asked {detail}), so the extension never registers it and register and "
                    "layout-sync cannot pass. Pass a document with agent-doc frontmatter (for example "
                    "`agent_doc_format: template`) or one matched by `[documents] include` in "
                    ".agent-doc/config.toml; `--steps install` needs no document.")
        return None

    def main(self) -> int:
        selected = [step.strip() for step in self.args.steps.split(",") if step.strip()]
        unknown = [step for step in selected if step not in STEPS]
        if unknown:
            print(f"unknown steps: {unknown}; known: {', '.join(STEPS)}", file=sys.stderr)
            return 2
        if any(step != "install" for step in selected):
            refusal = self.document_preflight()
            if refusal:
                print(refusal, file=sys.stderr)
                return 2
        for step in STEPS:
            if step in selected:
                skipped = prerequisite_failure(step, self.results)
                if skipped:
                    self.record(step, "SKIP", skipped)
                    continue
                getattr(self, "step_" + step.replace("-", "_"))()
        print(summary_line(self.results))
        return 1 if any(verdict == "FAIL" for _, verdict, _ in self.results) else 0


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
    coder = "vscode     btakita.agent-doc@0.2.84  code-server:/tmp/code-server/bin/code-server\n"
    assert workspace_target_lines(coder) == [coder.strip()]
    vscode_web = "vscode     btakita.agent-doc@0.2.84  vscode-web:/tmp/vscode-web/bin/code-server\n"
    assert workspace_target_lines(vscode_web) == [vscode_web.strip()]

    # The 2026-11-04 Coder run: install failed, so register must not wait and
    # remote-host onward must not run; layout-sync follows register.
    results = [("install", "FAIL", "x")]
    assert prerequisite_failure("register", results) == "install did not pass"
    assert prerequisite_failure("install", results) is None
    results = [("install", "PASS", ""), ("register", "FAIL", "x")]
    assert prerequisite_failure("register", results) is None
    assert prerequisite_failure("remote-host", results) == "register did not pass"
    assert prerequisite_failure("layout-sync", results) == "register did not pass"
    assert prerequisite_failure("layout-sync", [("install", "PASS", "")]) is None
    assert prerequisite_failure("tmux-attach", [("remote-host", "SKIP", "")]) == "remote-host did not pass"

    assert summary_line([("install", "FAIL", ""), ("register", "SKIP", ""), ("layout-sync", "SKIP", "")]) == (
        "vscode-remote-smoke: 0 passed, 2 skipped, 1 failed of 3 steps; failed: install")
    assert summary_line([("install", "PASS", "")]) == "vscode-remote-smoke: 1 passed, 0 skipped, 0 failed of 1 steps"

    no_host = install_failure_detail([], "skip")
    assert "no workspace extension host is installed" in no_host and ".vscode-server" in no_host, no_host
    skipped = install_failure_detail(["remote-ssh:/home/u/.vscode-server"], "skip")
    assert "not installed in any workspace extension host" in skipped, skipped
    assert "without `--install skip`" in skipped and "agent-doc plugin install vscode" in skipped, skipped
    assert "--install skip" not in install_failure_detail(["code-server (/usr/bin/code-server)"], "release")

    unattended = wait_failure_detail("no VS Code registration for the document", [], "open plan.md")
    assert "no VS Code window is connected" in unattended, unattended
    attended = wait_failure_detail("no VS Code registration for the document",
                                   [(42, "/home/u/.vscode-server")], "open plan.md")
    assert "pid 42 under /home/u/.vscode-server" in attended and "output channel" in attended, attended

    remote = ["/home/u/.vscode-server/bin/abc/node", "/home/u/.vscode-server/bin/abc/out/bootstrap-fork",
              "--type=extensionHost", "--transformURIs"]
    assert is_server_extension_host(remote) and server_root_of(remote) == "/home/u/.vscode-server"
    code_server = ["/usr/lib/code-server/lib/node", "/usr/lib/code-server/out/bootstrap-fork", "--type=extensionHost"]
    assert is_server_extension_host(code_server) and server_root_of(code_server) == "/usr/lib/code-server"
    # Coder's vscode-web: the VS Code Server web build extracted flat into its prefix.
    web = ["/tmp/vscode-web/node", "/tmp/vscode-web/out/bootstrap-fork", "--type=extensionHost"]
    assert is_server_extension_host(web) and server_root_of(web) == "/tmp/vscode-web"
    custom = ["/home/coder/.vscode-web/node", "/home/coder/.vscode-web/out/bootstrap-fork", "--type=extensionHost"]
    assert server_root_of(custom) == "/home/coder/.vscode-web"
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
    if not args.doc and any(step.strip() not in ("", "install") for step in args.steps.split(",")):
        parser.error("--doc is required unless --steps is install")
    return Smoke(args).main()


if __name__ == "__main__":
    sys.exit(main())
