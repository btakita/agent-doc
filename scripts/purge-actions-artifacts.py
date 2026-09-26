#!/usr/bin/env python3
"""Fail-closed executor for the GitHub Actions artifact purge handoff.

`audit-actions-artifacts.py` produces a read-only candidate handoff and is
deliberately incapable of deletion. This command is the only place that may call
the artifact deletion endpoint, and it is **dry run by default**: an actual
deletion requires BOTH `--execute` and `--authorize-deletion <owner/repo>`.

Two invariants are encoded here rather than left in the handoff prose:

1. Every candidate is re-derived against the live Actions API at execution time,
   using the audit module's own candidacy rule as the single source of truth. A
   handoff row whose live artifact is gone, expired, resized, renamed, or no
   longer provably backed by an exact durable counterpart can never authorize a
   deletion.
2. Every artifact id recorded in `artifact-unresolved.csv` is refused
   unconditionally -- even if the live re-derivation would now call it a
   candidate, and even if an operator names it explicitly with `--only`.

Structural faults in the handoff (a candidate/withheld overlap, a candidate with
no counterpart, summary totals that disagree with the CSVs, a stale generation, a
repository that does not match the audit) abort the whole run before any network
call. Per-artifact drift refuses that artifact and leaves the rest of the plan
intact.

Run `--self-test` for the offline refusal-path regressions (wired into
`make check` via `artifact-purge-check`).
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import importlib.util
import json
import re
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterable

GITHUB_API = "https://api.github.com"
AUDIT_SCRIPT = Path(__file__).resolve().parent / "audit-actions-artifacts.py"
DEFAULT_HANDOFF_DIR = (
    Path(__file__).resolve().parent.parent
    / "tasks"
    / "agent-doc"
    / "artifact-purge-audit-2026-09-25"
)
DEFAULT_MAX_DELETIONS = 250
DEFAULT_MAX_HANDOFF_AGE_DAYS = 7
CANDIDATES_CSV = "artifact-delete-candidates.csv"
UNRESOLVED_CSV = "artifact-unresolved.csv"
SUMMARY_JSON = "artifact-audit-summary.json"


class PurgeRefusal(Exception):
    """A whole-run fail-closed refusal. Nothing is deleted."""


def load_audit_module() -> Any:
    """Import the audit generator so candidacy has exactly one definition."""
    if not AUDIT_SCRIPT.is_file():
        raise PurgeRefusal(f"audit generator is missing: {AUDIT_SCRIPT}")
    spec = importlib.util.spec_from_file_location("audit_actions_artifacts", AUDIT_SCRIPT)
    if spec is None or spec.loader is None:
        raise PurgeRefusal(f"audit generator is not importable: {AUDIT_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@dataclass(frozen=True)
class AuditRow:
    artifact_id: int
    artifact_name: str
    size_bytes: int
    expires_at: str
    counterpart_kind: str
    counterpart_name: str
    counterpart_url: str
    unresolved_reason: str


@dataclass(frozen=True)
class Handoff:
    directory: Path
    repo: str
    generated_at: str
    candidates: dict[int, AuditRow]
    withheld: dict[int, AuditRow]


@dataclass(frozen=True)
class LiveArtifact:
    artifact_id: int
    artifact_name: str
    size_bytes: int
    expires_at: str
    expired: bool
    counterpart_kind: str
    counterpart_name: str
    counterpart_url: str
    unresolved_reason: str


@dataclass
class Plan:
    delete: list[int] = field(default_factory=list)
    refused: list[dict[str, Any]] = field(default_factory=list)
    skipped: list[dict[str, Any]] = field(default_factory=list)

    def delete_bytes(self, handoff: Handoff) -> int:
        return sum(handoff.candidates[artifact_id].size_bytes for artifact_id in self.delete)


def _row(record: dict[str, str], source: str) -> AuditRow:
    try:
        return AuditRow(
            artifact_id=int(record["artifact_id"]),
            artifact_name=record["artifact_name"],
            size_bytes=int(record["size_bytes"]),
            expires_at=record["expires_at"],
            counterpart_kind=record["counterpart_kind"],
            counterpart_name=record["counterpart_name"],
            counterpart_url=record["counterpart_url"],
            unresolved_reason=record.get("unresolved_reason", ""),
        )
    except (KeyError, TypeError, ValueError) as error:
        raise PurgeRefusal(f"{source}: unreadable row {record!r}: {error}") from error


def _read_csv(path: Path) -> list[dict[str, str]]:
    if not path.is_file():
        raise PurgeRefusal(f"handoff file is missing: {path}")
    with path.open(encoding="utf-8", newline="") as handle:
        return list(csv.DictReader(handle))


def load_handoff(
    directory: Path,
    *,
    now: dt.datetime | None = None,
    max_age_days: int | None = DEFAULT_MAX_HANDOFF_AGE_DAYS,
) -> Handoff:
    """Load and structurally validate the handoff. Any inconsistency aborts."""
    summary_path = directory / SUMMARY_JSON
    if not summary_path.is_file():
        raise PurgeRefusal(f"handoff file is missing: {summary_path}")
    try:
        summary = json.loads(summary_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise PurgeRefusal(f"{summary_path}: unreadable summary: {error}") from error

    repo = summary.get("repository")
    if not isinstance(repo, str) or repo.count("/") != 1:
        raise PurgeRefusal(f"{summary_path}: summary has no usable repository: {repo!r}")
    if summary.get("deletion_performed") is not False:
        raise PurgeRefusal(
            f"{summary_path}: summary already records deletion_performed="
            f"{summary.get('deletion_performed')!r}; regenerate the audit before purging"
        )

    candidate_rows = [_row(record, CANDIDATES_CSV) for record in _read_csv(directory / CANDIDATES_CSV)]
    withheld_rows = [_row(record, UNRESOLVED_CSV) for record in _read_csv(directory / UNRESOLVED_CSV)]

    candidates: dict[int, AuditRow] = {}
    for row in candidate_rows:
        if row.artifact_id in candidates:
            raise PurgeRefusal(f"{CANDIDATES_CSV}: duplicate artifact id {row.artifact_id}")
        if not row.counterpart_url:
            raise PurgeRefusal(
                f"{CANDIDATES_CSV}: artifact {row.artifact_id} has no durable counterpart URL"
            )
        if row.unresolved_reason:
            raise PurgeRefusal(
                f"{CANDIDATES_CSV}: artifact {row.artifact_id} carries unresolved reason "
                f"{row.unresolved_reason!r}"
            )
        candidates[row.artifact_id] = row

    withheld: dict[int, AuditRow] = {}
    for row in withheld_rows:
        if row.artifact_id in withheld:
            raise PurgeRefusal(f"{UNRESOLVED_CSV}: duplicate artifact id {row.artifact_id}")
        if row.counterpart_url:
            raise PurgeRefusal(
                f"{UNRESOLVED_CSV}: withheld artifact {row.artifact_id} carries a counterpart URL; "
                "the audit is inconsistent"
            )
        withheld[row.artifact_id] = row

    overlap = sorted(set(candidates) & set(withheld))
    if overlap:
        raise PurgeRefusal(
            f"{CANDIDATES_CSV} and {UNRESOLVED_CSV} both list "
            f"{len(overlap)} artifact id(s), first {overlap[0]}; the audit is inconsistent"
        )

    for key, actual in (("candidate_count", len(candidates)), ("unresolved_count", len(withheld))):
        recorded = summary.get(key)
        if recorded != actual:
            raise PurgeRefusal(
                f"{SUMMARY_JSON}: {key}={recorded!r} disagrees with {actual} CSV rows"
            )
    recorded_bytes = summary.get("candidate_size_bytes")
    actual_bytes = sum(row.size_bytes for row in candidates.values())
    if recorded_bytes != actual_bytes:
        raise PurgeRefusal(
            f"{SUMMARY_JSON}: candidate_size_bytes={recorded_bytes!r} disagrees with "
            f"{actual_bytes} summed from {CANDIDATES_CSV}"
        )

    generated_at = summary.get("generated_at")
    if not isinstance(generated_at, str) or not generated_at:
        raise PurgeRefusal(f"{SUMMARY_JSON}: summary has no generated_at timestamp")
    if max_age_days is not None:
        try:
            generated = dt.datetime.fromisoformat(generated_at.replace("Z", "+00:00"))
        except ValueError as error:
            raise PurgeRefusal(f"{SUMMARY_JSON}: unparseable generated_at {generated_at!r}") from error
        if generated.tzinfo is None:
            generated = generated.replace(tzinfo=dt.timezone.utc)
        reference = now or dt.datetime.now(dt.timezone.utc)
        age = reference - generated
        if age > dt.timedelta(days=max_age_days):
            raise PurgeRefusal(
                f"handoff was generated {age.days} days ago (limit {max_age_days}); regenerate it "
                "with audit-actions-artifacts.py, or pass --allow-stale-handoff to purge from a "
                "stale candidate list re-derived live"
            )

    return Handoff(
        directory=directory,
        repo=repo,
        generated_at=generated_at,
        candidates=candidates,
        withheld=withheld,
    )


def assert_rederivation_scope(audit: Any, repo: str) -> None:
    """Refuse when the audit module's PyPI project cannot describe `repo`."""
    match = re.search(r"/pypi/([^/]+)/json", getattr(audit, "PYPI_API", ""))
    if match is None:
        raise PurgeRefusal("audit generator exposes no recognizable PyPI project URL")
    project = match.group(1)
    name = repo.split("/", 1)[1]
    if project.replace("-", "_") != name.replace("-", "_"):
        raise PurgeRefusal(
            f"audit generator resolves PyPI project {project!r}, which cannot verify "
            f"counterparts for {repo!r}; re-derivation would compare the wrong project"
        )


