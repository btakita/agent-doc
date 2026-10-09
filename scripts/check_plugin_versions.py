#!/usr/bin/env python3
"""Require a package generation bump for every changed editor target.

`--bump <target>` (`#jbversionbumpperbuild`) additionally *performs* the bump
when the target's sources differ from the state at its last package generation,
so a build cannot ship distinct bytes under a version string that already
shipped. Two builds of JetBrains 0.2.388 on 2026-09-20 did exactly that, and the
running IDE then mapped an unlinked jar whose version claimed to be current.

The bump predicate and every digest-aware check deliberately include the
UNSTAGED working tree. `gradlew buildPlugin` compiles whatever is on disk, so an
unstaged `.kt` edit is already in the artifact. That is the exact gap the
duplicate 0.2.388 pair came through.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
import sys
from dataclasses import dataclass


DIGEST_KEY = "pluginSourceDigest"


@dataclass(frozen=True)
class Target:
    name: str
    source_prefixes: tuple[str, ...]
    source_suffixes: tuple[str, ...]
    primary_version: str
    version_files: tuple[str, ...]
    source_digest_key: str | None = None


TARGETS = (
    Target(
        "JetBrains",
        ("editors/jetbrains/src/", "editors/jetbrains/build.gradle.kts"),
        (".kt", ".java", ".xml", ".kts"),
        "editors/jetbrains/gradle.properties",
        ("editors/jetbrains/gradle.properties",),
        DIGEST_KEY,
    ),
    Target(
        "JetBrains 262",
        ("editors/jetbrains-262/", "editors/jetbrains/src/"),
        (".kt", ".java", ".xml", ".kts"),
        "editors/jetbrains-262/gradle.properties",
        ("editors/jetbrains-262/gradle.properties",),
        DIGEST_KEY,
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


def source_digest(target: Target) -> str:
    """Digest of the exact source bytes a build would compile.

    Content, not git history, is the right basis. A commit-derived predicate
    ("sources changed since the version file was last committed") stays true for
    as long as the tree is dirty, so it bumps on EVERY invocation -- three bumps
    for zero distinct builds, measured while writing this. A digest bumps once
    per distinct artifact, which is the actual invariant.

    Tracked and untracked sources plus artifact-shaping build metadata count,
    because gradlew packages whatever is on disk.
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


