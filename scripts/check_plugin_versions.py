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

GH #346 additionally rejects a JetBrains generation whose Kotlin delta is only
comments/formatting or a consistent identifier rename, and pins the schema-3
Remote Dev grouping path behind the hot-reloaded `client_measured_layout`
derivation before its frozen compatibility floor.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import subprocess
import sys
from dataclasses import dataclass


DIGEST_KEY = "pluginSourceDigest"
RELEASE_KEY = "pluginReleaseVersion"
MEASURED_LAYOUT_FORWARDER = (
    "editors/jetbrains-262/backend/src/main/kotlin/com/github/btakita/agentdoc/split/backend/"
    "RemoteClientLayoutForwarder.kt"
)

KOTLIN_KEYWORDS = {
    "as", "break", "class", "continue", "do", "else", "false", "for", "fun", "if",
    "in", "interface", "is", "null", "object", "package", "return", "super", "this",
    "throw", "true", "try", "typealias", "typeof", "val", "var", "when", "while",
}
KOTLIN_TOKEN = re.compile(
    r'''(?sx)
    (?P<space>\s+)
    |(?P<line_comment>//[^\n]*)
    |(?P<block_comment>/\*.*?\*/)
    |(?P<triple_string>""".*?""")
    |(?P<string>"(?:\\.|[^"\\])*")
    |(?P<char>'(?:\\.|[^'\\])*')
    |(?P<identifier>[A-Za-z_][A-Za-z0-9_]*)
    |(?P<number>\d+(?:\.\d+)?)
    |(?P<symbol>.)
    ''',
)


@dataclass(frozen=True)
class Target:
    name: str
    source_prefixes: tuple[str, ...]
    source_suffixes: tuple[str, ...]
    primary_version: str
    version_files: tuple[str, ...]
    source_digest_key: str | None = None
    excluded_source_fragments: tuple[str, ...] = ()


