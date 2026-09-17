#!/usr/bin/env python3
"""Require a package generation bump for every changed editor target."""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass


@dataclass(frozen=True)
class Target:
    name: str
    source_prefixes: tuple[str, ...]
    source_suffixes: tuple[str, ...]
    primary_version: str
    version_files: tuple[str, ...]


TARGETS = (
    Target(
        "JetBrains",
        ("editors/jetbrains/src/",),
        (".kt",),
        "editors/jetbrains/gradle.properties",
        ("editors/jetbrains/gradle.properties",),
    ),
    Target(
        "VS Code",
        ("editors/vscode/src/",),
        (".ts",),
        "editors/vscode/package.json",
        (
            "editors/vscode/package.json",
            "editors/vscode/package-lock.json",
            "editors/vscode/src/native.ts",
        ),
    ),
    Target(
        "Zed",
        ("editors/zed/src/", "agent-doc-zed-lsp-io/src/"),
        (".rs",),
        "editors/zed/extension.toml",
        ("editors/zed/extension.toml",),
    ),
)


def git(*args: str) -> list[str]:
    result = subprocess.run(
        ("git", *args), check=True, text=True, capture_output=True
    )
    return [line for line in result.stdout.splitlines() if line]


def is_source(target: Target, path: str) -> bool:
    return path.startswith(target.source_prefixes) and path.endswith(target.source_suffixes)


def main() -> int:
    staged = set(git("diff", "--cached", "--name-only", "--diff-filter=ACMR"))
    failures: list[str] = []
    for target in TARGETS:
        staged_source = any(is_source(target, path) for path in staged)
        version_commit = git("log", "-1", "--format=%H", "--", target.primary_version)
        drifted_source = False
        if version_commit:
            changed_since_version = git(
                "diff", "--name-only", f"{version_commit[0]}..HEAD", "--"
            )
            drifted_source = any(is_source(target, path) for path in changed_since_version)
        missing_versions = [path for path in target.version_files if path not in staged]
        if (staged_source or drifted_source) and missing_versions:
            reason = "staged source changes" if staged_source else "source changes since the last package generation"
            failures.append(
                f"{target.name}: {reason} require staged generation files: "
                + ", ".join(missing_versions)
            )

    if failures:
        print("ERROR: editor package generation fence failed", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
