#!/usr/bin/env python3
"""Fake-publisher and fresh-shell proof for the point-of-use publish credential plan.

Implements the "Static and non-publishing proof" section of
`tasks/software/plan-ambient-publish-token-point-of-use.md` as a runnable target.

**No real credential is ever resolved, printed, or rotated, and no real publisher
is ever invoked.** `passage`, `cargo`, `maturin`, `bun`, and `python -m twine` are
all replaced by recorders that emit a fixed nonsecret sentinel. Any value the
recorders observe is masked before it reaches a log or this program's output: only
the sentinel and an explicit nonsecret literal allowlist are ever written
verbatim.

Three phases:

1. **Static** — recompute each preimage SHA-256 recorded in the plan, and run
   `git apply --unidiff-zero --check` for every fenced patch in its named
   repository. Also detects whether the plan is already applied, by checking
   whether each patch reverse-applies.

2. **Fake publisher** — for each production call site, rebuild the *patched* file
   in a scratch checkout, extract the patched recipe lines, and run exactly those
   lines in a sandbox whose environment holds none of the six root aliases and
   none of `NPM_CONFIG_TOKEN`. This works before the operator applies the plan,
   which is the whole point: the proof must exist before the quiet window.

   - success mode: the recipe exits 0, the fake `passage` was asked for exactly
     the expected key, the fake publisher ran, and the publisher child saw
     *exactly* the expected point-of-use variable carrying the sentinel — no root
     alias, no `NPM_CONFIG_TOKEN`, and no credential on its command line.
   - failure and empty modes: the recipe exits nonzero and no fake publisher runs
     at all, proving the lookup fails closed instead of falling back to a
     credential store or an interactive prompt.
   - TestPyPI keeps its unpatched recipe and must see no production alias and no
     `TWINE_PASSWORD`.

3. **Fresh shell** — name-only state of the seven publish variables. The assertion
   is on the ignored root `.env`: which alias names are assigned and which are
   commented, read without ever evaluating the file, so it cannot resolve a
   credential. The plan's literal step-5 login-shell loop runs as corroboration,
   but a non-interactive shell does not fire mise's activation hook, so that probe
   is labelled `inconclusive` rather than counted as a pass — forcing activation
   would mean sourcing `.env` and resolving real credentials. Expectations follow
   the detected application state instead of assuming one.

`--self-test` runs negative controls proving the assertions can actually fail: the
unpatched recipe, a never-invoked publisher, a credential on the command line, a
leaked parent alias, and both fail-closed modes exiting 0 must each be reported as
a failure. Two of the controls are end-to-end with an obvious decoy value injected
past the parent scrubber, because on a machine whose environment already lacks the
aliases the scrub is a no-op and the child-side assertion would otherwise be
untested. `parent_env_names_removed` in the report says whether that was the case.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_PLAN = REPO_ROOT / "tasks" / "software" / "plan-ambient-publish-token-point-of-use.md"

SENTINEL = "proof-sentinel-not-a-credential"
SENTINEL_MASK = "<sentinel>"

# The six aliases the plan comments out in the ignored root `.env`.
ROOT_ALIASES = (
    "PYPI_PUBLISH_TOKEN",
    "UV_PUBLISH_TOKEN",
    "MATURIN_PYPI_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "NPM_PUBLISH_TOKEN",
    "NPM_TOKEN",
)
# Resolved only inside the publisher process, never ambient.
CHILD_ONLY = ("NPM_CONFIG_TOKEN",)
TWINE_VARS = ("TWINE_USERNAME", "TWINE_PASSWORD")
WATCHED = ROOT_ALIASES + CHILD_ONLY + TWINE_VARS
# The plan's own fresh-shell name list.
FRESH_SHELL_NAMES = ROOT_ALIASES + CHILD_ONLY

# Values safe to record verbatim because they are not credentials.
NONSECRET_LITERALS = frozenset({"__token__"})

MODES = ("success", "failure", "empty")

# Intentionally untracked operator files. Their absence (CI, a fresh clone) is a
# legitimate skip, not drift.
IGNORED_OPERATOR_FILES = frozenset({".env", "src/lazily-js/.env"})


class ProofFailure(Exception):
    """A proof assertion failed."""


# --------------------------------------------------------------------------- #
# Plan parsing
# --------------------------------------------------------------------------- #

HEADING_TO_REPO = {
    "Superproject root": ".",
    "src/module-harness": "src/module-harness",
    "src/corky": "src/corky",
    "src/agent-doc": "src/agent-doc",
    "src/lazily-py": "src/lazily-py",
    "src/lazily-js": "src/lazily-js",
    "src/ctx-core-dev": "src/ctx-core-dev",
}


@dataclass(frozen=True)
class FilePatch:
    repo: str
    path: str
    text: str

    @property
    def hunks(self) -> list[list[str]]:
        groups: list[list[str]] = []
        for line in self.text.split("\n"):
            if line.startswith("@@"):
                groups.append([])
            elif groups is not None and groups:
                groups[-1].append(line)
        return groups


def parse_preimages(plan_text: str) -> dict[str, str]:
    """Read the `| path | sha256 |` preimage table."""
    recorded: dict[str, str] = {}
    for line in plan_text.split("\n"):
        match = re.match(r"^\|\s*`([^`]+)`\s*\|\s*`([0-9a-f]{64})`\s*\|\s*$", line)
        if match:
            recorded[match.group(1)] = match.group(2)
    if not recorded:
        raise ProofFailure("plan has no recognizable preimage hash table")
    return recorded


def parse_patches(plan_text: str) -> list[FilePatch]:
    """Collect every fenced ```diff block, attributed to its `###` repository."""
    patches: list[FilePatch] = []
    repo: str | None = None
    fence: list[str] | None = None
    for line in plan_text.split("\n"):
        heading = re.match(r"^###\s+(.*?)\s*$", line)
        if heading is not None and fence is None:
            repo = HEADING_TO_REPO.get(heading.group(1).strip("`"))
            continue
        if line.strip() == "```diff":
            if repo is None:
                raise ProofFailure("a diff fence appears before any recognized `###` heading")
            fence = []
            continue
        if fence is not None and line.strip() == "```":
            for chunk in _split_files(fence):
                path = _patched_path(chunk)
                patches.append(FilePatch(repo=repo or ".", path=path, text="\n".join(chunk) + "\n"))
            fence = None
            continue
        if fence is not None:
            fence.append(line)
    if fence is not None:
        raise ProofFailure("plan has an unterminated ```diff fence")
    if not patches:
        raise ProofFailure("plan has no ```diff patch blocks")
    return patches