def live_candidacy(audit: Any, client: Any, repo: str, workers: int) -> dict[int, LiveArtifact]:
    """Re-derive candidacy for every live artifact using the audit's own rule."""
    artifacts = client.github_pages(f"/repos/{repo}/actions/artifacts", "artifacts")
    releases_payload = client.github_pages(f"/repos/{repo}/releases", "releases")
    releases = {
        release["tag_name"]: {asset["name"]: asset for asset in release["assets"]}
        for release in releases_payload
        if not release["draft"]
    }
    pypi_releases = client.json(audit.PYPI_API, authenticated=False)["releases"]

    run_ids = sorted({artifact["workflow_run"]["id"] for artifact in artifacts})
    runs, run_errors = audit.fetch_runs(client, repo, run_ids, workers)
    del runs  # candidacy needs only the artifact's own head_branch; runs prove reachability

    live: dict[int, LiveArtifact] = {}
    for artifact in artifacts:
        run_id = artifact["workflow_run"]["id"]
        kind, name, url, reason = audit.counterpart(artifact, releases, pypi_releases)
        if run_id in run_errors:
            kind = name = url = ""
            reason = f"workflow run lookup failed: {run_errors[run_id]}"
        artifact_id = int(artifact["id"])
        if artifact_id in live:
            raise PurgeRefusal(f"live Actions API returned artifact {artifact_id} twice")
        live[artifact_id] = LiveArtifact(
            artifact_id=artifact_id,
            artifact_name=artifact["name"],
            size_bytes=int(artifact["size_in_bytes"]),
            expires_at=artifact["expires_at"],
            expired=bool(artifact["expired"]),
            counterpart_kind=kind,
            counterpart_name=name,
            counterpart_url=url,
            unresolved_reason=reason,
        )
    return live


