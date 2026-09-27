#!/usr/bin/env python3
"""Measure every gated `agent:review` item in tasks/software/lazily.md.

`#advance-review` used to mean four hand-run probes plus a prose refresh with
fresh timestamps. Across those passes the gates never moved -- they are all
operator-blocked -- and the only defect the pass ever caught was rotted
*evidence* sitting behind a green-sounding gate (`#lzambienttokenresolve`'s
preimage pin, four separate times). So the useful signal is not "is this gate
still blocked" but "does this gate's recorded evidence still measure true".

This reports one of four states per gate:

  blocked_fresh   still waiting on the operator, recorded evidence still holds
  evidence_stale  recorded evidence no longer holds, or expires inside the warn
                  window -- actionable now, without the operator
  unprobeable     no probe exists from here by construction (recorded, and never
                  reported as green)
  cleared         the blocking condition is gone; close or re-scope the item

Exit 0 when no gate is `evidence_stale`, 1 when one or more is, 2 on refusal
(a probe whose own inputs it cannot trust). Reads no credential, deletes
nothing, and reaches the network only through the two macOS release probes and
the credential proof it shells out to.

Every threshold that also lives in another script is imported from that script,
never restated here: the artifact-purge handoff limit and handoff location come
from `src/agent-doc/scripts/purge-actions-artifacts.py`, so moving them there
moves them here.
"""

from __future__ import annotations

import argparse
import datetime as dt
import importlib.util
import json
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Iterable, Sequence

REPO_ROOT = Path(__file__).resolve().parents[1]
AGENT_DOC_ROOT = REPO_ROOT / "src" / "agent-doc"
PURGE_SCRIPT = AGENT_DOC_ROOT / "scripts" / "purge-actions-artifacts.py"
CREDENTIAL_PROOF = "scripts/proof-point-of-use-publish-credentials.py"

# Pinned from `#lzmacosrunner`. The gate is blocked on Mac hardware, so this list
# is expected to stay byte-identical; any difference in either direction means
# the review text no longer describes the repository.
MACOS_INCOMPLETE_PAIR_BASELINE: tuple[str, ...] = (
    "v0.35.399",
    "v0.35.400",
    "v0.35.401",
    "v0.35.406",
    "v0.35.417",
)

DEFAULT_EXPIRY_WARN_HOURS = 24

BLOCKED_FRESH = "blocked_fresh"
EVIDENCE_STALE = "evidence_stale"
UNPROBEABLE = "unprobeable"
CLEARED = "cleared"

STATUSES = (BLOCKED_FRESH, EVIDENCE_STALE, UNPROBEABLE, CLEARED)


class GateRefusal(Exception):
    """A probe cannot trust its own inputs, so it reports nothing at all."""


@dataclass(frozen=True)
class CommandResult:
    exit_code: int
    stdout: str
    stderr: str

    @property
    def text(self) -> str:
        return f"{self.stdout}\n{self.stderr}"


Runner = Callable[[Sequence[str], Path], CommandResult]


def default_runner(argv: Sequence[str], cwd: Path) -> CommandResult:
    completed = subprocess.run(
        list(argv),
        cwd=str(cwd),
        capture_output=True,
        text=True,
        check=False,
    )
    return CommandResult(completed.returncode, completed.stdout, completed.stderr)


