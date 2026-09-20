#!/usr/bin/env python3
"""Require a package generation bump for every changed editor target.

`--bump <target>` (`#jbversionbumpperbuild`) additionally *performs* the bump
when the target's sources differ from the state at its last package generation,
so a build cannot ship distinct bytes under a version string that already
shipped. Two builds of JetBrains 0.2.388 on 2026-09-20 did exactly that, and the
running IDE then mapped an unlinked jar whose version claimed to be current.

The bump predicate deliberately includes the UNSTAGED working tree. The fence
above only needs staged/committed evidence, but `gradlew buildPlugin` compiles
whatever is on disk, so an unstaged `.kt` edit is already in the artifact. That
is the exact gap the duplicate 0.2.388 pair came through.
"""

from __future__ import annotations

import argparse
import hashlib
import re
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


DIGEST_KEY = "pluginSourceDigest"


def source_digest(target: Target) -> str:
    """Digest of the exact source bytes a build would compile.

    Content, not git history, is the right basis. A commit-derived predicate
    ("sources changed since the version file was last committed") stays true for
    as long as the tree is dirty, so it bumps on EVERY invocation -- three bumps
    for zero distinct builds, measured while writing this. A digest bumps once
    per distinct artifact, which is the actual invariant.

    Tracked and untracked sources both count, because gradlew compiles whatever
    is on disk.
    """
    paths = set(git("ls-files", "--", *target.source_prefixes))
    paths |= set(git("ls-files", "--others", "--exclude-standard", "--", *target.source_prefixes))
    digest = hashlib.sha256()
    for path in sorted(p for p in paths if is_source(target, p)):
        digest.update(path.encode())
        try:
            with open(path, "rb") as handle:
                digest.update(handle.read())
        except FileNotFoundError:
            # Deleted-but-still-listed: its absence is itself part of the state.
            digest.update(b"<absent>")
    return digest.hexdigest()


def read_property(path: str, key: str) -> str | None:
    with open(path, encoding="utf-8") as handle:
        match = re.search(rf"(?m)^{re.escape(key)}\s*=\s*(\S+)\s*$", handle.read())
    return match.group(1) if match else None


def bump_gradle_patch(path: str) -> tuple[str, str]:
    """Increment the patch component of `pluginVersion` in a gradle.properties."""
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    match = re.search(r"(?m)^pluginVersion\s*=\s*(\d+)\.(\d+)\.(\d+)\s*$", text)
    if not match:
        raise SystemExit(f"{path}: no numeric `pluginVersion = X.Y.Z` line to bump")
    major, minor, patch = (int(part) for part in match.groups())
    old = f"{major}.{minor}.{patch}"
    new = f"{major}.{minor}.{patch + 1}"
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text[: match.start()] + f"pluginVersion = {new}" + text[match.end() :])
    return old, new


def write_property(path: str, key: str, value: str) -> None:
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    line = f"{key} = {value}"
    pattern = rf"(?m)^{re.escape(key)}\s*=.*$"
    text = re.sub(pattern, line, text) if re.search(pattern, text) else text.rstrip("\n") + f"\n{line}\n"
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)


def bump(target_name: str) -> int:
    matches = [t for t in TARGETS if t.name.lower() == target_name.lower()]
    if not matches:
        raise SystemExit(f"unknown target {target_name!r}")
    target = matches[0]
    current = source_digest(target)
    recorded = read_property(target.primary_version, DIGEST_KEY)
    if recorded == current:
        print(
            f"[bump-if-changed] {target.name}: sources byte-identical to the packaged "
            f"generation; holding version"
        )
        return 0
    old, new = bump_gradle_patch(target.primary_version)
    write_property(target.primary_version, DIGEST_KEY, current)
    reason = "no digest recorded" if recorded is None else "sources changed"
    print(f"[bump-if-changed] {target.name}: {reason}; pluginVersion {old} -> {new}")
    return 0


def self_test() -> int:
    """`#jbversionbumpperbuild` regressions, run from `make check`."""
    import tempfile

    jb = TARGETS[0]

    # bump_gradle_patch increments only the patch and leaves the file otherwise intact.
    with tempfile.TemporaryDirectory() as tmp:
        path = f"{tmp}/gradle.properties"
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("org.gradle.jvmargs = -Xmx2g\npluginVersion = 0.2.388\nother = keep\n")
        old, new = bump_gradle_patch(path)
        assert (old, new) == ("0.2.388", "0.2.389"), (old, new)
        body = open(path, encoding="utf-8").read()
        assert "pluginVersion = 0.2.389" in body, body
        assert "org.gradle.jvmargs = -Xmx2g" in body, "unrelated properties must survive a bump"
        assert "other = keep" in body, "unrelated properties must survive a bump"

        # write_property appends when absent, replaces in place when present.
        write_property(path, DIGEST_KEY, "aaa")
        assert read_property(path, DIGEST_KEY) == "aaa"
        write_property(path, DIGEST_KEY, "bbb")
        assert read_property(path, DIGEST_KEY) == "bbb"
        assert open(path, encoding="utf-8").read().count(DIGEST_KEY) == 1, (
            "replacing a digest must not append a second one"
        )

    # The defect this guards: a COMMIT-derived predicate stays true while the tree
    # is dirty, so it bumped on every invocation -- three bumps for zero distinct
    # builds. A content digest must be stable across repeated calls on unchanged
    # bytes, and must move when the bytes move.
    global git
    original_git = git
    try:
        with tempfile.TemporaryDirectory() as tmp:
            import os

            src = f"{tmp}/editors/jetbrains/src"
            os.makedirs(src)
            kt = f"{src}/A.kt"
            with open(kt, "w", encoding="utf-8") as handle:
                handle.write("class A")
            rel = os.path.relpath(kt, tmp)
            git = lambda *a: [rel] if "ls-files" in a and "--others" not in a else []  # noqa: E731
            cwd = os.getcwd()
            os.chdir(tmp)
            try:
                first = source_digest(jb)
                assert source_digest(jb) == first, (
                    "an unchanged tree must digest identically, or every build bumps"
                )
                with open(kt, "w", encoding="utf-8") as handle:
                    handle.write("class A { }")
                assert source_digest(jb) != first, "changed source bytes must change the digest"
            finally:
                os.chdir(cwd)
    finally:
        git = original_git

    print("[self-test] check_plugin_versions: ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the bump/digest regressions",
    )
    parser.add_argument(
        "--bump",
        metavar="TARGET",
        help="bump TARGET's package generation when its sources changed since the last one",
    )
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.bump:
        return bump(args.bump)
    return check()


def check() -> int:
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