def source_digest_failure(target: Target) -> str | None:
    """Return the content-identity fence failure for a digest-aware target."""
    if target.source_digest_key is None:
        return None

    recorded = read_property(target.primary_version, target.source_digest_key)
    if recorded is None:
        return (
            f"{target.name}: {target.primary_version} does not record "
            f"{target.source_digest_key}; run the package generation bump"
        )

    current = source_digest(target)
    if recorded == current:
        return None
    return (
        f"{target.name}: current source digest {current} does not match the recorded "
        f"{target.source_digest_key} {recorded}; run the package generation bump"
    )


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
    jb262 = TARGETS[1]
    assert "editors/jetbrains-262/" in jb262.source_prefixes
    assert "editors/jetbrains/src/" in jb262.source_prefixes, (
        "the modular backend compiles classic implementation sources, so they must fence both generations"
    )

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
            java = f"{src}/UpgradeAgent.java"
            build = f"{tmp}/editors/jetbrains/build.gradle.kts"
            with open(kt, "w", encoding="utf-8") as handle:
                handle.write("class A")
            with open(java, "w", encoding="utf-8") as handle:
                handle.write("class UpgradeAgent {}")
            with open(build, "w", encoding="utf-8") as handle:
                handle.write("plugins { java }")
            rels = [os.path.relpath(path, tmp) for path in (kt, java, build)]
            git = lambda *a: rels if "ls-files" in a and "--others" not in a else []  # noqa: E731
            cwd = os.getcwd()
            os.chdir(tmp)
            try:
                first = source_digest(jb)
                assert source_digest(jb) == first, (
                    "an unchanged tree must digest identically, or every build bumps"
                )
                with open(jb.primary_version, "w", encoding="utf-8") as handle:
                    handle.write("pluginVersion = 0.2.388\n")
                missing = source_digest_failure(jb)
                assert missing and "does not record" in missing, missing
                write_property(jb.primary_version, DIGEST_KEY, first)
                assert source_digest_failure(jb) is None, (
                    "recorded digest must admit the byte-identical package generation"
                )
                with open(kt, "w", encoding="utf-8") as handle:
                    handle.write("class A { }")
                assert source_digest(jb) != first, "changed source bytes must change the digest"
                mismatch = source_digest_failure(jb)
                assert mismatch and "does not match" in mismatch, (
                    "an unstaged byte change must fail without consulting git history"
                )
                second = source_digest(jb)
                with open(java, "w", encoding="utf-8") as handle:
                    handle.write("class UpgradeAgent { static void agentmain() {} }")
                assert source_digest(jb) != second, "Java agent bytes must change the digest"
                third = source_digest(jb)
                with open(build, "w", encoding="utf-8") as handle:
                    handle.write("tasks.jar { manifest { } }")
                assert source_digest(jb) != third, "artifact build metadata must change the digest"
            finally:
                os.chdir(cwd)
    finally:
        git = original_git

    # `#installgenskew`: the binary embeds the JetBrains generation it expects
    # (agent-doc-reliable-sync-io/build.rs), so both install recipes must run
    # the generation bump BEFORE building it, and bump-plugin must record the
    # source digest (a bare `sed` bump let the next install bump again after the
    # binary built: plugin 0.2.443 live beside a binary expecting 0.2.442).
    import os

    makefile_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "Makefile")
    makefile = open(makefile_path, encoding="utf-8").read()

    def recipe(name: str) -> str:
        start = makefile.index(f"\n{name}:")
        end = makefile.find("\n\n", start + 1)
        return makefile[start:end]

    for name in ("install", "install-full"):
        header = recipe(name).splitlines()[1]
        assert "editor-generation-bump" in header, f"{name} must depend on editor-generation-bump: {header!r}"
    bump_plugin = recipe("bump-plugin")
    assert "check_plugin_versions.py --bump JetBrains" in bump_plugin, bump_plugin
    assert "sed -i" not in bump_plugin, "bump-plugin must not bump pluginVersion without recording its digest"
    bump_plugin_262 = recipe("bump-plugin-262")
    assert 'check_plugin_versions.py --bump "JetBrains 262"' in bump_plugin_262, bump_plugin_262
    check_header = recipe("check").splitlines()[1]
    assert "jetbrains-262-check" in check_header, check_header
    assert "--bump JetBrains" in recipe("editor-generation-bump")

    release_workflow_path = os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", ".github", "workflows", "release.yml"
    )
    release_workflow = open(release_workflow_path, encoding="utf-8").read()
    assert "python3 scripts/check_plugin_versions.py" in release_workflow, (
        "tag-triggered releases must fail closed on editor source-generation drift"
    )
    assert "agent-doc-jetbrains-262-$modular_version.zip" in release_workflow, (
        "tag-triggered releases must package the modular JetBrains distribution by exact name"
    )
    assert "verifySplitModeSandboxes" in release_workflow, (
        "the modular release artifact must prove the same ZIP is installed into both sandboxes"
    )

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
        digest_failure = source_digest_failure(target)
        if digest_failure is not None:
            failures.append(digest_failure)
            continue

        missing_versions = [path for path in target.version_files if path not in staged]
        if target.source_digest_key is not None:
            if staged_source and missing_versions:
                failures.append(
                    f"{target.name}: staged source changes require staged generation files: "
                    + ", ".join(missing_versions)
                )
            continue

        version_commit = git("log", "-1", "--format=%H", "--", target.primary_version)
        drifted_source = False
        if version_commit:
            changed_since_version = git(
                "diff", "--name-only", f"{version_commit[0]}..HEAD", "--"
            )
            drifted_source = any(is_source(target, path) for path in changed_since_version)
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