def gate(
    gate_id: str,
    status: str,
    detail: str,
    unblocker: str,
    evidence: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if status not in STATUSES:
        raise GateRefusal(f"{gate_id}: unknown status {status!r}")
    return {
        "id": gate_id,
        "status": status,
        "detail": detail,
        "unblocker": unblocker,
        "evidence": evidence or {},
    }


# --------------------------------------------------------------------------- #
# Shared thresholds, imported rather than restated
# --------------------------------------------------------------------------- #


def load_purge_module(path: Path = PURGE_SCRIPT) -> Any:
    """Import the purge script so its guard constants cannot desync from here."""
    if not path.is_file():
        raise GateRefusal(
            f"{path} is missing; the artifact-purge handoff limit has no source to import "
            "(is the src/agent-doc submodule checked out?)"
        )
    spec = importlib.util.spec_from_file_location("purge_actions_artifacts", path)
    if spec is None or spec.loader is None:
        raise GateRefusal(f"{path} cannot be imported as a module")
    module = importlib.util.module_from_spec(spec)
    # `dataclasses` resolves annotations through `sys.modules`, so the module has
    # to be registered before its body runs.
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
    except Exception as error:  # pragma: no cover - surfaced as a refusal
        sys.modules.pop(spec.name, None)
        raise GateRefusal(f"{path} failed to import: {error}") from error
    for name in ("DEFAULT_MAX_HANDOFF_AGE_DAYS", "DEFAULT_HANDOFF_DIR", "SUMMARY_JSON"):
        if not hasattr(module, name):
            raise GateRefusal(f"{path} no longer exports {name}; the handoff guard moved")
    return module


# --------------------------------------------------------------------------- #
# #lzambienttokenresolve -- point-of-use publish credential plan
# --------------------------------------------------------------------------- #


def probe_credential_plan(
    runner: Runner,
    root: Path,
    report_path: Path,
) -> dict[str, Any]:
    """Re-prove the plan's patch set. Asserts from the report, not the exit code."""
    result = runner(
        [sys.executable, CREDENTIAL_PROOF, "--report-path", str(report_path)],
        root,
    )
    if not report_path.is_file():
        raise GateRefusal(
            f"{CREDENTIAL_PROOF} wrote no report at {report_path} "
            f"(exit {result.exit_code}); there is nothing to assert from"
        )
    try:
        report = json.loads(report_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise GateRefusal(f"{report_path} is not valid JSON: {error}") from error

    static = report.get("static")
    if not isinstance(static, dict):
        raise GateRefusal(f"{report_path} has no `static` phase; the proof report shape changed")
    preimages = static.get("preimages")
    if not isinstance(preimages, list) or not preimages:
        raise GateRefusal(f"{report_path} records no preimages; the proof report shape changed")
    patches = static.get("patches")
    if not isinstance(patches, list) or not patches:
        raise GateRefusal(f"{report_path} records no patches; the proof report shape changed")
    if "failure_count" not in report:
        raise GateRefusal(f"{report_path} has no failure_count; the proof report shape changed")

    drifted = [entry.get("path") for entry in preimages if entry.get("state") != "match"]
    unappliable = [entry.get("path") for entry in patches if entry.get("state") != "appliable"]
    failure_count = int(report["failure_count"])
    evidence = {
        "proof_exit_code": result.exit_code,
        "failure_count": failure_count,
        "preimages_total": len(preimages),
        "preimages_drifted": drifted,
        "patches_total": len(patches),
        "patches_unappliable": unappliable,
        "application_state": static.get("application_state"),
        "report_path": str(report_path),
    }

    reasons: list[str] = []
    if drifted:
        reasons.append(f"{len(drifted)} preimage(s) drifted: {', '.join(map(str, drifted))}")
    if unappliable:
        reasons.append(f"{len(unappliable)} patch set(s) no longer apply: {', '.join(map(str, unappliable))}")
    if failure_count:
        reasons.append(f"proof reported {failure_count} failure(s)")
    # A report that contradicts its own exit status is itself the defect: the
    # gate must not read green off either half alone.
    if bool(reasons) != bool(result.exit_code):
        reasons.append(
            f"proof exit code {result.exit_code} disagrees with its report "
            f"(failure_count={failure_count}, drifted={len(drifted)}, unappliable={len(unappliable)})"
        )

    if reasons:
        return gate(
            "#lzambienttokenresolve",
            EVIDENCE_STALE,
            "; ".join(reasons),
            "refresh the recorded preimage hashes, then re-run `make publish-credential-proof`",
            evidence,
        )
    if static.get("application_state") != "unapplied":
        return gate(
            "#lzambienttokenresolve",
            CLEARED,
            f"application_state is {static.get('application_state')!r}; the plan is no longer pending",
            "close or re-scope the review item",
            evidence,
        )
    return gate(
        "#lzambienttokenresolve",
        BLOCKED_FRESH,
        f"{len(preimages)}/{len(preimages)} preimages match and {len(patches)}/{len(patches)} "
        "patch sets apply; plan still unapplied",
        "operator: quiet window, credential rotation, then the plan's step-5 loop in an interactive shell",
        evidence,
    )


# --------------------------------------------------------------------------- #
# #lzagentdocartifactpurge -- audit handoff expiry
# --------------------------------------------------------------------------- #


def probe_artifact_purge(
    purge: Any,
    now: dt.datetime,
    handoff_dir: Path | None = None,
    warn_hours: float = DEFAULT_EXPIRY_WARN_HOURS,
) -> dict[str, Any]:
    """Measure the handoff against the purge script's own 7-day abort guard."""
    handoff_dir = Path(handoff_dir if handoff_dir is not None else purge.DEFAULT_HANDOFF_DIR)
    parent = handoff_dir.parent
    siblings = sorted(p for p in parent.glob("artifact-purge-audit-*") if p.is_dir())
    if not siblings:
        raise GateRefusal(f"no artifact-purge handoff directory under {parent}")
    if len(siblings) > 1:
        raise GateRefusal(
            "more than one artifact-purge handoff on disk, so no single basis can be "
            f"authorized: {', '.join(p.name for p in siblings)}"
        )
    if siblings[0] != handoff_dir:
        raise GateRefusal(
            f"the only handoff on disk is {siblings[0].name}, but the purge script defaults to "
            f"{handoff_dir.name}"
        )

    summary_path = handoff_dir / purge.SUMMARY_JSON
    if not summary_path.is_file():
        raise GateRefusal(f"{summary_path} is missing; the handoff has no summary to read")
    try:
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise GateRefusal(f"{summary_path} is not valid JSON: {error}") from error

    raw = summary.get("generated_at")
    if not isinstance(raw, str) or not raw:
        raise GateRefusal(f"{summary_path} has no generated_at timestamp")
    try:
        generated = dt.datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError as error:
        raise GateRefusal(f"{summary_path}: unparseable generated_at {raw!r}") from error
    if generated.tzinfo is None:
        generated = generated.replace(tzinfo=dt.timezone.utc)

    limit_days = int(purge.DEFAULT_MAX_HANDOFF_AGE_DAYS)
    deadline = generated + dt.timedelta(days=limit_days)
    remaining_hours = (deadline - now).total_seconds() / 3600.0

    durability = summary.get("candidate_durability")
    split: dict[str, Any] = {}
    if isinstance(durability, dict):
        for key in ("durable", "perishable"):
            bucket = durability.get(key)
            if isinstance(bucket, dict):
                split[key] = {
                    "count": bucket.get("count"),
                    "size_bytes": bucket.get("size_bytes"),
                    "counterpart_kinds": bucket.get("counterpart_kinds"),
                }

    evidence = {
        "handoff": handoff_dir.name,
        "generated_at": generated.isoformat(),
        "max_handoff_age_days": limit_days,
        "deadline": deadline.isoformat(),
        "remaining_hours": round(remaining_hours, 2),
        "warn_within_hours": warn_hours,
        "candidate_count": summary.get("candidate_count"),
        "candidate_size_bytes": summary.get("candidate_size_bytes"),
        "withheld_count": summary.get("unresolved_count"),
        "candidate_durability": split,
        "deletion_performed": summary.get("deletion_performed"),
    }

    if remaining_hours <= 0:
        return gate(
            "#lzagentdocartifactpurge",
            EVIDENCE_STALE,
            f"handoff expired {abs(remaining_hours):.1f}h ago; every purge run now aborts on the "
            f"{limit_days}-day guard",
            "regenerate the audit with audit-actions-artifacts.py (or authorize with --allow-stale-handoff)",
            evidence,
        )
    if remaining_hours <= warn_hours:
        return gate(
            "#lzagentdocartifactpurge",
            EVIDENCE_STALE,
            f"handoff expires in {remaining_hours:.1f}h (warn window {warn_hours:.0f}h); after "
            f"{deadline.isoformat()} every purge run aborts",
            "authorize now, or regenerate the audit before the deadline",
            evidence,
        )
    if summary.get("deletion_performed"):
        return gate(
            "#lzagentdocartifactpurge",
            CLEARED,
            "the handoff records a completed deletion",
            "close the review item",
            evidence,
        )
    return gate(
        "#lzagentdocartifactpurge",
        BLOCKED_FRESH,
        f"sole handoff {handoff_dir.name} is {remaining_hours:.1f}h from its {limit_days}-day "
        "abort guard and records no deletion",
        "operator: authorize a destructive run (option (b) `--durability durable` is the conservative one)",
        evidence,
    )


# --------------------------------------------------------------------------- #
# #lzmacosrunner -- Darwin asset coverage gap
# --------------------------------------------------------------------------- #

_COVERAGE_TAGS = re.compile(r"incomplete pair of Darwin archives:\s*(?P<tags>[^;]+)")
_COVERAGE_COUNT = re.compile(r"(?P<count>\d+) release\(s\) published after")
_TAG = re.compile(r"^v\d+\.\d+\.\d+$")


def parse_incomplete_pairs(text: str) -> tuple[str, ...]:
    """Read the named releases, never the exit status, and refuse on a shape change."""
    match = _COVERAGE_TAGS.search(text)
    if match is None:
        raise GateRefusal(
            "release-macos-coverage-check refused but named no incomplete-pair releases; "
            "its message shape changed and the gap can no longer be measured"
        )
    tags = tuple(part.strip() for part in match.group("tags").split(",") if part.strip())
    bad = [tag for tag in tags if not _TAG.match(tag)]
    if bad:
        raise GateRefusal(
            f"release-macos-coverage-check named unparseable releases: {', '.join(bad)}"
        )
    counted = _COVERAGE_COUNT.search(text)
    if counted is not None and int(counted.group("count")) != len(tags):
        raise GateRefusal(
            f"release-macos-coverage-check counts {counted.group('count')} release(s) but names "
            f"{len(tags)}: {', '.join(tags)}"
        )
    return tags


def probe_macos_gap(
    runner: Runner,
    agent_doc_root: Path,
    baseline: Iterable[str] = MACOS_INCOMPLETE_PAIR_BASELINE,
) -> dict[str, Any]:
    cadence = runner(["make", "release-macos-cadence-check"], agent_doc_root)
    coverage = runner(["make", "release-macos-coverage-check"], agent_doc_root)
    window = "open" if cadence.exit_code == 0 else "closed"
    evidence: dict[str, Any] = {
        "cadence_exit_code": cadence.exit_code,
        "weekly_window": window,
        "coverage_exit_code": coverage.exit_code,
        "baseline": list(baseline),
    }

    if coverage.exit_code == 0:
        evidence["incomplete_pairs"] = []
        return gate(
            "#lzmacosrunner",
            CLEARED,
            "release-macos-coverage-check passes: every release since the last Darwin-complete "
            "one ships a complete pair",
            "close the review item",
            evidence,
        )

    tags = parse_incomplete_pairs(coverage.text)
    evidence["incomplete_pairs"] = list(tags)
    expected = set(baseline)
    live = set(tags)
    added = sorted(live - expected)
    removed = sorted(expected - live)
    if added or removed:
        evidence["added"] = added
        evidence["removed"] = removed
        parts = []
        if added:
            parts.append(f"{len(added)} new incomplete-pair release(s): {', '.join(added)}")
        if removed:
            parts.append(f"{len(removed)} baseline release(s) no longer listed: {', '.join(removed)}")
        return gate(
            "#lzmacosrunner",
            EVIDENCE_STALE,
            "; ".join(parts),
            "refresh MACOS_INCOMPLETE_PAIR_BASELINE and the review text to the measured list",
            evidence,
        )
    return gate(
        "#lzmacosrunner",
        BLOCKED_FRESH,
        f"the same {len(tags)} release(s) still ship an incomplete Darwin pair and the weekly "
        f"window is {window}; the gap is not widening",
        "operator: `make release-macos-assets TAG=<tag>` on a Mac, or supply a build host",
        evidence,
    )


# --------------------------------------------------------------------------- #
# #lzgithubbudget20 -- unprobeable by construction
# --------------------------------------------------------------------------- #


def probe_github_budget() -> dict[str, Any]:
    """No probe exists from here: record that, and never report it as green."""
    return gate(
        "#lzgithubbudget20",
        UNPROBEABLE,
        "personal billing settings are not exposed to this token: "
        "/users/btakita/settings/billing/{actions,usage} return 404 with a `user`-scope hint, and "
        "the REST billing API exposes no payment-method field even with that scope",
        "operator: add a valid payment method to the personal GitHub account, then set the "
        "Actions product budget to $20/month with Stop usage enabled",
        {"probe": "none", "reason": "no API surface reports a payment method or product budget"},
    )


# --------------------------------------------------------------------------- #
# Orchestration
# --------------------------------------------------------------------------- #


def aggregate_exit_code(gates: Sequence[dict[str, Any]]) -> int:
    """Exit status derives from gate content, never from any probe's exit code."""
    return 1 if any(g["status"] == EVIDENCE_STALE for g in gates) else 0


def collect_gates(
    runner: Runner,
    now: dt.datetime,
    root: Path,
    report_path: Path,
    warn_hours: float,
) -> list[dict[str, Any]]:
    purge = load_purge_module()
    return [
        probe_credential_plan(runner, root, report_path),
        probe_artifact_purge(purge, now, warn_hours=warn_hours),
        probe_macos_gap(runner, root / "src" / "agent-doc"),
        probe_github_budget(),
    ]


def run(args: argparse.Namespace) -> int:
    root = args.root.resolve()
    now = dt.datetime.now(dt.timezone.utc)
    with tempfile.TemporaryDirectory(prefix="review-gate-status-") as tmp:
        report_path = (
            args.credential_report_path
            if args.credential_report_path
            else Path(tmp) / "publish-credential-proof.json"
        )
        gates = collect_gates(default_runner, now, root, report_path, args.expiry_warn_hours)

    by_status: dict[str, list[str]] = {status: [] for status in STATUSES}
    for entry in gates:
        by_status[entry["status"]].append(entry["id"])

    exit_code = aggregate_exit_code(gates)
    report = {
        "generated_at": now.isoformat(),
        "root": str(root),
        "gates": gates,
        "by_status": by_status,
        "stale_count": len(by_status[EVIDENCE_STALE]),
    }
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    if args.report_path:
        args.report_path.parent.mkdir(parents=True, exist_ok=True)
        args.report_path.write_text(rendered + "\n", encoding="utf-8")

    for entry in gates:
        print(f"[review-gate] {entry['status']:<14} {entry['id']}: {entry['detail']}", file=sys.stderr)
    if exit_code:
        print(
            f"[review-gate] {len(by_status[EVIDENCE_STALE])} gate(s) have stale evidence: "
            f"{', '.join(by_status[EVIDENCE_STALE])}",
            file=sys.stderr,
        )
    else:
        print("[review-gate] no gate evidence has rotted", file=sys.stderr)
    return exit_code


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--root", type=Path, default=REPO_ROOT)
    parser.add_argument(
        "--expiry-warn-hours",
        type=float,
        default=DEFAULT_EXPIRY_WARN_HOURS,
        help="report the artifact-purge handoff stale this long before it actually expires",
    )
    parser.add_argument("--report-path", type=Path)
    parser.add_argument(
        "--credential-report-path",
        type=Path,
        help="keep the credential proof's JSON report at this path instead of a temp dir",
    )
    parser.add_argument("--self-test", action="store_true", help="run the negative controls")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.self_test:
        return self_test()
    try:
        return run(args)
    except GateRefusal as refusal:
        print(f"[review-gate] refused: {refusal}", file=sys.stderr)
        return 2


# --------------------------------------------------------------------------- #
# Negative controls: prove each assertion can fail.
# --------------------------------------------------------------------------- #


def _expect_refusal(label: str, thunk: Callable[[], Any], expected: str) -> None:
    try:
        thunk()
    except GateRefusal as refusal:
        assert expected in str(refusal), f"{label}: expected {expected!r} in {refusal}"
        return
    raise AssertionError(f"{label}: the probe returned when it should have refused")


def _fake_runner(responses: dict[str, CommandResult]) -> Runner:
    def runner(argv: Sequence[str], cwd: Path) -> CommandResult:
        for key, response in responses.items():
            if any(key in part for part in argv):
                return response
        raise AssertionError(f"unexpected command: {list(argv)}")

    return runner


class _PurgeShim:
    """Stands in for the purge module so the age matrix runs offline."""

    SUMMARY_JSON = "artifact-audit-summary.json"

    def __init__(self, handoff_dir: Path, limit_days: int) -> None:
        self.DEFAULT_HANDOFF_DIR = handoff_dir
        self.DEFAULT_MAX_HANDOFF_AGE_DAYS = limit_days


def _write_handoff(
    parent: Path,
    name: str,
    generated_at: str | None,
    *,
    body: str | None = None,
    deletion_performed: bool = False,
) -> Path:
    directory = parent / name
    directory.mkdir(parents=True, exist_ok=True)
    summary_path = directory / _PurgeShim.SUMMARY_JSON
    if body is not None:
        summary_path.write_text(body, encoding="utf-8")
        return directory
    summary: dict[str, Any] = {
        "candidate_count": 2666,
        "candidate_size_bytes": 43834598531,
        "unresolved_count": 571,
        "deletion_performed": deletion_performed,
        "candidate_durability": {
            "durable": {"count": 2136, "size_bytes": 34825051886, "counterpart_kinds": ["github_release"]},
            "perishable": {"count": 530, "size_bytes": 9009546645, "counterpart_kinds": ["pypi_file"]},
        },
    }
    if generated_at is not None:
        summary["generated_at"] = generated_at
    summary_path.write_text(json.dumps(summary), encoding="utf-8")
    return directory


def _proof_report(
    *,
    failure_count: int = 0,
    drifted: int = 0,
    unappliable: int = 0,
    application_state: str = "unapplied",
    drop: str | None = None,
) -> dict[str, Any]:
    preimages = [
        {"path": f"f{i}", "state": "differs" if i < drifted else "match"} for i in range(8)
    ]
    patches = [
        {"path": f"f{i}", "state": "refused" if i < unappliable else "appliable"} for i in range(8)
    ]
    static: dict[str, Any] = {
        "application_state": application_state,
        "preimages": preimages,
        "patches": patches,
        "failures": [],
        "skipped_absent": [],
    }
    report: dict[str, Any] = {"static": static, "failure_count": failure_count, "failures": []}
    if drop == "static":
        report.pop("static")
    elif drop == "preimages":
        static.pop("preimages")
    elif drop == "patches":
        static.pop("patches")
    elif drop == "failure_count":
        report.pop("failure_count")
    return report


def _credential_runner(report: dict[str, Any] | None, exit_code: int) -> Runner:
    def runner(argv: Sequence[str], cwd: Path) -> CommandResult:
        assert CREDENTIAL_PROOF in argv, f"unexpected command: {list(argv)}"
        target = Path(argv[argv.index("--report-path") + 1])
        if report is not None:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(json.dumps(report), encoding="utf-8")
        return CommandResult(exit_code, "", "")

    return runner

def _coverage_result(tags: Sequence[str], *, count: int | None = None) -> CommandResult:
    total = len(tags) if count is None else count
    return CommandResult(
        2,
        "",
        f"agent-doc-dev: {total} release(s) published after the last Darwin-complete release "
        f"v0.35.398 (2026-09-15T16:41:52+00:00) ship an incomplete pair of Darwin archives: "
        f"{', '.join(tags)}; build them on a Mac with `make release-macos-assets TAG=<tag>`",
    )


def _macos_runner(coverage: CommandResult, cadence_exit: int = 0) -> Runner:
    return _fake_runner(
        {
            "release-macos-cadence-check": CommandResult(cadence_exit, "", ""),
            "release-macos-coverage-check": coverage,
        }
    )


def self_test() -> int:
    """Validate the detector before trusting any green result."""
    baseline = MACOS_INCOMPLETE_PAIR_BASELINE
    root = REPO_ROOT

    # --- the shared threshold is imported, not restated ----------------------
    purge = load_purge_module()
    assert isinstance(purge.DEFAULT_MAX_HANDOFF_AGE_DAYS, int)
    assert purge.SUMMARY_JSON, "the purge module must name its summary file"
    _expect_refusal(
        "missing purge script",
        lambda: load_purge_module(root / "scripts" / "does-not-exist.py"),
        "has no source to import",
    )

    with tempfile.TemporaryDirectory(prefix="review-gate-self-test-") as tmp:
        tmpdir = Path(tmp)

        # --- #lzmacosrunner --------------------------------------------------
        unchanged = probe_macos_gap(_macos_runner(_coverage_result(baseline)), root, baseline)
        assert unchanged["status"] == BLOCKED_FRESH, unchanged
        assert unchanged["evidence"]["weekly_window"] == "open"

        closed_window = probe_macos_gap(
            _macos_runner(_coverage_result(baseline), cadence_exit=1), root, baseline
        )
        assert closed_window["status"] == BLOCKED_FRESH, closed_window
        assert closed_window["evidence"]["weekly_window"] == "closed"

        widened = probe_macos_gap(
            _macos_runner(_coverage_result([*baseline, "v0.35.420"])), root, baseline
        )
        assert widened["status"] == EVIDENCE_STALE, widened
        assert widened["evidence"]["added"] == ["v0.35.420"], widened

        narrowed = probe_macos_gap(_macos_runner(_coverage_result(baseline[:-1])), root, baseline)
        assert narrowed["status"] == EVIDENCE_STALE, narrowed
        assert narrowed["evidence"]["removed"] == [baseline[-1]], narrowed

        cleared = probe_macos_gap(
            _macos_runner(CommandResult(0, "every release ships a complete pair\n", "")),
            root,
            baseline,
        )
        assert cleared["status"] == CLEARED, cleared

        # A refusal exit with no parseable list must never read as "no gap".
        _expect_refusal(
            "coverage message shape change",
            lambda: probe_macos_gap(
                _macos_runner(CommandResult(2, "", "something went wrong\n")), root, baseline
            ),
            "named no incomplete-pair releases",
        )
        _expect_refusal(
            "coverage count disagrees with the named list",
            lambda: probe_macos_gap(
                _macos_runner(_coverage_result(baseline, count=9)), root, baseline
            ),
            "but names",
        )
        _expect_refusal(
            "coverage named a non-tag",
            lambda: probe_macos_gap(
                _macos_runner(_coverage_result(["v0.35.399", "not-a-tag"])), root, baseline
            ),
            "unparseable releases",
        )
        # The last Darwin-complete release appears in the same sentence and must
        # not be counted as part of the gap.
        assert "v0.35.398" not in parse_incomplete_pairs(_coverage_result(baseline).text)

        # --- #lzagentdocartifactpurge ---------------------------------------
        now = dt.datetime(2026, 9, 27, 12, 0, tzinfo=dt.timezone.utc)
        handoffs = tmpdir / "handoffs"
        fresh_dir = _write_handoff(
            handoffs, "artifact-purge-audit-fresh", "2026-09-27T05:56:08.338552+00:00"
        )
        shim = _PurgeShim(fresh_dir, purge.DEFAULT_MAX_HANDOFF_AGE_DAYS)
        fresh = probe_artifact_purge(shim, now)
        assert fresh["status"] == BLOCKED_FRESH, fresh
        assert fresh["evidence"]["candidate_count"] == 2666
        assert fresh["evidence"]["candidate_durability"]["durable"]["count"] == 2136
        assert fresh["evidence"]["max_handoff_age_days"] == purge.DEFAULT_MAX_HANDOFF_AGE_DAYS

        # The limit is imported: shrink it and the same handoff goes stale.
        tight = probe_artifact_purge(_PurgeShim(fresh_dir, 0), now)
        assert tight["status"] == EVIDENCE_STALE, tight
        assert tight["evidence"]["max_handoff_age_days"] == 0

        # Expiry, and the warn window that fires before it.
        expired_dir = _write_handoff(
            tmpdir / "expired", "artifact-purge-audit-old", "2026-09-01T00:00:00+00:00"
        )
        expired = probe_artifact_purge(_PurgeShim(expired_dir, 7), now)
        assert expired["status"] == EVIDENCE_STALE, expired
        assert "expired" in expired["detail"], expired

        warn_dir = _write_handoff(
            tmpdir / "warn", "artifact-purge-audit-warn", "2026-09-20T18:00:00+00:00"
        )
        warned = probe_artifact_purge(_PurgeShim(warn_dir, 7), now, warn_hours=24)
        assert warned["status"] == EVIDENCE_STALE, warned
        assert "warn window" in warned["detail"], warned
        # The same handoff with no warn window is merely blocked.
        assert probe_artifact_purge(_PurgeShim(warn_dir, 7), now, warn_hours=0)["status"] == BLOCKED_FRESH

        done_dir = _write_handoff(
            tmpdir / "done",
            "artifact-purge-audit-done",
            "2026-09-27T05:56:08+00:00",
            deletion_performed=True,
        )
        assert probe_artifact_purge(_PurgeShim(done_dir, 7), now)["status"] == CLEARED

        # Two handoffs on disk: no single basis can be authorized.
        doubled = tmpdir / "doubled"
        first = _write_handoff(doubled, "artifact-purge-audit-a", "2026-09-27T05:56:08+00:00")
        _write_handoff(doubled, "artifact-purge-audit-b", "2026-09-27T05:56:08+00:00")
        _expect_refusal(
            "two handoffs",
            lambda: probe_artifact_purge(_PurgeShim(first, 7), now),
            "more than one artifact-purge handoff",
        )

        empty = tmpdir / "empty"
        empty.mkdir()
        _expect_refusal(
            "no handoff",
            lambda: probe_artifact_purge(_PurgeShim(empty / "artifact-purge-audit-x", 7), now),
            "no artifact-purge handoff directory",
        )

        mismatch = tmpdir / "mismatch"
        _write_handoff(mismatch, "artifact-purge-audit-real", "2026-09-27T05:56:08+00:00")
        _expect_refusal(
            "default points elsewhere",
            lambda: probe_artifact_purge(
                _PurgeShim(mismatch / "artifact-purge-audit-other", 7), now
            ),
            "but the purge script defaults to",
        )

        no_stamp = _write_handoff(tmpdir / "nostamp", "artifact-purge-audit-nostamp", None)
        _expect_refusal(
            "no generated_at",
            lambda: probe_artifact_purge(_PurgeShim(no_stamp, 7), now),
            "has no generated_at",
        )

        bad_stamp = _write_handoff(tmpdir / "badstamp", "artifact-purge-audit-bad", "not-a-date")
        _expect_refusal(
            "unparseable generated_at",
            lambda: probe_artifact_purge(_PurgeShim(bad_stamp, 7), now),
            "unparseable generated_at",
        )

        broken = _write_handoff(tmpdir / "broken", "artifact-purge-audit-broken", None, body="{")
        _expect_refusal(
            "corrupt summary",
            lambda: probe_artifact_purge(_PurgeShim(broken, 7), now),
            "is not valid JSON",
        )

        missing_summary = tmpdir / "nosummary" / "artifact-purge-audit-nosummary"
        missing_summary.mkdir(parents=True)
        _expect_refusal(
            "no summary file",
            lambda: probe_artifact_purge(_PurgeShim(missing_summary, 7), now),
            "has no summary to read",
        )

        # --- #lzambienttokenresolve -----------------------------------------
        proof_report = tmpdir / "proof.json"
        ok = probe_credential_plan(_credential_runner(_proof_report(), 0), root, proof_report)
        assert ok["status"] == BLOCKED_FRESH, ok
        assert ok["evidence"]["preimages_drifted"] == []

        drifted = probe_credential_plan(
            _credential_runner(_proof_report(drifted=1, failure_count=1), 1), root, proof_report
        )
        assert drifted["status"] == EVIDENCE_STALE, drifted
        assert "preimage(s) drifted" in drifted["detail"], drifted

        refused_patch = probe_credential_plan(
            _credential_runner(_proof_report(unappliable=2, failure_count=2), 1), root, proof_report
        )
        assert refused_patch["status"] == EVIDENCE_STALE, refused_patch
        assert "no longer apply" in refused_patch["detail"], refused_patch

        # A green exit over a report that names failures is itself the defect.
        lying_green = probe_credential_plan(
            _credential_runner(_proof_report(failure_count=3), 0), root, proof_report
        )
        assert lying_green["status"] == EVIDENCE_STALE, lying_green
        assert "disagrees with its report" in lying_green["detail"], lying_green

        # ...and so is a red exit over a clean report.
        lying_red = probe_credential_plan(_credential_runner(_proof_report(), 1), root, proof_report)
        assert lying_red["status"] == EVIDENCE_STALE, lying_red
        assert "disagrees with its report" in lying_red["detail"], lying_red

        applied = probe_credential_plan(
            _credential_runner(_proof_report(application_state="applied"), 0), root, proof_report
        )
        assert applied["status"] == CLEARED, applied

        for drop, expected in (
            ("static", "has no `static` phase"),
            ("preimages", "records no preimages"),
            ("patches", "records no patches"),
            ("failure_count", "has no failure_count"),
        ):
            _expect_refusal(
                f"proof report missing {drop}",
                lambda drop=drop: probe_credential_plan(
                    _credential_runner(_proof_report(drop=drop), 0), root, proof_report
                ),
                expected,
            )

        missing_report = tmpdir / "absent" / "proof.json"
        _expect_refusal(
            "proof wrote no report",
            lambda: probe_credential_plan(_credential_runner(None, 0), root, missing_report),
            "wrote no report",
        )

        corrupt_report = tmpdir / "corrupt.json"
        corrupt_report.write_text("{", encoding="utf-8")

        def _no_write(argv: Sequence[str], cwd: Path) -> CommandResult:
            return CommandResult(0, "", "")

        _expect_refusal(
            "proof report is corrupt",
            lambda: probe_credential_plan(_no_write, root, corrupt_report),
            "is not valid JSON",
        )

    # --- #lzgithubbudget20 ---------------------------------------------------
    budget = probe_github_budget()
    assert budget["status"] == UNPROBEABLE, budget
    assert budget["status"] not in (BLOCKED_FRESH, CLEARED), budget
    assert budget["unblocker"], "an unprobeable gate must still name its unblocker"

    # --- aggregation ---------------------------------------------------------
    blocked_only = [
        gate("#a", BLOCKED_FRESH, "d", "u"),
        gate("#b", UNPROBEABLE, "d", "u"),
        gate("#c", CLEARED, "d", "u"),
    ]
    assert aggregate_exit_code(blocked_only) == 0
    assert aggregate_exit_code([*blocked_only, gate("#d", EVIDENCE_STALE, "d", "u")]) == 1
    # Exactly one stale gate must be enough; it cannot be outvoted.
    assert aggregate_exit_code([gate("#d", EVIDENCE_STALE, "d", "u")]) == 1
    try:
        gate("#e", "green", "d", "u")
    except GateRefusal:
        pass
    else:
        raise AssertionError("an unknown status was accepted")

    print("[self-test] review_gate_status: ok", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