def _split_files(lines: list[str]) -> list[list[str]]:
    chunks: list[list[str]] = []
    for line in lines:
        if line.startswith("diff --git "):
            chunks.append([])
        if chunks:
            chunks[-1].append(line)
    if not chunks:
        raise ProofFailure("a diff fence contains no `diff --git` header")
    return chunks


def _patched_path(chunk: list[str]) -> str:
    for line in chunk:
        if line.startswith("+++ b/"):
            return line[len("+++ b/") :].strip()
    raise ProofFailure(f"patch chunk has no `+++ b/` path: {chunk[:2]}")


# --------------------------------------------------------------------------- #
# Static phase
# --------------------------------------------------------------------------- #


def repo_for_path(rel: str) -> str:
    parts = rel.split("/")
    return f"{parts[0]}/{parts[1]}" if len(parts) > 2 and parts[0] == "src" else "."


def repo_present(root: Path, repo: str) -> bool:
    """An uninitialized submodule is an existing but EMPTY directory."""
    if repo == ".":
        return True
    path = root / repo
    if not path.is_dir():
        return False
    return any(path.iterdir())


def absence_state(root: Path, rel: str) -> str:
    """Classify why `rel` is missing: a legitimate skip, or real drift."""
    if not repo_present(root, repo_for_path(rel)):
        return "repo_absent"
    if rel in IGNORED_OPERATOR_FILES:
        return "ignored_file_absent"
    return "missing"


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 16), b""):
            digest.update(block)
    return digest.hexdigest()


def git_apply_check(repo: Path, patch_text: str, *, reverse: bool = False) -> tuple[bool, str]:
    command = ["git", "apply", "--unidiff-zero", "--check"]
    if reverse:
        command.append("--reverse")
    command.append("-")
    completed = subprocess.run(
        command,
        cwd=repo,
        input=patch_text,
        capture_output=True,
        text=True,
    )
    return completed.returncode == 0, completed.stderr.strip()


def run_static_phase(plan_text: str, root: Path) -> dict[str, Any]:
    recorded = parse_preimages(plan_text)
    patches = parse_patches(plan_text)

    preimages: list[dict[str, Any]] = []
    failures: list[str] = []
    for rel, expected in sorted(recorded.items()):
        path = root / rel
        if not path.is_file():
            state = absence_state(root, rel)
            preimages.append({"path": rel, "state": state})
            if state == "missing":
                failures.append(f"preimage file is missing: {rel}")
            continue
        actual = sha256_of(path)
        state = "match" if actual == expected else "drifted"
        # Hashes are not secrets; file contents are never printed.
        preimages.append({"path": rel, "state": state, "recorded": expected, "actual": actual})
        if state == "drifted":
            failures.append(
                f"preimage drifted: {rel} recorded {expected[:12]}… but is {actual[:12]}…; "
                "refresh the plan's hash and hunk offsets before the operator applies it"
            )

    applies: list[dict[str, Any]] = []
    for patch in patches:
        rel = patch.path if patch.repo == "." else f"{patch.repo}/{patch.path}"
        if not (root / patch.repo / patch.path).is_file():
            state = absence_state(root, rel)
            applies.append({"repo": patch.repo, "path": patch.path, "state": state})
            if state == "missing":
                failures.append(f"patch target is missing: {rel}")
            continue
        repo = root / patch.repo
        forward, forward_error = git_apply_check(repo, patch.text)
        reverse, _ = git_apply_check(repo, patch.text, reverse=True)
        if forward:
            state = "appliable"
        elif reverse:
            state = "already_applied"
        else:
            state = "broken"
            failures.append(
                f"patch neither applies nor reverse-applies in {patch.repo}: {patch.path} "
                f"({forward_error.splitlines()[0] if forward_error else 'no detail'})"
            )
        applies.append({"repo": patch.repo, "path": patch.path, "state": state})

    states = {
        entry["state"]
        for entry in applies
        if entry["state"] not in {"repo_absent", "ignored_file_absent"}
    }
    if states == {"appliable"}:
        application = "unapplied"
    elif states == {"already_applied"}:
        application = "applied"
    else:
        application = "mixed"

    skipped = sorted(
        entry["path"] if entry["repo"] == "." else f"{entry['repo']}/{entry['path']}"
        for entry in applies
        if entry["state"] in {"repo_absent", "ignored_file_absent"}
    )
    return {
        "preimages": preimages,
        "patches": applies,
        "application_state": application,
        "skipped_absent": skipped,
        "failures": failures,
    }


# --------------------------------------------------------------------------- #
# Patched-recipe extraction
# --------------------------------------------------------------------------- #