TARGETS = (
    Target(
        "JetBrains",
        ("editors/jetbrains/src/", "editors/jetbrains/build.gradle.kts"),
        (".kt", ".java", ".xml", ".kts"),
        "editors/jetbrains/gradle.properties",
        ("editors/jetbrains/gradle.properties",),
        DIGEST_KEY,
        ("/src/test/",),
    ),
    Target(
        "JetBrains 262",
        ("editors/jetbrains-262/", "editors/jetbrains/src/"),
        (".kt", ".java", ".xml", ".kts"),
        "editors/jetbrains-262/gradle.properties",
        ("editors/jetbrains-262/gradle.properties",),
        DIGEST_KEY,
        ("/src/test/",),
    ),
    Target(
        "JetBrains 263",
        ("editors/jetbrains-262/", "editors/jetbrains/src/"),
        (".kt", ".java", ".xml", ".kts"),
        "editors/jetbrains-263/gradle.properties",
        ("editors/jetbrains-263/gradle.properties",),
        DIGEST_KEY,
        ("/src/test/",),
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


def git_text(*args: str) -> str | None:
    """Read one git object without conflating an empty file with a missing object."""
    result = subprocess.run(("git", *args), text=True, capture_output=True)
    return result.stdout if result.returncode == 0 else None


def kotlin_tokens(source: str) -> list[tuple[str, str]]:
    """Tokenize enough Kotlin to distinguish comments and consistent identifier renames."""
    return [
        (match.lastgroup or "symbol", match.group())
        for match in KOTLIN_TOKEN.finditer(source)
        if match.lastgroup not in {"space", "line_comment", "block_comment"}
    ]


def nonbehavioral_kotlin_change(before: str, after: str) -> str | None:
    """Classify source-only churn that should not force an exact-263 IDE restart."""
    left = kotlin_tokens(before)
    right = kotlin_tokens(after)
    if left == right:
        return "comments/formatting only"
    if len(left) != len(right):
        return None

    forward: dict[str, str] = {}
    reverse: dict[str, str] = {}
    changed = False
    for (left_kind, left_token), (right_kind, right_token) in zip(left, right, strict=True):
        if (left_kind, left_token) == (right_kind, right_token):
            continue
        if (
            left_kind != "identifier"
            or right_kind != "identifier"
            or left_token in KOTLIN_KEYWORDS
            or right_token in KOTLIN_KEYWORDS
        ):
            return None
        if forward.setdefault(left_token, right_token) != right_token:
            return None
        if reverse.setdefault(right_token, left_token) != left_token:
            return None
        changed = True
    return "identifier rename only" if changed else None


def client_measured_layout_boundary_failure(source: str) -> str | None:
    """Keep schema-3 grouping behind the hot-reloaded native derivation (GH #346)."""
    start = source.find("fun derive(")
    end = source.find("/** Parse the native `client_measured_layout` output", start)
    if start < 0 or end < 0:
        return "JetBrains 262/263: client_measured_layout derive boundary is missing"
    body = source[start:end]
    native = body.find('derivation.derive("client_measured_layout"')
    floor = body.find("frozenFrontendDerive(main, isSessionDocument)")
    if native < 0:
        return "JetBrains 262/263: schema-3 grouping bypasses client_measured_layout"
    if floor < 0 or floor < native:
        return (
            "JetBrains 262/263: the schema-3 frontend vector must remain a post-native "
            "compatibility floor"
        )
    schema_three_branch = body.find(
        "if (snapshot.schemaVersion >= FRONTEND_LAYOUT_SURFACE_SNAPSHOT_SCHEMA_VERSION)"
    )
    if 0 <= schema_three_branch < native:
        return (
            "JetBrains 262/263: schema 3 returns before client_measured_layout; move grouping "
            "behind the hot-reloaded native boundary"
        )
    return None


def previous_release_ref(current_version: str) -> str | None:
    """Return the newest release tag other than the version currently being checked."""
    current_tag = f"v{current_version}"
    return next(
        (tag for tag in git("tag", "--list", "v*", "--sort=-version:refname") if tag != current_tag),
        None,
    )


def nonbehavioral_generation_failure(target: Target, current_version: str) -> str | None:
    """Reject a plugin generation allocated solely for Kotlin comments or renames."""
    previous = previous_release_ref(current_version)
    if previous is None:
        return None
    old_properties = git_text("show", f"{previous}:{target.primary_version}")
    if old_properties is None:
        return None
    match = re.search(r"(?m)^pluginVersion\s*=\s*(\d+\.\d+\.\d+)\s*$", old_properties)
    if match is None or match.group(1) == ".".join(map(str, read_gradle_version(target.primary_version))):
        return None

    changed = set(git("diff", "--name-only", previous, "--", *target.source_prefixes))
    changed |= set(git("ls-files", "--others", "--exclude-standard", "--", *target.source_prefixes))
    runtime = sorted(path for path in changed if is_source(target, path))
    if not runtime:
        return f"{target.name}: plugin generation changed without packaged runtime source changes"

    classifications: list[tuple[str, str]] = []
    for path in runtime:
        if not path.endswith(".kt"):
            return None
        before = git_text("show", f"{previous}:{path}")
        try:
            after = open(path, encoding="utf-8").read()
        except FileNotFoundError:
            return None
        if before is None:
            return None
        classification = nonbehavioral_kotlin_change(before, after)
        if classification is None:
            return None
        classifications.append((path, classification))

    details = ", ".join(f"{path} ({kind})" for path, kind in classifications)
    return (
        f"{target.name}: plugin generation changes only comments/formatting or identifier names: "
        f"{details}; keep the existing generation or pair the edit with required runtime behavior"
    )


def is_source(target: Target, path: str) -> bool:
    return (
        path.startswith(target.source_prefixes)
        and path.endswith(target.source_suffixes)
        and not any(fragment in path for fragment in target.excluded_source_fragments)
    )


def source_digest(target: Target) -> str:
    """Digest of the source and build metadata that shape the packaged plugin.

    Content, not git history, is the right basis. A commit-derived predicate
    ("sources changed since the version file was last committed") stays true for
    as long as the tree is dirty, so it bumps on EVERY invocation -- three bumps
    for zero distinct builds, measured while writing this. A digest bumps once
    per distinct artifact, which is the actual invariant.

    Tracked and untracked runtime sources plus artifact-shaping build metadata
    count because gradlew packages whatever is on disk. Test source sets do not:
    Gradle compiles them for verification but does not put them in the plugin.
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


def read_gradle_version(path: str) -> tuple[int, int, int]:
    """Read a numeric `pluginVersion` from a gradle.properties file."""
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    match = re.search(r"(?m)^pluginVersion\s*=\s*(\d+)\.(\d+)\.(\d+)\s*$", text)
    if not match:
        raise SystemExit(f"{path}: no numeric `pluginVersion = X.Y.Z` line")
    return tuple(int(part) for part in match.groups())


def write_gradle_version(path: str, version: tuple[int, int, int]) -> None:
    """Replace exactly one numeric `pluginVersion` property."""
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    match = re.search(r"(?m)^pluginVersion\s*=\s*(\d+)\.(\d+)\.(\d+)\s*$", text)
    if not match:
        raise SystemExit(f"{path}: no numeric `pluginVersion = X.Y.Z` line")
    rendered = ".".join(str(part) for part in version)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text[: match.start()] + f"pluginVersion = {rendered}" + text[match.end() :])


def bump_gradle_patch(path: str) -> tuple[str, str]:
    """Increment the patch component of `pluginVersion` in a gradle.properties."""
    version = read_gradle_version(path)
    old = ".".join(str(part) for part in version)
    bumped = (version[0], version[1], version[2] + 1)
    new = ".".join(str(part) for part in bumped)
    write_gradle_version(path, bumped)
    return old, new


def write_property(path: str, key: str, value: str) -> None:
    with open(path, encoding="utf-8") as handle:
        text = handle.read()
    line = f"{key} = {value}"
    pattern = rf"(?m)^{re.escape(key)}\s*=.*$"
    text = re.sub(pattern, line, text) if re.search(pattern, text) else text.rstrip("\n") + f"\n{line}\n"
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)


def jetbrains_targets() -> tuple[Target, ...]:
    return tuple(target for target in TARGETS if target.name.startswith("JetBrains"))


def next_jetbrains_version() -> tuple[int, int, int]:
    """Allocate above both Marketplace updates so the shared plugin ID cannot collide."""
    highest = max(read_gradle_version(target.primary_version) for target in jetbrains_targets())
    return highest[0], highest[1], highest[2] + 1


def project_jetbrains_release(release_version: str) -> int:
    """Project one app release without replacing source-identical plugins."""
    if re.fullmatch(r"\d+\.\d+\.\d+", release_version) is None:
        raise SystemExit(f"invalid semantic release version {release_version!r}")

    targets = jetbrains_targets()
    current_digests = {target.name: source_digest(target) for target in targets}
    already_projected = all(
        read_property(target.primary_version, RELEASE_KEY) == release_version
        and read_property(target.primary_version, DIGEST_KEY) == current_digests[target.name]
        for target in targets
    )
    versions = [read_gradle_version(target.primary_version) for target in targets]
    if already_projected and len(set(versions)) == len(versions):
        print(
            f"[release-version] JetBrains: {release_version} already owns distinct "
            + ", ".join(".".join(map(str, version)) for version in versions)
        )
        return 0

    next_version = next_jetbrains_version()
    allocated = 0
    for target in targets:
        old_version = read_gradle_version(target.primary_version)
        old = ".".join(map(str, old_version))
        recorded_digest = read_property(target.primary_version, DIGEST_KEY)
        if recorded_digest == current_digests[target.name]:
            assigned = old_version
        else:
            assigned = (next_version[0], next_version[1], next_version[2] + allocated)
            allocated += 1
            write_gradle_version(target.primary_version, assigned)
        write_property(target.primary_version, DIGEST_KEY, current_digests[target.name])
        write_property(target.primary_version, RELEASE_KEY, release_version)
        new = ".".join(map(str, assigned))
        if assigned == old_version:
            print(
                f"[release-version] {target.name}: sources byte-identical to pluginVersion "
                f"{old}; retaining the live-compatible generation for release {release_version}"
            )
        else:
            print(f"[release-version] {target.name}: pluginVersion {old} -> {new}")
    return 0


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
    if target.name.startswith("JetBrains"):
        old_version = read_gradle_version(target.primary_version)
        new_version = next_jetbrains_version()
        old = ".".join(map(str, old_version))
        new = ".".join(map(str, new_version))
        write_gradle_version(target.primary_version, new_version)
    else:
        old, new = bump_gradle_patch(target.primary_version)
    write_property(target.primary_version, DIGEST_KEY, current)
    reason = "no digest recorded" if recorded is None else "sources changed"
    print(f"[bump-if-changed] {target.name}: {reason}; pluginVersion {old} -> {new}")
    return 0


def self_test() -> int:
    """`#jbversionbumpperbuild` regressions, run from `make check`."""
    import tempfile

    assert nonbehavioral_kotlin_change(
        "/** old wording */\nfun answer(nativeFold: String) = nativeFold\n",
        "/** clearer wording */\nfun answer(nativeFold: String) = nativeFold\n",
    ) == "comments/formatting only"
    assert nonbehavioral_kotlin_change(
        "fun answer(nativeFold: String, legacyFold: String) = nativeFold + legacyFold\n",
        "fun answer(nativeResolver: String, legacyResolver: String) = nativeResolver + legacyResolver\n",
    ) == "identifier rename only"
    assert nonbehavioral_kotlin_change(
        "fun answer(value: Int) = value + 1\n",
        "fun answer(value: Int) = value + 2\n",
    ) is None
    boundary_source = open(MEASURED_LAYOUT_FORWARDER, encoding="utf-8").read()
    assert client_measured_layout_boundary_failure(boundary_source) is None
    bypass = boundary_source.replace(
        'return derivation.derive("client_measured_layout", GSON.toJson(evidence))',
        "return frozenFrontendDerive(main, isSessionDocument)",
        1,
    )
    assert "schema-3 grouping bypasses" in client_measured_layout_boundary_failure(bypass)

    jb = TARGETS[0]
    jb262 = TARGETS[1]
    jb263 = TARGETS[2]
    assert "editors/jetbrains-262/" in jb262.source_prefixes
    assert "editors/jetbrains/src/" in jb262.source_prefixes, (
        "the modular backend compiles classic implementation sources, so they must fence both generations"
    )
    assert "editors/jetbrains-262/" in jb263.source_prefixes
    assert "editors/jetbrains/src/" in jb263.source_prefixes

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
            classic_test = f"{src}/test/kotlin/AContractTest.kt"
            build = f"{tmp}/editors/jetbrains/build.gradle.kts"
            modular_dir = f"{tmp}/editors/jetbrains-262"
            os.makedirs(modular_dir)
            modern_dir = f"{tmp}/editors/jetbrains-263"
            os.makedirs(modern_dir)
            modular_build = f"{modular_dir}/build.gradle.kts"
            modular_test = f"{modular_dir}/backend/src/test/kotlin/BackendContractTest.kt"
            os.makedirs(os.path.dirname(classic_test), exist_ok=True)
            os.makedirs(os.path.dirname(modular_test), exist_ok=True)
            with open(kt, "w", encoding="utf-8") as handle:
                handle.write("class A")
            with open(java, "w", encoding="utf-8") as handle:
                handle.write("class UpgradeAgent {}")
            with open(classic_test, "w", encoding="utf-8") as handle:
                handle.write("class AContractTest")
            with open(build, "w", encoding="utf-8") as handle:
                handle.write("plugins { java }")
            with open(modular_build, "w", encoding="utf-8") as handle:
                handle.write("plugins { splitMode }")
            with open(modular_test, "w", encoding="utf-8") as handle:
                handle.write("class BackendContractTest")
            with open(f"{modular_dir}/gradle.properties", "w", encoding="utf-8") as handle:
                handle.write("pluginVersion = 0.2.388\n")
            modern_props = f"{modern_dir}/gradle.properties"
            with open(modern_props, "w", encoding="utf-8") as handle:
                handle.write("pluginVersion = 0.2.388\ncompatibilitySinceBuild = 263\n")
            rels = [
                os.path.relpath(path, tmp)
                for path in (
                    kt,
                    java,
                    classic_test,
                    build,
                    modular_build,
                    modular_test,
                    modern_props,
                )
            ]
            git = lambda *a: rels if "ls-files" in a and "--others" not in a else []  # noqa: E731
            cwd = os.getcwd()
            os.chdir(tmp)
            try:
                first = source_digest(jb)
                assert source_digest(jb) == first, (
                    "an unchanged tree must digest identically, or every build bumps"
                )
                modular_first = source_digest(jb262)
                modern_first = source_digest(jb263)
                with open(classic_test, "w", encoding="utf-8") as handle:
                    handle.write("class AContractTest { val changed = true }")
                with open(modular_test, "w", encoding="utf-8") as handle:
                    handle.write("class BackendContractTest { val changed = true }")
                assert source_digest(jb) == first, (
                    "test-only classic changes must not manufacture a package generation"
                )
                assert source_digest(jb262) == modular_first, (
                    "test-only modular changes must not manufacture a 262 package generation"
                )
                assert source_digest(jb263) == modern_first, (
                    "test-only modular changes must not manufacture a 263 package generation"
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

                # A release with changed sources owns fresh Marketplace updates.
                # Repeating the projection is byte-stable, and a new app release
                # with the same source digests keeps those live-compatible plugin
                # generations instead of manufacturing a restart-only update.
                project_jetbrains_release("2.4.6")
                assert read_gradle_version(jb.primary_version) == (0, 2, 389)
                assert read_gradle_version(jb262.primary_version) == (0, 2, 390)
                assert read_gradle_version(jb263.primary_version) == (0, 2, 391)
                assert read_property(jb.primary_version, RELEASE_KEY) == "2.4.6"
                assert source_digest_failure(jb) is None
                assert source_digest_failure(jb262) is None
                assert source_digest_failure(jb263) is None
                project_jetbrains_release("2.4.6")
                assert read_gradle_version(jb.primary_version) == (0, 2, 389)
                assert read_gradle_version(jb262.primary_version) == (0, 2, 390)
                assert read_gradle_version(jb263.primary_version) == (0, 2, 391)
                project_jetbrains_release("2.4.7")
                assert read_gradle_version(jb.primary_version) == (0, 2, 389)
                assert read_gradle_version(jb262.primary_version) == (0, 2, 390)
                assert read_gradle_version(jb263.primary_version) == (0, 2, 391)
                assert read_property(jb.primary_version, RELEASE_KEY) == "2.4.7"
                assert read_property(jb262.primary_version, RELEASE_KEY) == "2.4.7"
                assert read_property(jb263.primary_version, RELEASE_KEY) == "2.4.7"
                with open(kt, "w", encoding="utf-8") as handle:
                    handle.write("class A { val release = 2 }")
                project_jetbrains_release("2.4.7")
                assert read_gradle_version(jb.primary_version) == (0, 2, 392)
                assert read_gradle_version(jb262.primary_version) == (0, 2, 393)
                assert read_gradle_version(jb263.primary_version) == (0, 2, 394)
                with open(kt, "w", encoding="utf-8") as handle:
                    handle.write("class A { val release = 3 }")
                bump("JetBrains")
                assert read_gradle_version(jb.primary_version) == (0, 2, 395)
                assert read_gradle_version(jb262.primary_version) == (0, 2, 393)
                assert read_gradle_version(jb263.primary_version) == (0, 2, 394)
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
    assert 'check_plugin_versions.py --release-version "$(VERSION)"' in recipe("release-version")
    check_header = recipe("check").splitlines()[1]
    assert "jetbrains-classic-check" in check_header, check_header
    assert "jetbrains-262-check" in check_header, check_header
    assert "jetbrains-263-check" in check_header, check_header
    modern_recipe = recipe("jetbrains-263-check")
    assert "cd editors/jetbrains-262" in modern_recipe, (
        "JetBrains 263 must build the modular frame-aware source line"
    )
    assert "verifySplitArtifact" in modern_recipe, modern_recipe
    assert "verifyClassicArtifact" not in modern_recipe, modern_recipe
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
    assert "agent-doc-jetbrains-263-$modern_version.zip" in release_workflow, (
        "tag-triggered releases must package the JetBrains 263 distribution by exact name"
    )
    assert "verifySplitModeSandboxes" in release_workflow, (
        "the modular release artifact must prove the same ZIP is installed into both sandboxes"
    )
    assert "verifyClassicArtifact" in release_workflow, (
        "the classic release artifact must prove its compatibility range from the packaged ZIP"
    )
    assert 'sort -u | wc -l)" -eq 3' in release_workflow, (
        "the release job must reject colliding Marketplace update versions"
    )

    # #ci25gradle: the classic build's Gradle 8.14 wrapper cannot compile its
    # build script on JDK 25 (class file major version 69), which CI installs
    # as the default for the modular Gradle 9 build. The classic daemon must
    # stay pinned to JDK 21, and every workflow that builds it must install 21.
    daemon_jvm_path = os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "..",
        "editors",
        "jetbrains",
        "gradle",
        "gradle-daemon-jvm.properties",
    )
    daemon_jvm = open(daemon_jvm_path, encoding="utf-8").read().splitlines()
    assert "toolchainVersion=21" in daemon_jvm, (
        "the classic JetBrains Gradle daemon must be pinned to JDK 21"
    )
    for workflow_name in ("ci.yml", "release.yml"):
        workflow_path = os.path.join(
            os.path.dirname(os.path.abspath(__file__)), "..", ".github", "workflows", workflow_name
        )
        workflow = open(workflow_path, encoding="utf-8").read()
        assert re.search(r"java-version: \|\n\s+21\n\s+25\n", workflow), (
            f"{workflow_name} must install JDK 21 for the classic plugin beside the default JDK 25"
        )

    # #relcimavenlocal: both workflows that build editors/jetbrains-262 must
    # publish the pinned lazily-kt release to mavenLocal BEFORE the 262 build,
    # through the one shared script, or the tag's plugins job cannot resolve
    # io.github.lazily:lazily (v0.35.481 failed to publish exactly this way).
    lazily_step = "bash scripts/publish-pinned-lazily-kt.sh"
    for workflow_name, build_marker in (
        ("ci.yml", "run: make check"),
        ("release.yml", "Build JetBrains compatibility-ranged plugin zips"),
    ):
        workflow_path = os.path.join(
            os.path.dirname(os.path.abspath(__file__)), "..", ".github", "workflows", workflow_name
        )
        workflow = open(workflow_path, encoding="utf-8").read()
        assert lazily_step in workflow, (
            f"{workflow_name} must publish the pinned lazily-kt release via {lazily_step}"
        )
        assert build_marker in workflow, (workflow_name, build_marker)
        assert workflow.index(lazily_step) < workflow.index(build_marker), (
            f"{workflow_name} must publish lazily-kt to mavenLocal before the JetBrains 262 build"
        )

    # GH #258 embeds Lazily from a dedicated Gradle configuration, while the
    # release helper still has to discover the shared pin. Exercise the helper
    # itself so a future dependency-configuration rename cannot silently make
    # tag-triggered releases fail after all local plugin builds pass.
    repo_root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    pin_probe = subprocess.run(
        [
            "bash",
            "-c",
            "source scripts/publish-pinned-lazily-kt.sh; "
            "pin editors/jetbrains-262/backend/build.gradle.kts; "
            "pin editors/jetbrains/build.gradle.kts",
        ],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
    )
    pins = pin_probe.stdout.splitlines()
    assert len(pins) == 2 and pins[0] == pins[1], pins
    assert re.fullmatch(r"\d+\.\d+\.\d+", pins[0]), pins[0]

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
    parser.add_argument(
        "--release-version",
        metavar="VERSION",
        help="project VERSION onto source-identity-fenced JetBrains update versions",
    )
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.bump:
        return bump(args.bump)
    if args.release_version:
        return project_jetbrains_release(args.release_version)
    return check()


def check() -> int:
    staged = set(git("diff", "--cached", "--name-only", "--diff-filter=ACMR"))
    failures: list[str] = []
    jetbrains_versions = {
        target.name: read_gradle_version(target.primary_version)
        for target in jetbrains_targets()
    }
    if len(set(jetbrains_versions.values())) != len(jetbrains_versions):
        rendered = ", ".join(
            f"{name}={'.'.join(map(str, version))}"
            for name, version in jetbrains_versions.items()
        )
        failures.append(
            "JetBrains: compatibility-ranged artifacts share one Marketplace update version: "
            + rendered
        )

    cargo_text = open("Cargo.toml", encoding="utf-8").read()
    root_match = re.search(r'(?m)^version\s*=\s*"(\d+\.\d+\.\d+)"\s*$', cargo_text)
    if root_match is None:
        failures.append("JetBrains: Cargo.toml does not expose the root release version")
    else:
        expected_release = root_match.group(1)
        for target in jetbrains_targets():
            actual_release = read_property(target.primary_version, RELEASE_KEY)
            if actual_release != expected_release:
                failures.append(
                    f"{target.name}: {RELEASE_KEY} {actual_release!r}, expected {expected_release}; "
                    "run `make release-version VERSION=" + expected_release + "`"
                )
            nonbehavioral = nonbehavioral_generation_failure(target, expected_release)
            if nonbehavioral is not None:
                failures.append(nonbehavioral)

    measured_layout_source = open(MEASURED_LAYOUT_FORWARDER, encoding="utf-8").read()
    boundary_failure = client_measured_layout_boundary_failure(measured_layout_source)
    if boundary_failure is not None:
        failures.append(boundary_failure)

    modular_build = open("editors/jetbrains-262/build.gradle.kts", encoding="utf-8").read()
    modular_properties = open("editors/jetbrains-262/gradle.properties", encoding="utf-8").read()
    classic_properties = open("editors/jetbrains/gradle.properties", encoding="utf-8").read()
    if (
        "compatibilitySinceBuild = 242" not in classic_properties
        or "compatibilityUntilBuild = 261.*" not in classic_properties
    ):
        failures.append("JetBrains: classic artifact must be compatibility-ranged to 242..261.*")
    if (
        "compatibilitySinceBuild = 262" not in modular_properties
        or "compatibilityUntilBuild = 262.*" not in modular_properties
        or 'sinceBuild.set(compatibilitySinceBuild)' not in modular_build
        or 'untilBuild.set(compatibilityUntilBuild)' not in modular_build
    ):
        failures.append("JetBrains 262: modular artifact must be compatibility-ranged to 262..262.*")
    modern_properties = open("editors/jetbrains-263/gradle.properties", encoding="utf-8").read()
    if (
        "compatibilitySinceBuild = 263" not in modern_properties
        or "compatibilityUntilBuild = 263.*" not in modern_properties
    ):
        failures.append("JetBrains 263: artifact must be compatibility-ranged to 263..263.*")
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