def plan_purge(
    handoff: Handoff,
    live: dict[int, LiveArtifact],
    *,
    only: Iterable[int] | None = None,
) -> Plan:
    """Intersect the handoff with live candidacy. Withheld ids are never deletable."""
    plan = Plan()

    if only is not None:
        requested = sorted(set(only))
        withheld_requested = [artifact_id for artifact_id in requested if artifact_id in handoff.withheld]
        if withheld_requested:
            raise PurgeRefusal(
                "--only names withheld artifact id(s) "
                f"{withheld_requested}; artifact-unresolved.csv rows are never deletable"
            )
        unknown = [artifact_id for artifact_id in requested if artifact_id not in handoff.candidates]
        if unknown:
            raise PurgeRefusal(f"--only names non-candidate artifact id(s) {unknown}")
        selection = requested
    else:
        selection = sorted(handoff.candidates)

    for artifact_id in selection:
        row = handoff.candidates[artifact_id]

        # Second guard on the same invariant: a withheld id stays refused even if the
        # live re-derivation now proves a counterpart for it.
        if artifact_id in handoff.withheld:
            plan.refused.append(
                {
                    "artifact_id": artifact_id,
                    "reason": "withheld_by_audit",
                    "detail": handoff.withheld[artifact_id].unresolved_reason,
                }
            )
            continue

        current = live.get(artifact_id)
        if current is None:
            plan.skipped.append({"artifact_id": artifact_id, "reason": "absent_from_live_api"})
            continue
        if current.expired:
            plan.skipped.append({"artifact_id": artifact_id, "reason": "already_expired"})
            continue

        drift = [
            f"{label}: audit {recorded!r} != live {observed!r}"
            for label, recorded, observed in (
                ("artifact_name", row.artifact_name, current.artifact_name),
                ("size_bytes", row.size_bytes, current.size_bytes),
                ("expires_at", row.expires_at, current.expires_at),
            )
            if recorded != observed
        ]
        if drift:
            plan.refused.append(
                {"artifact_id": artifact_id, "reason": "live_metadata_drift", "detail": "; ".join(drift)}
            )
            continue

        if not current.counterpart_url:
            plan.refused.append(
                {
                    "artifact_id": artifact_id,
                    "reason": "no_live_durable_counterpart",
                    "detail": current.unresolved_reason or "live re-derivation found no counterpart",
                }
            )
            continue

        counterpart_drift = [
            f"{label}: audit {recorded!r} != live {observed!r}"
            for label, recorded, observed in (
                ("counterpart_kind", row.counterpart_kind, current.counterpart_kind),
                ("counterpart_name", row.counterpart_name, current.counterpart_name),
                ("counterpart_url", row.counterpart_url, current.counterpart_url),
            )
            if recorded != observed
        ]
        if counterpart_drift:
            plan.refused.append(
                {
                    "artifact_id": artifact_id,
                    "reason": "counterpart_drift",
                    "detail": "; ".join(counterpart_drift),
                }
            )
            continue

        plan.delete.append(artifact_id)

    return plan


