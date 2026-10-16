#!/usr/bin/env python3
"""Wait for and prove that an agent-doc tag is public on GitHub and PyPI."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
import urllib.request
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPO = "btakita/agent-doc"
PLATFORM_ASSETS = {
    "agent-doc-x86_64-unknown-linux-gnu.tar.gz",
    "agent-doc-x86_64-unknown-linux-musl.tar.gz",
    "agent-doc-aarch64-unknown-linux-gnu.tar.gz",
    "agent-doc-x86_64-apple-darwin.tar.gz",
    "agent-doc-aarch64-apple-darwin.tar.gz",
    "agent-doc-x86_64-pc-windows-msvc.zip",
    "SHA256SUMS",
    "EDITOR-PACKAGES.sha256",
    "agent-doc-jetbrains-classic.xml",
    "agent-doc-jetbrains-262.xml",
    "agent-doc-jetbrains-263.xml",
}


def command_json(command: Sequence[str]) -> Any:
    output = subprocess.check_output(command, cwd=ROOT, text=True)
    return json.loads(output)


def tag_commit(tag: str) -> str:
    return subprocess.check_output(
        ["git", "rev-parse", f"{tag}^{{commit}}"], cwd=ROOT, text=True
    ).strip()


def matching_workflow_run(runs: Sequence[dict[str, Any]], sha: str) -> dict[str, Any] | None:
    matches = [run for run in runs if run.get("headSha") == sha]
    return max(matches, key=lambda run: str(run.get("createdAt", "")), default=None)


def wait_for_workflow(
    workflow: str,
    tag: str,
    sha: str,
    repo: str,
    *,
    deadline: float,
    poll_seconds: float,
    clock: Callable[[], float] = time.monotonic,
    sleeper: Callable[[float], None] = time.sleep,
) -> dict[str, Any]:
    while clock() < deadline:
        runs = command_json(
            [
                "gh",
                "run",
                "list",
                "--repo",
                repo,
                "--workflow",
                workflow,
                "--branch",
                tag,
                "--limit",
                "20",
                "--json",
                "databaseId,headSha,status,conclusion,createdAt,url",
            ]
        )
        run = matching_workflow_run(runs, sha)
        if run is not None:
            url = run.get("url", "<unknown>")
            print(f"release publication: watching {workflow} {url}", flush=True)
            result = subprocess.run(
                [
                    "gh",
                    "run",
                    "watch",
                    str(run["databaseId"]),
                    "--repo",
                    repo,
                    "--exit-status",
                ],
                cwd=ROOT,
                check=False,
            )
            if result.returncode != 0:
                raise RuntimeError(f"{workflow} failed for {tag}: {url}")
            return run
        sleeper(poll_seconds)
    raise RuntimeError(f"timed out waiting for {workflow} to start for {tag} ({sha})")


def missing_release_assets(asset_names: set[str]) -> list[str]:
    missing = sorted(PLATFORM_ASSETS - asset_names)
    families = {
        "VS Code package": lambda name: name.startswith("agent-doc-") and name.endswith(".vsix"),
        "JetBrains classic package": lambda name: name.startswith("agent-doc-jetbrains-")
        and "-262-" not in name
        and "-263-" not in name
        and name.endswith(".zip"),
        "JetBrains 262 package": lambda name: name.startswith("agent-doc-jetbrains-262-")
        and name.endswith(".zip"),
        "JetBrains 263 package": lambda name: name.startswith("agent-doc-jetbrains-263-")
        and name.endswith(".zip"),
    }
    missing.extend(label for label, predicate in families.items() if not any(map(predicate, asset_names)))
    return missing


def verify_github_release(tag: str, repo: str) -> None:
    release = command_json(
        [
            "gh",
            "release",
            "view",
            tag,
            "--repo",
            repo,
            "--json",
            "tagName,isDraft,isPrerelease,assets,url",
        ]
    )
    if release.get("tagName") != tag or release.get("isDraft"):
        raise RuntimeError(f"GitHub Release {tag} is absent or still a draft")
    asset_names = {asset["name"] for asset in release.get("assets", [])}
    missing = missing_release_assets(asset_names)
    if missing:
        raise RuntimeError(f"GitHub Release {tag} is incomplete; missing: {', '.join(missing)}")
    print(
        f"release publication: GitHub Release complete assets={len(asset_names)} url={release.get('url')}",
        flush=True,
    )


def pypi_has_wheel(index: dict[str, Any], version: str) -> bool:
    expected = f"agent_doc-{version}-py3-none-any.whl"
    return any(file.get("filename") == expected for file in index.get("files", []))


def pypi_simple_index() -> dict[str, Any]:
    request = urllib.request.Request(
        "https://pypi.org/simple/agent-doc/",
        headers={"Accept": "application/vnd.pypi.simple.v1+json"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def wait_for_pypi(version: str, *, deadline: float, poll_seconds: float) -> None:
    while time.monotonic() < deadline:
        if pypi_has_wheel(pypi_simple_index(), version):
            print(
                f"release publication: PyPI wheel public agent_doc-{version}-py3-none-any.whl",
                flush=True,
            )
            return
        time.sleep(poll_seconds)
    raise RuntimeError(f"timed out waiting for agent-doc {version} on the PyPI simple index")


def verify(tag: str, repo: str, timeout_seconds: float, poll_seconds: float) -> None:
    if not tag.startswith("v") or len(tag) == 1:
        raise RuntimeError(f"expected a v* release tag, got {tag!r}")
    sha = tag_commit(tag)
    deadline = time.monotonic() + timeout_seconds
    wait_for_workflow(
        "release.yml",
        tag,
        sha,
        repo,
        deadline=deadline,
        poll_seconds=poll_seconds,
    )
    verify_github_release(tag, repo)
    wait_for_workflow(
        "pypi.yml",
        tag,
        sha,
        repo,
        deadline=deadline,
        poll_seconds=poll_seconds,
    )
    wait_for_pypi(tag.removeprefix("v"), deadline=deadline, poll_seconds=poll_seconds)


def self_test() -> None:
    runs = [
        {"databaseId": 1, "headSha": "other", "createdAt": "2026-01-01T00:00:00Z"},
        {"databaseId": 2, "headSha": "wanted", "createdAt": "2026-01-01T00:00:00Z"},
        {"databaseId": 3, "headSha": "wanted", "createdAt": "2026-01-02T00:00:00Z"},
    ]
    assert matching_workflow_run(runs, "wanted")["databaseId"] == 3
    assert matching_workflow_run(runs, "missing") is None

    complete_assets = set(PLATFORM_ASSETS)
    complete_assets.update(
        {
            "agent-doc-1.2.3.vsix",
            "agent-doc-jetbrains-1.2.3.zip",
            "agent-doc-jetbrains-262-1.2.3.zip",
            "agent-doc-jetbrains-263-1.2.3.zip",
        }
    )
    assert missing_release_assets(complete_assets) == []
    complete_assets.remove("SHA256SUMS")
    assert missing_release_assets(complete_assets) == ["SHA256SUMS"]

    index = {"files": [{"filename": "agent_doc-1.2.3-py3-none-any.whl"}]}
    assert pypi_has_wheel(index, "1.2.3")
    assert not pypi_has_wheel(index, "1.2.4")
    print("release publication self-test: ok")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag")
    parser.add_argument("--repo", default=DEFAULT_REPO)
    parser.add_argument("--timeout-seconds", type=float, default=5400)
    parser.add_argument("--poll-seconds", type=float, default=15)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.tag is None:
        parser.error("--tag is required unless --self-test is used")
    try:
        verify(args.tag, args.repo, args.timeout_seconds, args.poll_seconds)
    except (RuntimeError, OSError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        print(f"release publication verification failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