def build_patched_file(root: Path, patch: FilePatch) -> str:
    """Apply the patch to a scratch copy and return the patched text."""
    source = root / patch.repo / patch.path
    if not source.is_file():
        rel = patch.path if patch.repo == "." else f"{patch.repo}/{patch.path}"
        raise ProofFailure(f"patch target is missing: {rel}")
    with tempfile.TemporaryDirectory() as tmp:
        scratch = Path(tmp)
        subprocess.run(["git", "init", "-q"], cwd=scratch, check=True, capture_output=True)
        target = scratch / patch.path
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        completed = subprocess.run(
            ["git", "apply", "--unidiff-zero", "-"],
            cwd=scratch,
            input=patch.text,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            reverse, _ = git_apply_check(root / patch.repo, patch.text, reverse=True)
            if reverse:
                return source.read_text(encoding="utf-8")  # already applied in the live tree
            raise ProofFailure(
                f"could not build the patched {patch.repo}/{patch.path}: {completed.stderr.strip()}"
            )
        return target.read_text(encoding="utf-8")


def make_recipes(text: str) -> dict[str, list[str]]:
    """Map each make target to its recipe lines (tab-prefixed)."""
    recipes: dict[str, list[str]] = {}
    current: str | None = None
    for line in text.split("\n"):
        if line.startswith("\t"):
            if current is not None:
                recipes[current].append(line)
            continue
        if not line.strip():
            continue  # a blank line does not end a recipe
        match = re.match(r"^([A-Za-z0-9_.][A-Za-z0-9_.\-]*)\s*:(?!=)", line)
        if match and not line.lstrip().startswith("#"):
            current = match.group(1)
            recipes.setdefault(current, [])
        else:
            current = None
    return recipes


def added_recipe_lines(hunk: list[str]) -> list[str]:
    return [line[1:] for line in hunk if line.startswith("+") and line[1:].startswith("\t")]


def added_shell_lines(hunk: list[str]) -> list[str]:
    return [line[1:] for line in hunk if line.startswith("+")]


def contiguous_index(haystack: list[str], needle: list[str]) -> int:
    if not needle:
        return -1
    for start in range(len(haystack) - len(needle) + 1):
        if haystack[start : start + len(needle)] == needle:
            return start
    return -1


def patched_recipe_for(root: Path, patch: FilePatch, target: str, must_contain: str) -> list[str]:
    """Return the patched recipe lines for `target`, proving they came from the patch."""
    patched = build_patched_file(root, patch)
    recipes = make_recipes(patched)
    if target not in recipes:
        raise ProofFailure(f"{patch.repo}/{patch.path} has no `{target}:` target after patching")
    recipe = recipes[target]
    for hunk in patch.hunks:
        added = added_recipe_lines(hunk)
        if added and contiguous_index(recipe, added) >= 0:
            if must_contain not in "\n".join(added):
                raise ProofFailure(
                    f"{patch.repo}/{patch.path} `{target}` patched lines do not mention "
                    f"{must_contain!r}; the plan's hunk no longer matches this call site"
                )
            return added
    raise ProofFailure(
        f"no hunk of {patch.repo}/{patch.path} lands contiguously inside `{target}`'s recipe; "
        "the plan's hunk offsets have drifted from the file"
    )


def unpatched_recipe_for(root: Path, repo: str, path: str, target: str) -> list[str]:
    recipes = make_recipes((root / repo / path).read_text(encoding="utf-8"))
    if target not in recipes:
        raise ProofFailure(f"{repo}/{path} has no `{target}:` target")
    return recipes[target]


# --------------------------------------------------------------------------- #
# Sandbox
# --------------------------------------------------------------------------- #

PASSAGE_FAKE = """#!/bin/sh
# Fake passage. Records the requested key NAME only and returns a fixed
# nonsecret sentinel. It never reads a real credential store.
printf '%s\\n' "$1" >> "$PROOF_PASSAGE_LOG"
case "$PROOF_PASSAGE_MODE" in
  failure) printf 'fake passage: lookup failed\\n' >&2; exit 1 ;;
  empty) exit 0 ;;
  *) printf '%s\\n' "$PROOF_SENTINEL" ;;
esac
"""

RECORDER = '''#!/usr/bin/env python3
"""Fake publisher. Records argv and watched env NAMES, masking every value."""
import json
import os
import sys

SENTINEL = os.environ["PROOF_SENTINEL"]
WATCHED = os.environ["PROOF_WATCHED"].split(",")
NONSECRET = set(os.environ["PROOF_NONSECRET"].split(","))


def mask(value):
    if value == SENTINEL:
        return "<sentinel>"
    if value in NONSECRET:
        return value
    return "<masked:%d>" % len(value)


name = os.environ.get("PROOF_PUBLISHER_NAME") or os.path.basename(sys.argv[0])
record = {
    "publisher": name,
    "argv": [mask(arg) for arg in sys.argv[1:]],
    "env": {key: mask(os.environ[key]) for key in WATCHED if key in os.environ},
}
with open(os.environ["PROOF_PUBLISHER_LOG"], "a", encoding="utf-8") as handle:
    handle.write(json.dumps(record, sort_keys=True) + "\\n")
'''

VENV_PYTHON_WRAPPER = """#!/bin/sh
# Temporary wrapper standing in for the project's absolute venv interpreter.
# PYTHONPATH already points at the fake `twine` package, so `-m twine` resolves
# to the recorder rather than a real publisher.
exec "$PROOF_REAL_PYTHON" "$@"
"""


@dataclass
class Sandbox:
    directory: Path
    bin_dir: Path
    passage_log: Path
    publisher_log: Path
    env: dict[str, str]

    def passage_keys(self) -> list[str]:
        if not self.passage_log.is_file():
            return []
        return [line for line in self.passage_log.read_text(encoding="utf-8").split("\n") if line]

    def publisher_records(self) -> list[dict[str, Any]]:
        if not self.publisher_log.is_file():
            return []
        return [
            json.loads(line)
            for line in self.publisher_log.read_text(encoding="utf-8").split("\n")
            if line.strip()
        ]


def parent_env_names_removed() -> list[str]:
    """Which watched names this process actually carries (so a no-op scrub is visible)."""
    return sorted(name for name in WATCHED if name in os.environ)


def scrubbed_parent_env() -> dict[str, str]:
    """The sandbox parent must carry none of the watched names.

    When this process already carries none, the scrub is a no-op — which is not a
    problem, but it does mean the child-side assertion is not *contrastively*
    demonstrated by this run. `parent_env_names_removed()` is reported so a reader
    can see that, and the self-test injects a decoy to prove the assertion is live.
    """
    env = {key: value for key, value in os.environ.items() if key not in WATCHED}
    leaked = sorted(name for name in WATCHED if name in env)
    if leaked:
        raise ProofFailure(f"sandbox parent environment still holds {leaked}")
    return env


def make_sandbox(
    directory: Path,
    mode: str,
    *,
    parent_env: dict[str, str] | None = None,
    allow_parent_leak: bool = False,
) -> Sandbox:
    directory.mkdir(parents=True, exist_ok=True)
    bin_dir = directory / "proof-bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    pythonpath = directory / "proof-pythonpath"
    (pythonpath / "twine").mkdir(parents=True, exist_ok=True)

    passage_log = directory / "passage.log"
    publisher_log = directory / "publisher.log"

    def write_exec(path: Path, body: str) -> None:
        path.write_text(body, encoding="utf-8")
        path.chmod(0o755)

    write_exec(bin_dir / "passage", PASSAGE_FAKE)
    for publisher in ("cargo", "maturin", "bun"):
        write_exec(bin_dir / publisher, RECORDER)
    (pythonpath / "twine" / "__init__.py").write_text("", encoding="utf-8")
    twine_main = (
        "import os\n"
        'os.environ["PROOF_PUBLISHER_NAME"] = "twine"\n'
        + "\n".join(RECORDER.split("\n")[1:])
    )
    (pythonpath / "twine" / "__main__.py").write_text(twine_main, encoding="utf-8")

    if parent_env is None:
        env = dict(scrubbed_parent_env())
    else:
        leaked = sorted(name for name in WATCHED if name in parent_env)
        if leaked and not allow_parent_leak:
            raise ProofFailure(f"sandbox parent environment still holds {leaked}")
        env = dict(parent_env)
    env["PATH"] = f"{bin_dir}:{env.get('PATH', '/usr/bin:/bin')}"
    env["PYTHONPATH"] = str(pythonpath) + (f":{env['PYTHONPATH']}" if env.get("PYTHONPATH") else "")
    env["PROOF_SENTINEL"] = SENTINEL
    env["PROOF_PASSAGE_MODE"] = mode
    env["PROOF_PASSAGE_LOG"] = str(passage_log)
    env["PROOF_PUBLISHER_LOG"] = str(publisher_log)
    env["PROOF_WATCHED"] = ",".join(WATCHED)
    env["PROOF_NONSECRET"] = ",".join(sorted(NONSECRET_LITERALS))
    env["PROOF_REAL_PYTHON"] = sys.executable
    return Sandbox(directory, bin_dir, passage_log, publisher_log, env)


def scaffold(sandbox: Sandbox, entries: Iterable[str]) -> None:
    for entry in entries:
        path = sandbox.directory / entry
        if entry.endswith("/"):
            path.mkdir(parents=True, exist_ok=True)
        elif entry.endswith("venv/bin/python"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(VENV_PYTHON_WRAPPER, encoding="utf-8")
            path.chmod(0o755)
        elif entry.endswith("venv/bin/maturin"):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(RECORDER, encoding="utf-8")
            path.chmod(0o755)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("", encoding="utf-8")


def run_make_recipe(sandbox: Sandbox, recipe: list[str]) -> subprocess.CompletedProcess[str]:
    makefile = sandbox.directory / "ProofMakefile"
    makefile.write_text(".PHONY: proof\nproof:\n" + "\n".join(recipe) + "\n", encoding="utf-8")
    return subprocess.run(
        ["make", "--no-print-directory", "-f", str(makefile), "proof"],
        cwd=sandbox.directory,
        env=sandbox.env,
        capture_output=True,
        text=True,
    )


SHELL_PREAMBLE = """#!/bin/sh
# Stands in for the surrounding publish--force.sh context so the patched
# credential block can run without a real package or registry.
NAME=proof-package
not_published() { printf '%s NOT PUBLISHED: %s\\n' "$NAME" "$1"; }
"""


def run_shell_block(sandbox: Sandbox, lines: list[str]) -> subprocess.CompletedProcess[str]:
    script = sandbox.directory / "proof-block.sh"
    body = "\n".join(line.expandtabs(1).lstrip() if line.startswith("\t") else line for line in lines)
    script.write_text(SHELL_PREAMBLE + body + "\n", encoding="utf-8")
    script.chmod(0o755)
    return subprocess.run(
        ["sh", str(script)],
        cwd=sandbox.directory,
        env=sandbox.env,
        capture_output=True,
        text=True,
    )


# --------------------------------------------------------------------------- #
# Call sites
# --------------------------------------------------------------------------- #


@dataclass(frozen=True)
class CallSite:
    name: str
    repo: str
    path: str
    target: str
    kind: str  # "make" | "shell"
    passage_key: str
    must_contain: str
    expect_env: dict[str, str]
    publisher: str
    publisher_calls: int = 1
    scaffold: tuple[str, ...] = ()
    patched: bool = True


CALL_SITES: tuple[CallSite, ...] = (
    CallSite(
        name="crates.io/root-instruction-files",
        repo=".",
        path="Makefile",
        target="publish",
        kind="make",
        passage_key="btak/CARGO_REGISTRY_TOKEN",
        must_contain="cargo publish",
        expect_env={"CARGO_REGISTRY_TOKEN": SENTINEL_MASK},
        publisher="cargo",
        scaffold=("src/instruction-files/",),
    ),
    CallSite(
        name="crates.io/module-harness",
        repo="src/module-harness",
        path="Makefile",
        target="publish",
        kind="make",
        passage_key="btak/CARGO_REGISTRY_TOKEN",
        must_contain="cargo publish",
        expect_env={"CARGO_REGISTRY_TOKEN": SENTINEL_MASK},
        publisher="cargo",
    ),
    CallSite(
        name="crates.io/corky-six-crates",
        repo="src/corky",
        path="Makefile",
        target="publish-crate",
        kind="make",
        passage_key="btak/CARGO_REGISTRY_TOKEN",
        must_contain="cargo publish -p corky",
        expect_env={"CARGO_REGISTRY_TOKEN": SENTINEL_MASK},
        publisher="cargo",
        publisher_calls=6,
    ),
    CallSite(
        name="pypi/corky-maturin",
        repo="src/corky",
        path="Makefile",
        target="publish-pypi",
        kind="make",
        passage_key="btak/PYPI_PUBLISH_TOKEN",
        must_contain="maturin publish",
        expect_env={"MATURIN_PYPI_TOKEN": SENTINEL_MASK},
        publisher="maturin",
        scaffold=(".venv/bin/maturin",),
    ),
    CallSite(
        name="pypi/agent-doc-venv-twine",
        repo="src/agent-doc",
        path="Makefile",
        target="publish-pypi",
        kind="make",
        passage_key="btak/PYPI_PUBLISH_TOKEN",
        must_contain="-m twine upload",
        expect_env={"TWINE_USERNAME": "__token__", "TWINE_PASSWORD": SENTINEL_MASK},
        publisher="twine",
        scaffold=(".venv/bin/python", "dist/proof-0.0.0-py3-none-any.whl"),
    ),
    CallSite(
        name="pypi/lazily-py-twine",
        repo="src/lazily-py",
        path="Makefile",
        target="publish",
        kind="make",
        passage_key="btak/PYPI_PUBLISH_TOKEN",
        must_contain="-m twine upload",
        expect_env={"TWINE_USERNAME": "__token__", "TWINE_PASSWORD": SENTINEL_MASK},
        publisher="twine",
        scaffold=("dist/proof-0.0.0-py3-none-any.whl",),
    ),
    CallSite(
        name="npm/ctx-core-dev-bun",
        repo="src/ctx-core-dev",
        path="lib/monorepo/bin/publish--force.sh",
        target="",
        kind="shell",
        passage_key="btak/GRANULAR_NPM_TOKEN",
        must_contain="bun publish",
        expect_env={"NPM_CONFIG_TOKEN": SENTINEL_MASK},
        publisher="bun",
    ),
    # TestPyPI keeps its unpatched recipe and must stay credential-free.
    CallSite(
        name="testpypi/lazily-py-unpatched",
        repo="src/lazily-py",
        path="Makefile",
        target="publish-test",
        kind="make",
        passage_key="",
        must_contain="testpypi",
        expect_env={},
        publisher="twine",
        scaffold=("dist/proof-0.0.0-py3-none-any.whl",),
        patched=False,
    ),
)


def recipe_for(site: CallSite, root: Path, patches: list[FilePatch]) -> list[str]:
    if not site.patched:
        recipe = unpatched_recipe_for(root, site.repo, site.path, site.target)
        if site.must_contain not in "\n".join(recipe):
            raise ProofFailure(
                f"{site.repo}/{site.path} `{site.target}` no longer mentions {site.must_contain!r}"
            )
        return recipe
    matching = [p for p in patches if p.repo == site.repo and p.path == site.path]
    if not matching:
        raise ProofFailure(f"the plan has no patch for {site.repo}/{site.path}")
    if site.kind == "shell":
        for hunk in matching[0].hunks:
            added = added_shell_lines(hunk)
            if site.must_contain in "\n".join(added):
                return added
        raise ProofFailure(
            f"no hunk of {site.repo}/{site.path} adds {site.must_contain!r}"
        )
    return patched_recipe_for(root, matching[0], site.target, site.must_contain)


def assert_success(site: CallSite, sandbox: Sandbox, result: subprocess.CompletedProcess[str]) -> None:
    if result.returncode != 0:
        raise ProofFailure(
            f"{site.name}: success mode exited {result.returncode}: {result.stderr.strip()[:400]}"
        )
    keys = sandbox.passage_keys()
    if site.passage_key:
        if keys != [site.passage_key]:
            raise ProofFailure(
                f"{site.name}: expected exactly one passage lookup for {site.passage_key!r}, got {keys}"
            )
    elif keys:
        raise ProofFailure(f"{site.name}: TestPyPI must resolve no credential, but asked for {keys}")

    records = sandbox.publisher_records()
    if len(records) != site.publisher_calls:
        raise ProofFailure(
            f"{site.name}: expected {site.publisher_calls} fake {site.publisher} invocation(s), "
            f"got {len(records)}"
        )
    for record in records:
        if record["publisher"] != site.publisher:
            raise ProofFailure(
                f"{site.name}: expected publisher {site.publisher!r}, recorded {record['publisher']!r}"
            )
        if record["env"] != site.expect_env:
            raise ProofFailure(
                f"{site.name}: publisher child env {record['env']} != expected {site.expect_env} — "
                "the child must see exactly the point-of-use variable and no root alias"
            )
        if SENTINEL_MASK in record["argv"]:
            raise ProofFailure(f"{site.name}: a credential reached the publisher command line")


def assert_fails_closed(
    site: CallSite, sandbox: Sandbox, result: subprocess.CompletedProcess[str], mode: str
) -> None:
    if result.returncode == 0:
        raise ProofFailure(f"{site.name}: {mode} mode exited 0; the lookup did not fail closed")
    records = sandbox.publisher_records()
    if records:
        raise ProofFailure(
            f"{site.name}: {mode} mode still invoked {len(records)} fake publisher(s); "
            "the recipe must refuse before the publisher starts"
        )


def exercise(
    site: CallSite,
    recipe: list[str],
    mode: str,
    workdir: Path,
    *,
    parent_env: dict[str, str] | None = None,
    allow_parent_leak: bool = False,
) -> tuple[Sandbox, subprocess.CompletedProcess[str]]:
    sandbox = make_sandbox(workdir, mode, parent_env=parent_env, allow_parent_leak=allow_parent_leak)
    scaffold(sandbox, site.scaffold)
    if site.kind == "shell":
        result = run_shell_block(sandbox, recipe)
    else:
        result = run_make_recipe(sandbox, recipe)
    return sandbox, result


def run_publisher_phase(root: Path, patches: list[FilePatch], modes: Iterable[str]) -> dict[str, Any]:
    results: list[dict[str, Any]] = []
    failures: list[str] = []
    passage_lookups = 0
    publisher_invocations = 0
    with tempfile.TemporaryDirectory(prefix="publish-credential-proof.") as tmp:
        base = Path(tmp)
        for site in CALL_SITES:
            if not repo_present(root, site.repo):
                results.append({"site": site.name, "state": "repo_absent"})
                continue
            try:
                recipe = recipe_for(site, root, patches)
            except ProofFailure as failure:
                failures.append(str(failure))
                results.append({"site": site.name, "state": "recipe_unavailable"})
                continue
            site_modes = modes if site.patched else ("success",)
            for mode in site_modes:
                workdir = base / f"{site.name.replace('/', '__')}.{mode}"
                sandbox: Sandbox | None = None
                try:
                    sandbox, result = exercise(site, recipe, mode, workdir)
                    if mode == "success":
                        assert_success(site, sandbox, result)
                    else:
                        assert_fails_closed(site, sandbox, result, mode)
                    results.append({"site": site.name, "mode": mode, "state": "ok"})
                except ProofFailure as failure:
                    failures.append(str(failure))
                    results.append({"site": site.name, "mode": mode, "state": "failed"})
                finally:
                    if sandbox is not None:
                        passage_lookups += len(sandbox.passage_keys())
                        publisher_invocations += len(sandbox.publisher_records())
    exercised = sorted({entry["site"] for entry in results if "mode" in entry})
    skipped = sorted({entry["site"] for entry in results if entry.get("state") == "repo_absent"})
    if not exercised:
        failures.append(
            "no call site was exercised at all; a proof that runs nothing must not report success"
        )
    return {
        "results": results,
        "exercised_sites": exercised,
        "skipped_absent_sites": skipped,
        "failures": failures,
        # Measured, not asserted: every lookup and every publisher invocation was
        # recorded by a fake inside the sandbox. A real `passage` or publisher
        # winning the PATH would leave these logs empty and fail the assertions
        # above, so a nonzero count is what proves the interception held.
        "fake_passage_lookups": passage_lookups,
        "fake_publisher_invocations": publisher_invocations,
    }


# --------------------------------------------------------------------------- #
# Fresh-shell phase
# --------------------------------------------------------------------------- #


def env_alias_name_state(root: Path) -> dict[str, list[str]]:
    """Name-only state of the six aliases in the ignored root `.env`.

    Reads assignment NAMES and whether each is commented. It never evaluates the
    file, never expands `$(passage ...)`, and never records a value, so it cannot
    resolve or expose a credential.
    """
    path = root / ".env"
    if not path.is_file():
        return {"live": [], "commented": [], "missing": ["/.env"]}
    live: set[str] = set()
    commented: set[str] = set()
    for raw in path.read_text(encoding="utf-8", errors="replace").split("\n"):
        line = raw.strip()
        for name in FRESH_SHELL_NAMES:
            if re.match(rf"^{re.escape(name)}\s*=", line):
                live.add(name)
            elif re.match(rf"^#\s*{re.escape(name)}\s*=", line):
                commented.add(name)
    return {"live": sorted(live), "commented": sorted(commented), "missing": []}


def login_shell_name_probe(root: Path) -> dict[str, Any]:
    """Run the plan's literal name-only loop in a fresh project login shell.

    A non-interactive `-l -c` shell does not fire mise's `precmd` activation hook,
    so this probe can legitimately observe nothing. That is reported as
    `inconclusive` rather than silently counted as a pass -- the alternative
    (sourcing `.env` to force activation) would resolve real credentials, which
    this program must never do.
    """
    shell = os.environ.get("SHELL") or "/bin/sh"
    names = " ".join(FRESH_SHELL_NAMES)
    script = (
        f"cd {root!s} || exit 90\n"
        f"for name in {names}; do\n"
        '  if env | cut -d= -f1 | grep -Fxq "$name"; then printf \'%s\\n\' "$name"; fi\n'
        "done\n"
    )
    completed = subprocess.run(
        [shell, "-l", "-c", script],
        env=scrubbed_parent_env(),
        capture_output=True,
        text=True,
        timeout=120,
    )
    present = sorted({line.strip() for line in completed.stdout.split("\n") if line.strip()})
    return {"shell": shell, "exit_code": completed.returncode, "present_names": present}


def run_fresh_shell_phase(root: Path, application_state: str) -> dict[str, Any]:
    """Assert on the resolution-free `.env` name state; corroborate with a shell probe."""
    alias_state = env_alias_name_state(root)
    probe = login_shell_name_probe(root)
    failures: list[str] = []

    if alias_state["missing"]:
        return {
            "application_state": application_state,
            "state": "skipped",
            "reason": (
                "the ignored root `.env` is not present (expected on CI and in a fresh clone), "
                "so there is no alias state to check; nothing is asserted rather than passing "
                "vacuously"
            ),
            "env_alias_names": alias_state,
            "failures": [],
        }
    if application_state == "applied":
        if alias_state["live"]:
            failures.append(
                "the plan reads as applied, but the root `.env` still assigns "
                f"{alias_state['live']} uncommented"
            )
        if probe["present_names"]:
            failures.append(
                "the plan is applied, but a fresh project shell still exports "
                f"{probe['present_names']}; close inherited shells and recheck"
            )
    elif application_state == "unapplied":
        if not alias_state["live"]:
            failures.append(
                "the plan is unapplied, so the root `.env` should still assign the aliases, but "
                "none were found uncommented — the alias-state check is observing the wrong file "
                "and would pass vacuously after application"
            )
        if alias_state["commented"]:
            failures.append(
                "the plan is unapplied, yet the root `.env` already comments "
                f"{alias_state['commented']}; the `.env` and tracked patches are out of step"
            )

    if probe["exit_code"] == 90:
        failures.append(f"fresh shell could not enter the project root {root}")

    # Absence in a non-interactive shell proves nothing while the aliases are live.
    conclusive = bool(probe["present_names"]) or application_state == "applied"
    probe["state"] = "conclusive" if conclusive else "inconclusive"
    if not conclusive:
        probe["reason"] = (
            "a non-interactive login shell does not run mise's activation hook, and sourcing "
            "`.env` to force it would resolve real credentials; the `.env` name-state assertion "
            "carries this phase instead. After application, rerun the plan's step-5 loop by hand "
            "in a real interactive shell."
        )

    return {
        "application_state": application_state,
        "env_alias_names": alias_state,
        "login_shell_probe": probe,
        "expectation": "absent" if application_state == "applied" else "present",
        "failures": failures,
    }


# --------------------------------------------------------------------------- #
# Orchestration
# --------------------------------------------------------------------------- #


def run(args: argparse.Namespace) -> int:
    root = args.root.resolve()
    plan_text = args.plan.read_text(encoding="utf-8")

    report: dict[str, Any] = {"plan": str(args.plan), "root": str(root)}
    static = run_static_phase(plan_text, root)
    report["static"] = static
    failures = list(static["failures"])

    if not args.static_only:
        patches = parse_patches(plan_text)
        modes = MODES if args.mode == "all" else (args.mode,)
        publisher = run_publisher_phase(root, patches, modes)
        report["fake_publisher"] = publisher
        failures.extend(publisher["failures"])

        if not args.skip_fresh_shell:
            fresh = run_fresh_shell_phase(root, static["application_state"])
            report["fresh_shell"] = fresh
            failures.extend(fresh["failures"])

    report["parent_env_names_removed"] = parent_env_names_removed()
    report["failure_count"] = len(failures)
    report["failures"] = failures
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    if args.report_path:
        args.report_path.parent.mkdir(parents=True, exist_ok=True)
        args.report_path.write_text(rendered + "\n", encoding="utf-8")
    if failures:
        print(f"[proof] {len(failures)} failure(s)", file=sys.stderr)
        for failure in failures:
            print(f"[proof]   {failure}", file=sys.stderr)
        return 1
    print("[proof] point-of-use publish credential proof: ok", file=sys.stderr)
    return 0


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--plan", type=Path, default=DEFAULT_PLAN)
    parser.add_argument("--root", type=Path, default=REPO_ROOT)
    parser.add_argument("--mode", choices=(*MODES, "all"), default="all")
    parser.add_argument("--static-only", action="store_true", help="run only the hash/apply checks")
    parser.add_argument("--skip-fresh-shell", action="store_true")
    parser.add_argument("--report-path", type=Path)
    parser.add_argument("--self-test", action="store_true", help="run the negative controls")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.self_test:
        return self_test()
    try:
        return run(args)
    except ProofFailure as failure:
        print(f"[proof] failed: {failure}", file=sys.stderr)
        return 2


# --------------------------------------------------------------------------- #
# Negative controls: prove each assertion can fail.
# --------------------------------------------------------------------------- #


def _expect_failure(label: str, thunk: Any, expected: str) -> None:
    try:
        thunk()
    except ProofFailure as failure:
        assert expected in str(failure), f"{label}: expected {expected!r} in {failure}"
        return
    raise AssertionError(f"{label}: the assertion passed when it should have failed")


def self_test() -> int:
    """Validate the detector before trusting any green result."""
    root = REPO_ROOT
    plan_text = DEFAULT_PLAN.read_text(encoding="utf-8")
    patches = parse_patches(plan_text)

    # The plan parser must find every repository's patch.
    repos = {patch.repo for patch in patches}
    for required in (
        ".",
        "src/module-harness",
        "src/corky",
        "src/agent-doc",
        "src/lazily-py",
        "src/lazily-js",
        "src/ctx-core-dev",
    ):
        assert required in repos, f"plan parse lost {required}: {sorted(repos)}"

    twine_site = next(site for site in CALL_SITES if site.name == "pypi/lazily-py-twine")
    cargo_site = next(site for site in CALL_SITES if site.name == "crates.io/module-harness")

    with tempfile.TemporaryDirectory(prefix="publish-credential-proof-selftest.") as tmp:
        base = Path(tmp)

        # Control A: the UNPATCHED recipe must be rejected in success mode. If this
        # passes, the whole proof is vacuous -- it would green the very state the
        # plan exists to change.
        unpatched = unpatched_recipe_for(root, cargo_site.repo, cargo_site.path, cargo_site.target)
        sandbox, result = exercise(cargo_site, unpatched, "success", base / "unpatched")
        _expect_failure(
            "unpatched recipe",
            lambda: assert_success(cargo_site, sandbox, result),
            "passage lookup",
        )

        # Control B: the patched recipe must pass the same assertion.
        patched = recipe_for(cargo_site, root, patches)
        sandbox, result = exercise(cargo_site, patched, "success", base / "patched")
        assert_success(cargo_site, sandbox, result)

        # Control C: a publisher that never runs must fail, so "no output" can
        # never read as success.
        empty_sandbox = make_sandbox(base / "never-ran", "success")
        empty_sandbox.passage_log.write_text(cargo_site.passage_key + "\n", encoding="utf-8")
        _expect_failure(
            "publisher never invoked",
            lambda: assert_success(
                cargo_site,
                empty_sandbox,
                subprocess.CompletedProcess([], 0, "", ""),
            ),
            "expected 1 fake cargo invocation(s), got 0",
        )

        # Control D: a credential on the command line must fail.
        argv_sandbox = make_sandbox(base / "argv-leak", "success")
        argv_sandbox.passage_log.write_text(cargo_site.passage_key + "\n", encoding="utf-8")
        argv_sandbox.publisher_log.write_text(
            json.dumps(
                {
                    "publisher": "cargo",
                    "argv": ["publish", SENTINEL_MASK],
                    "env": {"CARGO_REGISTRY_TOKEN": SENTINEL_MASK},
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        _expect_failure(
            "credential in argv",
            lambda: assert_success(
                cargo_site, argv_sandbox, subprocess.CompletedProcess([], 0, "", "")
            ),
            "reached the publisher command line",
        )

        # Control E: an extra inherited alias in the child must fail.
        leak_sandbox = make_sandbox(base / "env-leak", "success")
        leak_sandbox.passage_log.write_text(cargo_site.passage_key + "\n", encoding="utf-8")
        leak_sandbox.publisher_log.write_text(
            json.dumps(
                {
                    "publisher": "cargo",
                    "argv": ["publish"],
                    "env": {
                        "CARGO_REGISTRY_TOKEN": SENTINEL_MASK,
                        "NPM_TOKEN": "<masked:40>",
                    },
                },
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )
        _expect_failure(
            "inherited alias leak",
            lambda: assert_success(
                cargo_site, leak_sandbox, subprocess.CompletedProcess([], 0, "", "")
            ),
            "no root alias",
        )

        # Control F: failure and empty modes must fail closed, and a publisher that
        # ran anyway must be reported.
        for mode in ("failure", "empty"):
            sandbox, result = exercise(cargo_site, patched, mode, base / f"closed-{mode}")
            assert_fails_closed(cargo_site, sandbox, result, mode)
            _expect_failure(
                f"{mode} mode exiting 0",
                lambda s=sandbox, m=mode: assert_fails_closed(
                    cargo_site, s, subprocess.CompletedProcess([], 0, "", ""), m
                ),
                "did not fail closed",
            )

        # Control G: the twine interception is real -- a fake `twine/__main__.py` on
        # PYTHONPATH must be what `python -m twine` runs.
        recipe = recipe_for(twine_site, root, patches)
        sandbox, result = exercise(twine_site, recipe, "success", base / "twine")
        assert_success(twine_site, sandbox, result)
        assert [r["publisher"] for r in sandbox.publisher_records()] == ["twine"]

        # Control H: a scrubbed parent environment is mandatory, and the scrubber
        # must actually reject a leaked alias.
        _expect_failure(
            "unscrubbed parent env",
            lambda: make_sandbox(
                base / "dirty-parent",
                "success",
                parent_env={"NPM_TOKEN": "x", "PATH": "/usr/bin"},
            ),
            "parent environment still holds ['NPM_TOKEN']",
        )

        # Controls I and J are end-to-end: this process happens to carry none of the
        # aliases, so the parent scrub is a no-op here and the child-side assertion
        # would be untested. Inject an obvious decoy (never a real credential) past
        # the scrubber and prove a real run detects ambient inheritance.
        DECOY = "decoy-ambient-value-not-a-credential"
        leaky = {key: value for key, value in os.environ.items() if key not in WATCHED}

        # I: the pre-patch recipe inherits the ambient alias. The proof must reject
        # it -- this is exactly the state the plan exists to remove.
        sandbox, result = exercise(
            cargo_site,
            unpatched,
            "success",
            base / "ambient-inherited",
            parent_env={**leaky, "CARGO_REGISTRY_TOKEN": DECOY},
            allow_parent_leak=True,
        )
        records = sandbox.publisher_records()
        assert records and records[0]["env"] == {
            "CARGO_REGISTRY_TOKEN": "<masked:%d>" % len(DECOY)
        }, records
        _expect_failure(
            "ambient alias inherited by the unpatched recipe",
            lambda: assert_success(cargo_site, sandbox, result),
            "passage lookup",
        )

        # J: an UNRELATED ambient alias must be caught even when the patched recipe
        # sets its own point-of-use variable correctly.
        sandbox, result = exercise(
            cargo_site,
            patched,
            "success",
            base / "unrelated-ambient",
            parent_env={**leaky, "NPM_TOKEN": DECOY},
            allow_parent_leak=True,
        )
        _expect_failure(
            "unrelated ambient alias reaching the publisher child",
            lambda: assert_success(cargo_site, sandbox, result),
            "no root alias",
        )

    print("[self-test] proof_point_of_use_publish_credentials: ok", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