def delete_artifact(token: str, repo: str, artifact_id: int, timeout: int = 60) -> int:
    request = urllib.request.Request(
        f"{GITHUB_API}/repos/{repo}/actions/artifacts/{artifact_id}",
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "User-Agent": "agent-doc-artifact-purge",
            "X-GitHub-Api-Version": "2022-11-28",
        },
        method="DELETE",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return int(response.status)


def execute_plan(plan: Plan, deleter: Callable[[int], int]) -> tuple[list[int], list[dict[str, Any]]]:
    deleted: list[int] = []
    failed: list[dict[str, Any]] = []
    for artifact_id in plan.delete:
        try:
            status = deleter(artifact_id)
        except Exception as error:  # a live failure must never read as success
            failed.append({"artifact_id": artifact_id, "error": f"{type(error).__name__}: {error}"})
            continue
        if status == 204:
            deleted.append(artifact_id)
        else:
            failed.append({"artifact_id": artifact_id, "error": f"unexpected HTTP status {status}"})
    return deleted, failed


def authorize(args: argparse.Namespace, repo: str) -> None:
    """Fail closed before any network call unless deletion is explicitly authorized."""
    if not args.execute:
        return
    if not args.authorize_deletion:
        raise PurgeRefusal(
            "--execute requires --authorize-deletion <owner/repo>; refusing to delete"
        )
    if args.authorize_deletion != repo:
        raise PurgeRefusal(
            f"--authorize-deletion {args.authorize_deletion!r} does not match the audited "
            f"repository {repo!r}; refusing to delete"
        )


