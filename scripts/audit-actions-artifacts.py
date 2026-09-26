#!/usr/bin/env python3
"""Audit GitHub Actions artifacts against durable Release and PyPI copies.

This command is deliberately read-only with respect to GitHub. It enumerates
artifacts, resolves their workflow runs, and emits reports; it never calls an
artifact deletion endpoint.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import csv
import datetime as dt
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from collections import defaultdict
from pathlib import Path
from typing import Any, Iterable


GITHUB_API = "https://api.github.com"
PYPI_API = "https://pypi.org/pypi/agent-doc/json"
VERSION_RE = re.compile(r"^v?(\d+\.\d+\.\d+(?:[A-Za-z0-9.+-]*)?)$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="btakita/agent-doc")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--workers", type=int, default=8)
    return parser.parse_args()


def github_token() -> str:
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        return token
    completed = subprocess.run(
        ["gh", "auth", "token"],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout.strip()


class HttpClient:
    def __init__(self, token: str) -> None:
        self.token = token

    def json(self, url: str, authenticated: bool = True) -> Any:
        headers = {
            "Accept": "application/vnd.github+json",
            "User-Agent": "agent-doc-artifact-audit",
            "X-GitHub-Api-Version": "2022-11-28",
        }
        if authenticated:
            headers["Authorization"] = f"Bearer {self.token}"
        request = urllib.request.Request(url, headers=headers)
        for attempt in range(5):
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    return json.load(response)
            except urllib.error.HTTPError as error:
                if error.code not in {429, 500, 502, 503, 504} or attempt == 4:
                    raise
                time.sleep(2**attempt)
            except urllib.error.URLError:
                if attempt == 4:
                    raise
                time.sleep(2**attempt)
        raise AssertionError("retry loop exhausted")

    def github_pages(self, path: str, key: str) -> list[dict[str, Any]]:
        page = 1
        records: list[dict[str, Any]] = []
        while True:
            separator = "&" if "?" in path else "?"
            payload = self.json(f"{GITHUB_API}{path}{separator}per_page=100&page={page}")
            batch = payload[key] if isinstance(payload, dict) else payload
            if not batch:
                break
            records.extend(batch)
            if len(batch) < 100:
                break
            page += 1
        return records


def atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        newline="",
        dir=path.parent,
        prefix=f".{path.name}.",
        delete=False,
    ) as handle:
        handle.write(content)
        temporary = Path(handle.name)
    os.replace(temporary, path)


def write_csv(path: Path, rows: Iterable[dict[str, Any]], fields: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        newline="",
        dir=path.parent,
        prefix=f".{path.name}.",
        delete=False,
    ) as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=fields,
            extrasaction="ignore",
            lineterminator="\n",
        )
        writer.writeheader()
        writer.writerows(rows)
        temporary = Path(handle.name)
    os.replace(temporary, path)


def bytes_label(value: int) -> str:
    return f"{value:,} bytes ({value / 1_000_000_000:.2f} GB; {value / 2**30:.2f} GiB)"


def version_for(artifact: dict[str, Any]) -> str | None:
    branch = artifact.get("workflow_run", {}).get("head_branch") or ""
    match = VERSION_RE.fullmatch(branch)
    return match.group(1) if match else None


def release_asset_name(artifact_name: str) -> str | None:
    if not artifact_name.startswith("agent-doc-"):
        return None
    suffix = ".zip" if artifact_name.endswith("windows-msvc") else ".tar.gz"
    return f"{artifact_name}{suffix}"


def pypi_file(
    artifact_name: str,
    version: str,
    files: list[dict[str, Any]],
) -> dict[str, Any] | None:
    prefix = f"agent_doc-{version}-"
    predicates = {
        "wheel-bootstrap": lambda name: name.endswith("py3-none-any.whl"),
        "wheel-x86_64-unknown-linux-gnu": lambda name: "manylinux" in name and "x86_64" in name,
        "wheel-x86_64-apple-darwin": lambda name: "macosx" in name and "x86_64" in name,
        "wheel-aarch64-apple-darwin": lambda name: "macosx" in name and "arm64" in name,
        "wheel-x86_64-pc-windows-msvc": lambda name: name.endswith("win_amd64.whl"),
    }
    predicate = predicates.get(artifact_name)
    if predicate is None:
        return None
    matches = [file for file in files if file["filename"].startswith(prefix) and predicate(file["filename"])]
    if len(matches) > 1:
        raise RuntimeError(f"ambiguous PyPI counterpart for {artifact_name} {version}: {matches}")
    return matches[0] if matches else None


def counterpart(
    artifact: dict[str, Any],
    releases: dict[str, dict[str, dict[str, Any]]],
    pypi_releases: dict[str, list[dict[str, Any]]],
) -> tuple[str, str, str, str]:
    name = artifact["name"]
    if name == "github-pages":
        return "", "", "", "GitHub Pages deployment artifact has no Release/PyPI counterpart"

    version = version_for(artifact)
    if version is None:
        return "", "", "", "head branch is not a version tag"

    asset_name = release_asset_name(name)
    if asset_name is not None:
        tag = artifact["workflow_run"]["head_branch"]
        assets = releases.get(tag) or releases.get(f"v{version}") or {}
        asset = assets.get(asset_name)
        if asset:
            return "github_release", asset_name, asset["browser_download_url"], ""
        return "", "", "", f"release {tag} has no {asset_name} asset"

    if name.startswith("wheel-"):
        file = pypi_file(name, version, pypi_releases.get(version, []))
        if file:
            return "pypi", file["filename"], file["url"], ""
        return "", "", "", f"PyPI {version} has no file for {name}"

    return "", "", "", "unsupported artifact class"


def unresolved_category(reason: str) -> str:
    if reason.startswith("GitHub Pages deployment artifact"):
        return "GitHub Pages artifact has no Release/PyPI counterpart"
    if reason == "head branch is not a version tag":
        return "workflow run is not associated with a version tag"
    if reason.startswith("PyPI "):
        return "matching PyPI platform file is missing"
    if reason.startswith("release "):
        return "matching GitHub Release asset is missing"
    if reason.startswith("workflow run lookup failed"):
        return "workflow run metadata lookup failed"
    return reason


def fetch_runs(
    client: HttpClient,
    repo: str,
    run_ids: list[int],
    workers: int,
) -> tuple[dict[int, dict[str, Any]], dict[int, str]]:
    runs: dict[int, dict[str, Any]] = {}
    errors: dict[int, str] = {}

    def fetch(run_id: int) -> tuple[int, dict[str, Any]]:
        return run_id, client.json(f"{GITHUB_API}/repos/{repo}/actions/runs/{run_id}")

    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
        future_by_id = {executor.submit(fetch, run_id): run_id for run_id in run_ids}
        for future in concurrent.futures.as_completed(future_by_id):
            run_id = future_by_id[future]
            try:
                fetched_id, payload = future.result()
                runs[fetched_id] = payload
            except Exception as error:  # retain an actionable unresolved row
                errors[run_id] = f"{type(error).__name__}: {error}"
    return runs, errors


def report_markdown(
    repo: str,
    generated_at: str,
    rows: list[dict[str, Any]],
    candidates: list[dict[str, Any]],
    unresolved: list[dict[str, Any]],
) -> str:
    total_size = sum(row["size_bytes"] for row in rows)
    candidate_size = sum(row["size_bytes"] for row in candidates)
    unresolved_size = sum(row["size_bytes"] for row in unresolved)

    grouped: dict[tuple[str, str], list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        grouped[(row["workflow_name"], row["artifact_name"])].append(row)

    reasons: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in unresolved:
        reasons[row["unresolved_category"]].append(row)

    lines = [
        "# GitHub Actions artifact durability audit",
        "",
        f"Generated: {generated_at}",
        "",
        f"Repository: [{repo}](https://github.com/{repo})",
        "",
        "No artifacts were deleted. This report is a read-only candidate handoff.",
        "",
        "## Result",
        "",
        f"- Live nonexpired artifacts: {len(rows):,}, totaling {bytes_label(total_size)}.",
        f"- Verified delete candidates: {len(candidates):,}, totaling {bytes_label(candidate_size)}.",
        f"- Not candidates: {len(unresolved):,}, totaling {bytes_label(unresolved_size)}.",
        "- Candidate proof requires an exact tagged version plus an exact target-specific GitHub Release asset or PyPI filename.",
        "- The candidate CSV is sorted by workflow, run, artifact class, and artifact id; each row records id, size, expiry, run URL, and counterpart URL.",
        "",
        "## By workflow and artifact class",
        "",
        "| Workflow | Artifact class | Artifacts | Verified | Unresolved | Candidate bytes |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for (workflow, artifact_name), group in sorted(grouped.items()):
        verified = [row for row in group if row["candidate"]]
        lines.append(
            f"| {workflow} | {artifact_name} | {len(group):,} | {len(verified):,} | "
            f"{len(group) - len(verified):,} | {sum(row['size_bytes'] for row in verified):,} |"
        )

    lines.extend(
        [
            "",
            "## Unresolved classes",
            "",
            "| Reason | Artifacts | Bytes |",
            "|---|---:|---:|",
        ]
    )
    for reason, group in sorted(reasons.items(), key=lambda item: (-len(item[1]), item[0])):
        lines.append(f"| {reason.replace('|', '\\|')} | {len(group):,} | {sum(row['size_bytes'] for row in group):,} |")

    lines.extend(
        [
            "",
            "## Method",
            "",
            f"- Enumerated every page of [the Actions artifacts API](https://api.github.com/repos/{repo}/actions/artifacts).",
            "- Resolved every distinct workflow run id through the Actions runs API so the CSV can be grouped by workflow and run.",
            f"- Enumerated every page of [GitHub Releases](https://api.github.com/repos/{repo}/releases) and matched native artifacts to the exact tag and archive filename.",
            "- Read the [PyPI project JSON](https://pypi.org/pypi/agent-doc/json) and matched wheel artifacts to the exact version and platform filename.",
            "- Excluded untagged artifacts, GitHub Pages artifacts, missing platform files, and any class without an exact durable URL.",
            "",
            "## Purging",
            "",
            "This generator never deletes. `scripts/purge-actions-artifacts.py` is the only",
            "command that may call the artifact deletion endpoint, and it is dry run by default:",
            "",
            "- Deletion requires **both** `--execute` and `--authorize-deletion <owner/repo>`,",
            "  whose value must equal this audit's repository.",
            "- Every candidate is re-derived against the live Actions API at execution time using",
            "  this generator's own candidacy rule. A live artifact that is gone, expired, renamed,",
            "  resized, or no longer backed by an exact durable counterpart is refused.",
            "- Every artifact id in `artifact-unresolved.csv` is refused unconditionally, including",
            "  when named explicitly with `--only`.",
            "- A structurally inconsistent handoff (candidate/withheld overlap, a candidate with no",
            "  counterpart, summary totals that disagree with the CSVs, a generation older than",
            "  `--max-handoff-age-days`) aborts before any network call.",
            "- A plan above `--max-deletions` aborts rather than deleting a truncated subset.",
            "",
            "Run `python3 scripts/purge-actions-artifacts.py --self-test` for the offline",
            "refusal-path regressions; `make check` runs it via `artifact-purge-check`.",
            "",
            "## Files",
            "",
            "- `artifact-delete-candidates.csv` — operator delete-candidate list; this generator issues no deletion.",
            "- `artifact-unresolved.csv` — artifacts withheld from the candidate list and the exact reason. The purge executor refuses every id listed here.",
            "- `artifact-audit-summary.json` — machine-readable totals and group summaries.",
            "",
        ]
    )
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    client = HttpClient(github_token())
    generated_at = dt.datetime.now(dt.timezone.utc).isoformat()

    artifacts = client.github_pages(f"/repos/{args.repo}/actions/artifacts", "artifacts")
    artifacts = [artifact for artifact in artifacts if not artifact["expired"]]
    releases_payload = client.github_pages(f"/repos/{args.repo}/releases", "releases")
    releases = {
        release["tag_name"]: {asset["name"]: asset for asset in release["assets"]}
        for release in releases_payload
        if not release["draft"]
    }
    pypi = client.json(PYPI_API, authenticated=False)
    pypi_releases = pypi["releases"]

    run_ids = sorted({artifact["workflow_run"]["id"] for artifact in artifacts})
    runs, run_errors = fetch_runs(client, args.repo, run_ids, args.workers)

    rows: list[dict[str, Any]] = []
    for artifact in artifacts:
        run_id = artifact["workflow_run"]["id"]
        run = runs.get(run_id, {})
        kind, counterpart_name, counterpart_url, reason = counterpart(
            artifact,
            releases,
            pypi_releases,
        )
        if run_id in run_errors:
            reason = f"workflow run lookup failed: {run_errors[run_id]}"
            kind = counterpart_name = counterpart_url = ""
        rows.append(
            {
                "artifact_id": artifact["id"],
                "artifact_name": artifact["name"],
                "workflow_name": run.get("name", "<unavailable>"),
                "workflow_path": run.get("path", ""),
                "workflow_run_id": run_id,
                "workflow_run_url": run.get("html_url", ""),
                "workflow_event": run.get("event", ""),
                "head_branch": artifact["workflow_run"].get("head_branch") or "",
                "head_sha": artifact["workflow_run"].get("head_sha") or "",
                "created_at": artifact["created_at"],
                "updated_at": artifact["updated_at"],
                "expires_at": artifact["expires_at"],
                "size_bytes": artifact["size_in_bytes"],
                "candidate": bool(counterpart_url),
                "counterpart_kind": kind,
                "counterpart_name": counterpart_name,
                "counterpart_url": counterpart_url,
                "unresolved_category": unresolved_category(reason) if reason else "",
                "unresolved_reason": reason,
            }
        )

    rows.sort(key=lambda row: (row["workflow_name"], row["workflow_run_id"], row["artifact_name"], row["artifact_id"]))
    candidates = [row for row in rows if row["candidate"]]
    unresolved = [row for row in rows if not row["candidate"]]
    fields = list(rows[0].keys()) if rows else []

    report_dir = args.output_dir
    write_csv(report_dir / "artifact-delete-candidates.csv", candidates, fields)
    write_csv(report_dir / "artifact-unresolved.csv", unresolved, fields)

    groups: dict[str, dict[str, int]] = defaultdict(lambda: {"count": 0, "size_bytes": 0})
    for row in rows:
        key = f"{row['workflow_name']} :: {row['artifact_name']} :: {'candidate' if row['candidate'] else 'unresolved'}"
        groups[key]["count"] += 1
        groups[key]["size_bytes"] += row["size_bytes"]
    unresolved_categories: dict[str, dict[str, int]] = defaultdict(lambda: {"count": 0, "size_bytes": 0})
    for row in unresolved:
        category = row["unresolved_category"]
        unresolved_categories[category]["count"] += 1
        unresolved_categories[category]["size_bytes"] += row["size_bytes"]
    summary = {
        "generated_at": generated_at,
        "repository": args.repo,
        "source_artifact_count": len(rows),
        "source_size_bytes": sum(row["size_bytes"] for row in rows),
        "distinct_workflow_runs": len(run_ids),
        "github_release_count": len(releases),
        "pypi_release_count": len(pypi_releases),
        "candidate_count": len(candidates),
        "candidate_size_bytes": sum(row["size_bytes"] for row in candidates),
        "unresolved_count": len(unresolved),
        "unresolved_size_bytes": sum(row["size_bytes"] for row in unresolved),
        "unresolved_categories": unresolved_categories,
        "groups": groups,
        "deletion_performed": False,
    }
    atomic_write(report_dir / "artifact-audit-summary.json", json.dumps(summary, indent=2, sort_keys=True) + "\n")
    atomic_write(
        report_dir / "README.md",
        report_markdown(args.repo, generated_at, rows, candidates, unresolved),
    )

    print(json.dumps(summary, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
