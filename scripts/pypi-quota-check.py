#!/usr/bin/env python3
"""Report PyPI project storage headroom and the limit-request decision, unauthenticated.

Why this exists: the recurring "are we about to hit the PyPI ceiling again?"
check used to require the authenticated `/manage/project/<name>/settings/` page,
which PyPI gates behind a password re-confirmation — so every re-check stalled on
a human. The PEP 691 Simple API (`Accept: application/vnd.pypi.simple.v1+json`)
carries a `size` for every file, so the project total is computable without
logging in. It is also the correct source: `pypi.org/pypi/<name>/json` has served
a stale CDN view that still listed deleted releases.

The verdict is mechanical. A ceiling is "in reach" only when remaining headroom
drops below `--headroom-floor-gib` OR the projected number of further releases at
the current post-cutover wheel size drops below `--release-floor`. Deleting
releases is reserved for that case: an unreachable ceiling is not a reason to
prune history.

Exit codes: 0 = headroom fine, 1 = ceiling in reach (act), 2 = check failed.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass, field

GIB = 1024**3
MIB = 1024**2
KIB = 1024

SIMPLE_ACCEPT = "application/vnd.pypi.simple.v1+json"


def version_key(version: str) -> tuple:
    """Sort key that orders plain dotted-numeric versions and never raises."""
    parts = []
    for chunk in version.split("."):
        parts.append((0, int(chunk), "") if chunk.isdigit() else (1, 0, chunk))
    return tuple(parts)


def version_of(filename: str, project: str) -> str | None:
    """Recover the release version from a distribution filename."""
    for prefix in {f"{project.replace('-', '_')}-", f"{project}-"}:
        if not filename.startswith(prefix):
            continue
        rest = filename[len(prefix) :]
        for suffix in (".tar.gz", ".zip"):
            if rest.endswith(suffix):
                return rest[: -len(suffix)]
        return rest.split("-", 1)[0]
    return None


def release_sizes(files: list[dict], project: str) -> dict[str, int]:
    """Total bytes PyPI stores per release. Raises on a file with no size."""
    sizes: dict[str, int] = {}
    for entry in files:
        filename = entry.get("filename", "")
        version = version_of(filename, project)
        if version is None:
            raise ValueError(f"cannot recover a version from {filename!r}")
        size = entry.get("size")
        if not isinstance(size, int):
            raise ValueError(f"{filename!r} has no usable size: {size!r}")
        sizes[version] = sizes.get(version, 0) + size
    return sizes


@dataclass
class Verdict:
    project: str
    limit_bytes: int
    total_bytes: int
    releases: int
    thin_releases: int
    thin_bytes: int
    fat_releases: int
    fat_bytes: int
    oldest: str | None
    newest: str | None
    oldest_fat: str | None
    newest_fat: str | None
    tranche: list[str]
    tranche_bytes: int
    ceiling_in_reach: bool
    reasons: list[str] = field(default_factory=list)

    @property
    def headroom_bytes(self) -> int:
        return self.limit_bytes - self.total_bytes

    @property
    def mean_thin_bytes(self) -> int:
        return self.thin_bytes // self.thin_releases if self.thin_releases else 0

    @property
    def releases_until_ceiling(self) -> int | None:
        mean = self.mean_thin_bytes
        return max(self.headroom_bytes, 0) // mean if mean else None


def assess(
    sizes: dict[str, int],
    *,
    project: str,
    limit_bytes: int,
    cutover: str,
    tranche_size: int,
    headroom_floor_bytes: int,
    release_floor: int,
) -> Verdict:
    """Pure arithmetic over the per-release sizes. No network, no clock."""
    ordered = sorted(sizes, key=version_key)
    cut = version_key(cutover)
    thin = [v for v in ordered if version_key(v) >= cut]
    fat = [v for v in ordered if version_key(v) < cut]
    tranche = fat[:tranche_size]
    total = sum(sizes.values())
    verdict = Verdict(
        project=project,
        limit_bytes=limit_bytes,
        total_bytes=total,
        releases=len(sizes),
        thin_releases=len(thin),
        thin_bytes=sum(sizes[v] for v in thin),
        fat_releases=len(fat),
        fat_bytes=sum(sizes[v] for v in fat),
        oldest=ordered[0] if ordered else None,
        newest=ordered[-1] if ordered else None,
        oldest_fat=fat[0] if fat else None,
        newest_fat=fat[-1] if fat else None,
        tranche=tranche,
        tranche_bytes=sum(sizes[v] for v in tranche),
        ceiling_in_reach=False,
    )
    if verdict.headroom_bytes < headroom_floor_bytes:
        verdict.reasons.append(
            f"headroom {verdict.headroom_bytes / GIB:.3f} GiB is below the "
            f"{headroom_floor_bytes / GIB:.3f} GiB floor"
        )
    remaining = verdict.releases_until_ceiling
    if remaining is not None and remaining < release_floor:
        verdict.reasons.append(
            f"only {remaining} further releases fit at the current "
            f"{verdict.mean_thin_bytes / KIB:.1f} KiB mean (floor {release_floor})"
        )
    verdict.ceiling_in_reach = bool(verdict.reasons)
    return verdict


def fetch_simple(project: str, index: str, timeout: float) -> dict:
    request = urllib.request.Request(
        f"{index.rstrip('/')}/{project}/",
        headers={"Accept": SIMPLE_ACCEPT},
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.load(response)


def issue_state(issue: str) -> dict:
    """Best-effort limit-request status via `gh`. Never fails the check."""
    try:
        repo, number = issue.rsplit("#", 1)
    except ValueError:
        return {"issue": issue, "status": "unparsed"}
    try:
        raw = subprocess.run(
            [
                "gh",
                "api",
                f"repos/{repo}/issues/{number}",
                "--jq",
                "{state,title,created_at,updated_at,comments}",
            ],
            capture_output=True,
            text=True,
            timeout=30,
            check=True,
        ).stdout
    except (OSError, subprocess.SubprocessError) as error:
        return {"issue": issue, "status": "unavailable", "error": str(error)}
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as error:
        return {"issue": issue, "status": "unparsed", "error": str(error)}
    data["issue"] = issue
    # One comment on an open PyPI limit request is the github-actions
    # acknowledgement bot, not a decision.
    data["status"] = "decided" if data.get("state") == "closed" else "awaiting_decision"
    return data


def report(verdict: Verdict, issue: dict | None) -> str:
    lines = [
        f"{verdict.project}: {verdict.total_bytes / GIB:.3f} GiB of "
        f"{verdict.limit_bytes / GIB:.1f} GiB "
        f"({100 * verdict.total_bytes / verdict.limit_bytes:.1f}%), "
        f"headroom {verdict.headroom_bytes / GIB:.3f} GiB",
        f"  releases: {verdict.releases} "
        f"({verdict.oldest}..{verdict.newest})",
        f"  post-cutover: {verdict.thin_releases} releases, "
        f"{verdict.thin_bytes / MIB:.2f} MiB "
        f"(mean {verdict.mean_thin_bytes / KIB:.1f} KiB)",
        f"  pre-cutover:  {verdict.fat_releases} releases, "
        f"{verdict.fat_bytes / GIB:.3f} GiB"
        + (
            f" ({verdict.oldest_fat}..{verdict.newest_fat})"
            if verdict.oldest_fat
            else ""
        ),
    ]
    remaining = verdict.releases_until_ceiling
    if remaining is not None:
        lines.append(f"  further releases before the ceiling: {remaining:,}")
    if verdict.tranche:
        lines.append(
            f"  next deletable tranche IF a ceiling is in reach: "
            f"{verdict.tranche[0]}..{verdict.tranche[-1]} "
            f"({len(verdict.tranche)} releases, "
            f"{verdict.tranche_bytes / GIB:.3f} GiB)"
        )
    if issue is not None:
        suffix = f", updated {issue['updated_at']}" if issue.get("updated_at") else ""
        lines.append(
            f"  limit request {issue['issue']}: {issue['status']}"
            f" (state={issue.get('state', '?')},"
            f" comments={issue.get('comments', '?')}{suffix})"
        )
    if verdict.ceiling_in_reach:
        lines.append("  VERDICT: ceiling in reach — " + "; ".join(verdict.reasons))
        lines.append(
            "  Delete the tranche above only after confirming the limit request "
            "will not land first, and verify deletions from the authenticated "
            "/manage/project/ pages."
        )
    else:
        lines.append(
            "  VERDICT: headroom fine — do NOT delete releases. "
            "Growth is bounded by the post-cutover wheel size."
        )
    return "\n".join(lines)


SELF_TEST_FILES = [
    # Two pre-cutover releases at fat-wheel size, one post-cutover thin release.
    {"filename": "agent_doc-0.35.113-py3-none-any.whl", "size": 60 * MIB},
    {"filename": "agent_doc-0.35.113.tar.gz", "size": 5 * MIB},
    {"filename": "agent_doc-0.35.114-py3-none-any.whl", "size": 65 * MIB},
    {"filename": "agent_doc-0.35.383-py3-none-any.whl", "size": 30 * KIB},
    {"filename": "agent_doc-0.35.383.tar.gz", "size": 5 * KIB},
]


def self_test() -> str:
    project = "agent-doc"
    sizes = release_sizes(SELF_TEST_FILES, project)
    assert sizes == {
        "0.35.113": 65 * MIB,
        "0.35.114": 65 * MIB,
        "0.35.383": 35 * KIB,
    }, sizes
    assert version_of("agent_doc-0.35.417.tar.gz", project) == "0.35.417"
    assert version_of("agent_doc-0.35.417-py3-none-any.whl", project) == "0.35.417"
    assert version_of("other-1.0.whl", project) is None
    # Dotted-numeric ordering must be numeric, not lexicographic: 0.35.9 < 0.35.113.
    assert version_key("0.35.9") < version_key("0.35.113")
    assert sorted(["0.35.113", "0.35.9"], key=version_key) == ["0.35.9", "0.35.113"]
    # A non-numeric chunk must sort without raising.
    assert version_key("0.35.1rc1") > version_key("0.35.1")

    roomy = assess(
        sizes,
        project=project,
        limit_bytes=10 * GIB,
        cutover="0.35.383",
        tranche_size=40,
        headroom_floor_bytes=1 * GIB,
        release_floor=200,
    )
    assert roomy.releases == 3, roomy.releases
    assert roomy.thin_releases == 1 and roomy.fat_releases == 2
    assert roomy.oldest == "0.35.113" and roomy.newest == "0.35.383"
    assert roomy.newest_fat == "0.35.114"
    assert roomy.tranche == ["0.35.113", "0.35.114"], roomy.tranche
    assert roomy.tranche_bytes == 130 * MIB
    assert not roomy.ceiling_in_reach, roomy.reasons
    assert roomy.releases_until_ceiling is not None
    assert roomy.releases_until_ceiling > 200

    # A ceiling is in reach when the headroom floor bites...
    tight = assess(
        sizes,
        project=project,
        limit_bytes=140 * MIB,
        cutover="0.35.383",
        tranche_size=40,
        headroom_floor_bytes=1 * GIB,
        release_floor=0,
    )
    assert tight.ceiling_in_reach and "headroom" in tight.reasons[0], tight.reasons

    # ...and independently when the projected release count does.
    crowded = assess(
        sizes,
        project=project,
        limit_bytes=140 * MIB,
        cutover="0.35.383",
        tranche_size=40,
        headroom_floor_bytes=0,
        release_floor=1_000_000,
    )
    assert crowded.ceiling_in_reach
    assert any("further releases" in reason for reason in crowded.reasons), crowded.reasons

    # With no post-cutover release the projection is unknown, not zero: an
    # unknown mean must never manufacture a ceiling-in-reach verdict.
    fat_only = assess(
        {"0.35.113": 65 * MIB},
        project=project,
        limit_bytes=10 * GIB,
        cutover="0.35.383",
        tranche_size=40,
        headroom_floor_bytes=1 * GIB,
        release_floor=1_000_000,
    )
    assert fat_only.releases_until_ceiling is None
    assert not fat_only.ceiling_in_reach, fat_only.reasons

    # A file with no size is a failed check, never a smaller total.
    try:
        release_sizes([{"filename": "agent_doc-0.35.1.tar.gz"}], project)
    except ValueError:
        pass
    else:  # pragma: no cover - guarded by the assert below
        raise AssertionError("a missing size must fail the check")

    assert "VERDICT: headroom fine" in report(roomy, None)
    assert "VERDICT: ceiling in reach" in report(tight, None)
    decided = report(roomy, {"issue": "pypi/support#1", "status": "decided", "state": "closed"})
    assert "pypi/support#1: decided" in decided
    return "[self-test] pypi-quota-check: ok"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--project", default="agent-doc")
    parser.add_argument("--index", default="https://pypi.org/simple")
    parser.add_argument("--limit-gib", type=float, default=10.0)
    parser.add_argument(
        "--thin-cutover",
        default="0.35.383",
        help="first release built as a thin bootstrap wheel (#pypislim)",
    )
    parser.add_argument("--tranche", type=int, default=40)
    parser.add_argument(
        "--headroom-floor-gib",
        type=float,
        default=1.0,
        help="below this remaining headroom, a ceiling counts as in reach",
    )
    parser.add_argument(
        "--release-floor",
        type=int,
        default=200,
        help="below this many further releases, a ceiling counts as in reach",
    )
    parser.add_argument("--issue", default="pypi/support#12214")
    parser.add_argument("--no-issue", action="store_true")
    parser.add_argument("--timeout", type=float, default=30.0)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)

    if args.self_test:
        print(self_test())
        return 0

    try:
        index = fetch_simple(args.project, args.index, args.timeout)
        sizes = release_sizes(index.get("files", []), args.project)
    except (urllib.error.URLError, OSError, ValueError, json.JSONDecodeError) as error:
        print(f"pypi-quota-check: failed to read the simple index: {error}", file=sys.stderr)
        return 2
    if not sizes:
        print(f"pypi-quota-check: {args.project} has no files on {args.index}", file=sys.stderr)
        return 2

    verdict = assess(
        sizes,
        project=args.project,
        limit_bytes=int(args.limit_gib * GIB),
        cutover=args.thin_cutover,
        tranche_size=args.tranche,
        headroom_floor_bytes=int(args.headroom_floor_gib * GIB),
        release_floor=args.release_floor,
    )
    issue = None if args.no_issue else issue_state(args.issue)
    if args.json:
        payload = {
            "project": verdict.project,
            "limit_bytes": verdict.limit_bytes,
            "total_bytes": verdict.total_bytes,
            "headroom_bytes": verdict.headroom_bytes,
            "releases": verdict.releases,
            "oldest": verdict.oldest,
            "newest": verdict.newest,
            "thin_releases": verdict.thin_releases,
            "thin_bytes": verdict.thin_bytes,
            "mean_thin_bytes": verdict.mean_thin_bytes,
            "fat_releases": verdict.fat_releases,
            "fat_bytes": verdict.fat_bytes,
            "oldest_fat": verdict.oldest_fat,
            "newest_fat": verdict.newest_fat,
            "releases_until_ceiling": verdict.releases_until_ceiling,
            "tranche": verdict.tranche,
            "tranche_bytes": verdict.tranche_bytes,
            "ceiling_in_reach": verdict.ceiling_in_reach,
            "reasons": verdict.reasons,
            "limit_request": issue,
        }
        print(json.dumps(payload, indent=2))
    else:
        print(report(verdict, issue))
    return 1 if verdict.ceiling_in_reach else 0


if __name__ == "__main__":
    sys.exit(main())