def build_report(
    handoff: Handoff,
    repo: str,
    plan: Plan,
    *,
    executed: bool,
    deleted: list[int],
    failed: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "handoff_dir": str(handoff.directory),
        "handoff_generated_at": handoff.generated_at,
        "repository": repo,
        "mode": "execute" if executed else "dry-run",
        "audit_candidate_count": len(handoff.candidates),
        "audit_withheld_count": len(handoff.withheld),
        "deletable_count": len(plan.delete),
        "deletable_size_bytes": plan.delete_bytes(handoff),
        "refused_count": len(plan.refused),
        "skipped_count": len(plan.skipped),
        "refused": plan.refused,
        "skipped": plan.skipped,
        "deletion_performed": bool(deleted),
        "deleted_count": len(deleted),
        "deleted": deleted,
        "failed_count": len(failed),
        "failed": failed,
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--handoff-dir", type=Path, default=DEFAULT_HANDOFF_DIR)
    parser.add_argument("--repo", help="override the audited repository (must match the audit)")
    parser.add_argument(
        "--execute",
        action="store_true",
        help="perform deletions; requires --authorize-deletion <owner/repo>",
    )
    parser.add_argument(
        "--authorize-deletion",
        metavar="OWNER/REPO",
        help="explicit deletion authorization; must equal the audited repository",
    )
    parser.add_argument(
        "--only",
        metavar="ID",
        type=int,
        nargs="+",
        help="restrict the plan to these candidate artifact ids",
    )
    parser.add_argument("--max-deletions", type=int, default=DEFAULT_MAX_DELETIONS)
    parser.add_argument("--max-handoff-age-days", type=int, default=DEFAULT_MAX_HANDOFF_AGE_DAYS)
    parser.add_argument(
        "--allow-stale-handoff",
        action="store_true",
        help="skip the handoff freshness check (candidacy is still re-derived live)",
    )
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--report-path", type=Path, help="also write the JSON report here")
    parser.add_argument("--self-test", action="store_true", help="run the offline refusal-path regressions")
    return parser.parse_args(argv)


def default_client_factory(audit: Any, repo: str) -> Any:
    del repo
    return audit.HttpClient(audit.github_token())


def default_deleter_factory(client: Any, repo: str) -> Callable[[int], int]:
    token = client.token
    return lambda artifact_id: delete_artifact(token, repo, artifact_id)


def run(
    args: argparse.Namespace,
    *,
    client_factory: Callable[[Any, str], Any] = default_client_factory,
    deleter_factory: Callable[[Any, str], Callable[[int], int]] = default_deleter_factory,
) -> int:
    audit = load_audit_module()
    handoff = load_handoff(
        args.handoff_dir,
        max_age_days=None if args.allow_stale_handoff else args.max_handoff_age_days,
    )
    repo = args.repo or handoff.repo
    if repo != handoff.repo:
        raise PurgeRefusal(
            f"--repo {repo!r} does not match the audited repository {handoff.repo!r}"
        )
    assert_rederivation_scope(audit, repo)
    authorize(args, repo)

    client = client_factory(audit, repo)
    live = live_candidacy(audit, client, repo, args.workers)
    plan = plan_purge(handoff, live, only=args.only)

    if len(plan.delete) > args.max_deletions:
        raise PurgeRefusal(
            f"plan would delete {len(plan.delete)} artifacts, above the --max-deletions cap "
            f"{args.max_deletions}; nothing was deleted"
        )

    deleted: list[int] = []
    failed: list[dict[str, Any]] = []
    if args.execute:
        deleted, failed = execute_plan(plan, deleter_factory(client, repo))

    report = build_report(handoff, repo, plan, executed=args.execute, deleted=deleted, failed=failed)
    rendered = json.dumps(report, indent=2, sort_keys=True)
    print(rendered)
    if args.report_path:
        args.report_path.parent.mkdir(parents=True, exist_ok=True)
        args.report_path.write_text(rendered + "\n", encoding="utf-8")
    return 1 if failed else 0


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.self_test:
        return self_test()
    try:
        return run(args)
    except PurgeRefusal as refusal:
        print(f"[purge] refused: {refusal}", file=sys.stderr)
        return 2


# --------------------------------------------------------------------------- #
# Offline regressions. Every refusal path the handoff prose used to carry.
# --------------------------------------------------------------------------- #


class _FakeClient:
    """Stands in for `audit.HttpClient` with no network access."""

    token = "fake-token"

    def __init__(self, artifacts: list[dict[str, Any]], releases: list[dict[str, Any]], pypi: dict[str, Any]):
        self.artifacts = artifacts
        self.releases = releases
        self.pypi = pypi

    def github_pages(self, path: str, key: str) -> list[dict[str, Any]]:
        if path.endswith("/actions/artifacts"):
            return self.artifacts
        if path.endswith("/releases"):
            return self.releases
        raise AssertionError(f"unexpected paged path: {path}")

    def json(self, url: str, authenticated: bool = True) -> Any:
        if "/pypi/" in url:
            return {"releases": self.pypi}
        match = re.search(r"/actions/runs/(\d+)$", url)
        if match:
            return {"name": "Release", "path": ".github/workflows/release.yml", "event": "release"}
        raise AssertionError(f"unexpected json url: {url}")


def _artifact(artifact_id: int, name: str, size: int, tag: str, *, expired: bool = False) -> dict[str, Any]:
    return {
        "id": artifact_id,
        "name": name,
        "size_in_bytes": size,
        "expired": expired,
        "created_at": "2026-09-01T00:00:00Z",
        "updated_at": "2026-09-01T00:00:00Z",
        "expires_at": "2026-12-01T00:00:00Z",
        "workflow_run": {"id": 900 + artifact_id, "head_branch": tag, "head_sha": "deadbeef"},
    }


def _write_handoff(
    directory: Path,
    candidates: list[dict[str, Any]],
    withheld: list[dict[str, Any]],
    *,
    generated_at: str,
    repo: str = "btakita/agent-doc",
    summary_overrides: dict[str, Any] | None = None,
) -> None:
    fields = [
        "artifact_id",
        "artifact_name",
        "workflow_name",
        "workflow_path",
        "workflow_run_id",
        "workflow_run_url",
        "workflow_event",
        "head_branch",
        "head_sha",
        "created_at",
        "updated_at",
        "expires_at",
        "size_bytes",
        "candidate",
        "counterpart_kind",
        "counterpart_name",
        "counterpart_url",
        "unresolved_category",
        "unresolved_reason",
    ]
    directory.mkdir(parents=True, exist_ok=True)
    for name, rows in ((CANDIDATES_CSV, candidates), (UNRESOLVED_CSV, withheld)):
        with (directory / name).open("w", encoding="utf-8", newline="") as handle:
            writer = csv.DictWriter(handle, fieldnames=fields, extrasaction="ignore", lineterminator="\n")
            writer.writeheader()
            for row in rows:
                writer.writerow({key: row.get(key, "") for key in fields})
    summary = {
        "generated_at": generated_at,
        "repository": repo,
        "candidate_count": len(candidates),
        "candidate_size_bytes": sum(int(row["size_bytes"]) for row in candidates),
        "unresolved_count": len(withheld),
        "unresolved_size_bytes": sum(int(row["size_bytes"]) for row in withheld),
        "deletion_performed": False,
    }
    summary.update(summary_overrides or {})
    (directory / SUMMARY_JSON).write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def _candidate_row(artifact_id: int, name: str, size: int, tag: str) -> dict[str, Any]:
    asset = f"{name}.tar.gz"
    return {
        "artifact_id": str(artifact_id),
        "artifact_name": name,
        "expires_at": "2026-12-01T00:00:00Z",
        "size_bytes": str(size),
        "candidate": "True",
        "counterpart_kind": "github_release",
        "counterpart_name": asset,
        "counterpart_url": f"https://example.invalid/{tag}/{asset}",
        "head_branch": tag,
    }


def _withheld_row(artifact_id: int, name: str, size: int, reason: str) -> dict[str, Any]:
    return {
        "artifact_id": str(artifact_id),
        "artifact_name": name,
        "expires_at": "2026-12-01T00:00:00Z",
        "size_bytes": str(size),
        "candidate": "False",
        "unresolved_category": reason,
        "unresolved_reason": reason,
    }


def _release(tag: str, asset_names: list[str]) -> dict[str, Any]:
    return {
        "tag_name": tag,
        "draft": False,
        "assets": [
            {"name": asset, "browser_download_url": f"https://example.invalid/{tag}/{asset}"}
            for asset in asset_names
        ],
    }


def _forbid_network(audit: Any, repo: str) -> Any:
    raise AssertionError(
        f"the self-test must refuse before contacting the API (repo {repo!r}); no client is available"
    )


def _forbid_deletion(client: Any, repo: str) -> Callable[[int], int]:
    def deleter(artifact_id: int) -> int:
        raise AssertionError(f"the self-test must never delete artifact {artifact_id} in {repo}")

    return deleter


def _quiet_run(args: argparse.Namespace, **kwargs: Any) -> int:
    """Run without letting the report JSON pollute `make check` output."""
    import contextlib
    import io

    with contextlib.redirect_stdout(io.StringIO()):
        return run(args, **kwargs)


def _refusal(argv: list[str], expected: str, *, client: Any | None = None) -> None:
    args = parse_args(argv)
    factory = (lambda audit, repo: client) if client is not None else _forbid_network
    try:
        _quiet_run(args, client_factory=factory, deleter_factory=_forbid_deletion)
    except PurgeRefusal as refusal:
        assert expected in str(refusal), f"expected {expected!r} in {refusal}"
        return
    raise AssertionError(f"expected a refusal containing {expected!r} for {argv}")


def self_test() -> int:
    """Refusal-path regressions for `#lzartifactpurgeexec`, run from `make check`."""
    import tempfile

    audit = load_audit_module()
    fresh = dt.datetime.now(dt.timezone.utc).isoformat()
    tag = "v0.35.0"
    linux = "agent-doc-x86_64-unknown-linux-gnu"
    macos = "agent-doc-x86_64-apple-darwin"

    def client_for(artifacts: list[dict[str, Any]], assets: list[str]) -> _FakeClient:
        return _FakeClient(artifacts, [_release(tag, assets)], {})

    def planned(
        directory: Path,
        artifacts: list[dict[str, Any]],
        assets: list[str],
        *,
        only: Iterable[int] | None = None,
        max_age_days: int | None = DEFAULT_MAX_HANDOFF_AGE_DAYS,
    ) -> tuple[Handoff, Plan]:
        handoff = load_handoff(directory, max_age_days=max_age_days)
        live = live_candidacy(audit, client_for(artifacts, assets), handoff.repo, 2)
        return handoff, plan_purge(handoff, live, only=only)

    # 1. The happy path: an exactly-matching candidate with a live durable counterpart.
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        _write_handoff(
            directory,
            [_candidate_row(11, linux, 1024, tag)],
            [_withheld_row(22, "github-pages", 2048, "GitHub Pages artifact has no Release/PyPI counterpart")],
            generated_at=fresh,
        )
        handoff, plan = planned(directory, [_artifact(11, linux, 1024, tag)], [f"{linux}.tar.gz"])
        assert plan.delete == [11], plan
        assert not plan.refused and not plan.skipped, plan
        assert plan.delete_bytes(handoff) == 1024

        # 2. Dry run is the default and deletes nothing even with a nonempty plan.
        report = build_report(handoff, handoff.repo, plan, executed=False, deleted=[], failed=[])
        assert report["mode"] == "dry-run" and report["deletion_performed"] is False, report
        assert report["deletable_count"] == 1 and report["deleted"] == [], report

        # 3. `--execute` without authorization refuses before any network call.
        _refusal(
            ["--handoff-dir", str(directory), "--execute"],
            "--execute requires --authorize-deletion",
        )

        # 4. Authorization must name the audited repository exactly.
        _refusal(
            [
                "--handoff-dir",
                str(directory),
                "--execute",
                "--authorize-deletion",
                "btakita/other-repo",
            ],
            "does not match the audited repository",
        )

        # 5. A mismatched --repo refuses.
        _refusal(["--handoff-dir", str(directory), "--repo", "someone/else"], "does not match the audited")

        # 6. Naming a withheld id explicitly refuses; withheld rows are never deletable.
        try:
            planned(directory, [_artifact(11, linux, 1024, tag)], [f"{linux}.tar.gz"], only=[22])
        except PurgeRefusal as refusal:
            assert "withheld artifact id" in str(refusal), refusal
        else:
            raise AssertionError("--only must refuse a withheld artifact id")

        # 7. Naming a non-candidate id refuses.
        try:
            planned(directory, [_artifact(11, linux, 1024, tag)], [f"{linux}.tar.gz"], only=[999])
        except PurgeRefusal as refusal:
            assert "non-candidate artifact id" in str(refusal), refusal
        else:
            raise AssertionError("--only must refuse a non-candidate artifact id")

        # 8. A live size change refuses the artifact and deletes nothing.
        _, drifted = planned(directory, [_artifact(11, linux, 4096, tag)], [f"{linux}.tar.gz"])
        assert drifted.delete == [], drifted
        assert drifted.refused[0]["reason"] == "live_metadata_drift", drifted
        assert "size_bytes" in drifted.refused[0]["detail"], drifted

        # 9. A live rename refuses the artifact.
        _, renamed = planned(directory, [_artifact(11, macos, 1024, tag)], [f"{macos}.tar.gz"])
        assert renamed.delete == [] and renamed.refused[0]["reason"] == "live_metadata_drift", renamed

        # 10. A vanished durable counterpart refuses the artifact: the audit said the
        #     release asset existed, the live re-derivation proves it no longer does.
        _, unbacked = planned(directory, [_artifact(11, linux, 1024, tag)], [])
        assert unbacked.delete == [], unbacked
        assert unbacked.refused[0]["reason"] == "no_live_durable_counterpart", unbacked

        # 11. An untagged live run has no candidacy at all, so the id is refused.
        _, untagged = planned(directory, [_artifact(11, linux, 1024, "main")], [f"{linux}.tar.gz"])
        assert untagged.delete == [], untagged
        assert untagged.refused[0]["reason"] == "no_live_durable_counterpart", untagged

        # 12. An id already gone from the live API is skipped, never "deleted".
        _, absent = planned(directory, [], [f"{linux}.tar.gz"])
        assert absent.delete == [] and absent.skipped[0]["reason"] == "absent_from_live_api", absent

        # 13. An expired live artifact is skipped.
        _, expired = planned(
            directory, [_artifact(11, linux, 1024, tag, expired=True)], [f"{linux}.tar.gz"]
        )
        assert expired.delete == [] and expired.skipped[0]["reason"] == "already_expired", expired

        # 14. The deletion cap is a fail-closed abort, not a truncation.
        _refusal(
            ["--handoff-dir", str(directory), "--max-deletions", "0"],
            "above the --max-deletions cap",
            client=client_for([_artifact(11, linux, 1024, tag)], [f"{linux}.tar.gz"]),
        )

        # 15. Execution deletes exactly the planned set, and a failure never reads as success.
        calls: list[int] = []

        def ok(artifact_id: int) -> int:
            calls.append(artifact_id)
            return 204

        deleted, failed = execute_plan(plan, ok)
        assert (deleted, failed, calls) == ([11], [], [11]), (deleted, failed, calls)

        def refused_by_api(artifact_id: int) -> int:
            return 403

        deleted, failed = execute_plan(plan, refused_by_api)
        assert deleted == [] and failed[0]["error"].startswith("unexpected HTTP status 403"), failed
        report = build_report(handoff, handoff.repo, plan, executed=True, deleted=deleted, failed=failed)
        assert report["deletion_performed"] is False and report["failed_count"] == 1, report

        def raised(artifact_id: int) -> int:
            raise urllib.error.HTTPError("u", 500, "boom", None, None)

        deleted, failed = execute_plan(plan, raised)
        assert deleted == [] and "HTTPError" in failed[0]["error"], failed

        # 16. Drift refusals never block the rest of the plan.
        _write_handoff(
            directory,
            [_candidate_row(11, linux, 1024, tag), _candidate_row(12, macos, 2048, tag)],
            [],
            generated_at=fresh,
        )
        _, mixed = planned(
            directory,
            [_artifact(11, linux, 1024, tag), _artifact(12, macos, 9999, tag)],
            [f"{linux}.tar.gz", f"{macos}.tar.gz"],
        )
        assert mixed.delete == [11], mixed
        assert [entry["artifact_id"] for entry in mixed.refused] == [12], mixed

        # 16b. End-to-end through the CLI: a default invocation with a nonempty plan
        #      exits 0 and never reaches the deleter.
        report_path = Path(tmp) / "report.json"
        exit_code = _quiet_run(
            parse_args(["--handoff-dir", str(directory), "--report-path", str(report_path)]),
            client_factory=lambda audit_mod, repo: client_for(
                [_artifact(11, linux, 1024, tag), _artifact(12, macos, 2048, tag)],
                [f"{linux}.tar.gz", f"{macos}.tar.gz"],
            ),
            deleter_factory=_forbid_deletion,
        )
        assert exit_code == 0, exit_code
        written = json.loads(report_path.read_text(encoding="utf-8"))
        assert written["mode"] == "dry-run" and written["deletion_performed"] is False, written
        assert written["deletable_count"] == 2 and written["deleted"] == [], written

        # 16c. End-to-end with full authorization: exactly the planned ids are deleted.
        executed: list[int] = []
        exit_code = _quiet_run(
            parse_args(
                [
                    "--handoff-dir",
                    str(directory),
                    "--execute",
                    "--authorize-deletion",
                    "btakita/agent-doc",
                ]
            ),
            client_factory=lambda audit_mod, repo: client_for(
                [_artifact(11, linux, 1024, tag), _artifact(12, macos, 2048, tag)],
                [f"{linux}.tar.gz", f"{macos}.tar.gz"],
            ),
            deleter_factory=lambda client, repo: lambda artifact_id: (
                executed.append(artifact_id) or 204
            ),
        )
        assert exit_code == 0 and executed == [11, 12], (exit_code, executed)

    # 17. A withheld id that also appears as a candidate aborts the whole run: the two
    #     CSVs disagree, so no row in either can be trusted.
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        _write_handoff(
            directory,
            [_candidate_row(11, linux, 1024, tag)],
            [_withheld_row(11, linux, 1024, "matching GitHub Release asset is missing")],
            generated_at=fresh,
        )
        _refusal(["--handoff-dir", str(directory)], "both list")

    # 18. A candidate with no durable counterpart URL aborts at load.
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        broken = _candidate_row(11, linux, 1024, tag)
        broken["counterpart_url"] = ""
        _write_handoff(directory, [broken], [], generated_at=fresh)
        _refusal(["--handoff-dir", str(directory)], "no durable counterpart URL")

    # 19. Summary totals that disagree with the CSVs abort at load.
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        _write_handoff(
            directory,
            [_candidate_row(11, linux, 1024, tag)],
            [],
            generated_at=fresh,
            summary_overrides={"candidate_count": 7},
        )
        _refusal(["--handoff-dir", str(directory)], "candidate_count=7")

    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        _write_handoff(
            directory,
            [_candidate_row(11, linux, 1024, tag)],
            [],
            generated_at=fresh,
            summary_overrides={"candidate_size_bytes": 1},
        )
        _refusal(["--handoff-dir", str(directory)], "candidate_size_bytes=1")

    # 20. A summary that already claims a deletion aborts at load.
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        _write_handoff(
            directory,
            [_candidate_row(11, linux, 1024, tag)],
            [],
            generated_at=fresh,
            summary_overrides={"deletion_performed": True},
        )
        _refusal(["--handoff-dir", str(directory)], "already records deletion_performed")

    # 21. A stale handoff aborts unless --allow-stale-handoff is explicit.
    with tempfile.TemporaryDirectory() as tmp:
        directory = Path(tmp) / "handoff"
        stale = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=30)).isoformat()
        _write_handoff(directory, [_candidate_row(11, linux, 1024, tag)], [], generated_at=stale)
        _refusal(["--handoff-dir", str(directory)], "days ago (limit")
        handoff, plan = planned(
            directory, [_artifact(11, linux, 1024, tag)], [f"{linux}.tar.gz"], max_age_days=None
        )
        assert plan.delete == [11], plan

    # 22. Missing handoff files abort rather than planning an empty purge.
    with tempfile.TemporaryDirectory() as tmp:
        _refusal(["--handoff-dir", str(Path(tmp) / "absent")], "handoff file is missing")

    # 23. Re-derivation scope: the audit module can only verify its own PyPI project.
    try:
        assert_rederivation_scope(audit, "btakita/some-other-project")
    except PurgeRefusal as refusal:
        assert "cannot verify" in str(refusal), refusal
    else:
        raise AssertionError("re-derivation must refuse a repository the audit cannot verify")
    assert_rederivation_scope(audit, "btakita/agent-doc")

    # 24. The committed handoff itself loads and structurally validates.
    if (DEFAULT_HANDOFF_DIR / SUMMARY_JSON).is_file():
        committed = load_handoff(DEFAULT_HANDOFF_DIR, max_age_days=None)
        assert len(committed.candidates) == 2781, len(committed.candidates)
        assert len(committed.withheld) == 466, len(committed.withheld)
        assert not (set(committed.candidates) & set(committed.withheld))

    print("[self-test] purge_actions_artifacts: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
