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

`#editorbumpgeneration`: for VS Code and Zed, `--bump` writes the COMPLETE
generation (`GENERATION_PINS`), and the fence rejects a generation whose files
disagree, naming the version-pin guard tests a bump must keep green
(`VERSION_PIN_GUARDS`). See editors/SPEC.md section 14.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from collections.abc import Callable
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


SEMVER = re.compile(r"\d+\.\d+\.\d+")


@dataclass(frozen=True)
class VersionPin:
    """One place a package generation's version string lives (`#editorbumpgeneration`).

    `read` and `write` operate on the file's text. `write` replaces only the
    version bytes, so a bump's diff is exactly the version lines.
    """

    path: str
    label: str
    read: Callable[[str], str | None]
    write: Callable[[str, str], str]


def _json_value(text: str, key_path: tuple[str, ...]) -> str | None:
    value: object = json.loads(text)
    for key in key_path:
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    return value if isinstance(value, str) else None


def _json_version_pin(path: str, key_path: tuple[str, ...], scope_marker: str | None) -> VersionPin:
    """A `"version"` string in npm-formatted (2-space) JSON at `key_path`.

    The replacement is textual, so the rest of the file is byte-identical, and is
    then verified structurally: the reparsed document must equal the original
    with exactly `key_path` changed, or the write refuses.
    """
    label = key_path[0] + "".join('[""]' if key == "" else f".{key}" for key in key_path[1:])
    indent = " " * (2 * len(key_path))
    pattern = re.compile(rf'(?m)^{indent}"{re.escape(key_path[-1])}": "([^"]*)"')

    def write(text: str, version: str) -> str:
        start = 0 if scope_marker is None else text.find(scope_marker)
        match = pattern.search(text, start) if start >= 0 else None
        if match is None:
            raise SystemExit(f"{path}: no {label} string to bump")
        rendered = text[: match.start(1)] + version + text[match.end(1) :]
        expected = json.loads(text)
        node = expected
        for key in key_path[:-1]:
            node = node[key]
        node[key_path[-1]] = version
        if json.loads(rendered) != expected:
            raise SystemExit(f"{path}: rewriting {label} would change more than {label}")
        return rendered

    return VersionPin(path, label, lambda text: _json_value(text, key_path), write)


def _single_match_pin(path: str, label: str, pattern: re.Pattern[str], region: Callable[[str], str]) -> VersionPin:
    """A version captured by exactly one match of `pattern` within `region(text)`."""

    def find(text: str) -> re.Match[str] | None:
        matches = list(pattern.finditer(region(text)))
        if len(matches) != 1:
            return None
        return matches[0]

    def read(text: str) -> str | None:
        match = find(text)
        return match.group(1) if match else None

    def write(text: str, version: str) -> str:
        match = find(text)
        if match is None:
            raise SystemExit(f"{path}: expected exactly one {label} to bump")
        return text[: match.start(1)] + version + text[match.end(1) :]

    return VersionPin(path, label, read, write)


def _toml_root_table(text: str) -> str:
    """The TOML text before the first `[table]` header: the manifest's own keys."""
    header = re.search(r"(?m)^\s*\[", text)
    return text if header is None else text[: header.start()]


VSCODE_PACKAGE = "editors/vscode/package.json"
VSCODE_LOCK = "editors/vscode/package-lock.json"
VSCODE_NATIVE = "editors/vscode/src/native.ts"
ZED_MANIFEST = "editors/zed/extension.toml"

# Every file of one package generation, per editor target. JetBrains targets
# carry theirs in gradle.properties (`pluginVersion` plus the source digest) and
# bump through the digest path instead.
GENERATION_PINS: dict[str, tuple[VersionPin, ...]] = {
    "VS Code": (
        _json_version_pin(VSCODE_PACKAGE, ("version",), None),
        _json_version_pin(VSCODE_LOCK, ("version",), None),
        _json_version_pin(VSCODE_LOCK, ("packages", "", "version"), '\n  "packages": {\n    "": {\n'),
        _single_match_pin(
            VSCODE_NATIVE,
            "EDITOR_PLUGIN_VERSION",
            re.compile(r"(?m)^export const EDITOR_PLUGIN_VERSION = '([^']*)';"),
            lambda text: text,
        ),
    ),
    "Zed": (
        _single_match_pin(
            ZED_MANIFEST,
            "version",
            re.compile(r'(?m)^version\s*=\s*"([^"]*)"\s*$'),
            _toml_root_table,
        ),
    ),
}

# The tests that pin each generation's version to something else. A bump must
# keep every one green, so the fence and the bump name them
# (`#editorbumpgeneration`). Each entry is (file, test function).
VERSION_PIN_GUARDS: dict[str, tuple[tuple[str, str], ...]] = {
    "VS Code": (
        # package.json `version` == native.ts EDITOR_PLUGIN_VERSION (the version
        # the extension reports over FFI, which stale-plugin detection reads).
        ("tests/full_content_static_guard.rs", "vscode_manifest_exposes_jetbrains_parity_commands"),
    ),
    "Zed": (
        # agent-doc-reliable-sync-io/build.rs bakes extension.toml `version` as
        # the Zed generation this build pairs with.
        (
            "agent-doc-controller-io/src/project_controller/rpc.rs",
            "sidecar_registration_reports_the_running_generation",
        ),
    ),
}


def render_guards(target_name: str) -> str:
    return ", ".join(f"{path}::{test}" for path, test in VERSION_PIN_GUARDS.get(target_name, ()))


def version_key(version: str) -> tuple[int, ...]:
    return tuple(int(part) for part in version.split("."))


def read_generation(target: Target, revision: str | None = None) -> list[tuple[VersionPin, str | None]]:
    """Each pin's version, from the working tree or (with `revision`) a commit."""
    texts: dict[str, str | None] = {}
    pins = []
    for pin in GENERATION_PINS[target.name]:
        if pin.path not in texts:
            if revision is None:
                try:
                    texts[pin.path] = open(pin.path, encoding="utf-8").read()
                except FileNotFoundError:
                    texts[pin.path] = None
            else:
                texts[pin.path] = git_text("show", f"{revision}:{pin.path}")
        text = texts[pin.path]
        pins.append((pin, None if text is None else pin.read(text)))
    return pins


def generation_version(pins: list[tuple[VersionPin, str | None]]) -> str | None:
    """The generation a set of pins claims: the highest numeric version among them.

    The highest, not the primary file's, because a version any pin already
    carried may already have shipped and must not be reused.
    """
    numeric = [version for _, version in pins if version is not None and SEMVER.fullmatch(version)]
    return max(numeric, key=version_key) if numeric else None


def generation_coherence_failure(target: Target) -> str | None:
    """Fail when the files of one generation disagree (the f14a185b30 class)."""
    pins = read_generation(target)
    rendered = "; ".join(f"{pin.path} {pin.label} {version or '<missing>'}" for pin, version in pins)
    versions = {version for _, version in pins}
    if len(versions) == 1 and None not in versions and SEMVER.fullmatch(next(iter(versions)) or ""):
        return None
    return (
        f"{target.name}: incomplete package generation ({rendered}); run "
        f"`python3 scripts/check_plugin_versions.py --bump {target.name!r}` to complete it. "
        f"Version-pin guards it must keep green: {render_guards(target.name)}"
    )


def write_generation(target: Target, version: str) -> list[str]:
    """Write `version` into every pin of the target's generation; return what changed.

    Every file is rewritten in memory first, so a missing pin refuses the whole
    bump instead of leaving a partial generation on disk.
    """
    changed: list[str] = []
    rewrites: dict[str, tuple[str, str]] = {}
    for path in dict.fromkeys(pin.path for pin in GENERATION_PINS[target.name]):
        with open(path, encoding="utf-8") as handle:
            original = handle.read()
        text = original
        for pin in GENERATION_PINS[target.name]:
            if pin.path == path and pin.read(text) != version:
                text = pin.write(text, version)
                changed.append(f"{path} ({pin.label})")
        rewrites[path] = (original, text)
    for path, (original, text) in rewrites.items():
        if text != original:
            with open(path, "w", encoding="utf-8") as handle:
                handle.write(text)
    return changed


def last_generation_commit(target: Target) -> str | None:
    commits = git("log", "-1", "--format=%H", "--", target.primary_version)
    return commits[0] if commits else None


def committed_source_drift(target: Target) -> bool:
    """Sources committed since the commit that last touched the primary version file."""
    version_commit = last_generation_commit(target)
    if version_commit is None:
        return False
    changed = git("diff", "--name-only", f"{version_commit}..HEAD", "--")
    return any(is_source(target, path) for path in changed)


def working_tree_source_changes(target: Target) -> bool:
    """Staged, unstaged, or untracked sources: the build packages what is on disk."""
    changed = set(git("diff", "HEAD", "--name-only", "--", *target.source_prefixes))
    changed |= set(git("ls-files", "--others", "--exclude-standard", "--", *target.source_prefixes))
    return any(is_source(target, path) for path in changed)


def bump_generation(target: Target) -> int:
    """`--bump` for a target without a source digest: write the COMPLETE generation.

    The predicate is the fence's own: a source changed since the commit that last
    touched the primary version file, or a source differs on disk. A generation
    already allocated in the working tree (disk differs from HEAD) holds, so a
    repeated bump is a no-op rather than one bump per invocation. Holding still
    completes an incomplete generation, so a hand bump of one file is repaired.
    """
    disk = read_generation(target)
    current = generation_version(disk)
    if current is None:
        raise SystemExit(
            f"{target.name}: no numeric X.Y.Z version in "
            + ", ".join(f"{pin.path} ({pin.label})" for pin, _ in disk)
        )
    head = generation_version(read_generation(target, "HEAD"))
    guards = render_guards(target.name)
    if head is not None and current != head:
        hold_reason = f"generation {current} already allocated in the working tree (HEAD has {head})"
    elif not (committed_source_drift(target) or working_tree_source_changes(target)):
        hold_reason = f"no source changes since generation {current}"
    else:
        hold_reason = None

    if hold_reason is not None:
        completed = write_generation(target, current)
        if completed:
            print(
                f"[bump-if-changed] {target.name}: {hold_reason}; completed the generation in "
                + ", ".join(completed)
                + f". Keep green: {guards}"
            )
        else:
            print(f"[bump-if-changed] {target.name}: {hold_reason}; holding version")
        return 0

    major, minor, patch = version_key(current)
    new = f"{major}.{minor}.{patch + 1}"
    changed = write_generation(target, new)
    print(
        f"[bump-if-changed] {target.name}: sources changed; {current} -> {new} in "
        + ", ".join(changed)
        + f". Keep green: {guards}"
    )
    return 0


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
    if target.name in GENERATION_PINS:
        return bump_generation(target)
    if not target.name.startswith("JetBrains"):
        raise SystemExit(f"{target.name}: no package generation is defined for --bump")
    current = source_digest(target)
    recorded = read_property(target.primary_version, DIGEST_KEY)
    if recorded == current:
        print(
            f"[bump-if-changed] {target.name}: sources byte-identical to the packaged "
            f"generation; holding version"
        )
        return 0
    old_version = read_gradle_version(target.primary_version)
    new_version = next_jetbrains_version()
    old = ".".join(map(str, old_version))
    new = ".".join(map(str, new_version))
    write_gradle_version(target.primary_version, new_version)
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
        'derivation.derive("client_measured_layout", GSON.toJson(evidence))',
        "frozenFrontendDerive(main, isSessionDocument)",
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

    publish_helper = open(
        os.path.join(repo_root, "scripts", "publish-pinned-lazily-kt.sh"),
        encoding="utf-8",
    ).read()
    assert 'ln -s "$spec_dir" "$scratch/lazily-spec"' in publish_helper, (
        "the exact-tag lazily-kt checkout must retain its required ../lazily-spec sibling"
    )

    # #lzexplicitcomposite: the version pinned in each JetBrains build is the
    # default dependency authority. A checkout path must never implicitly opt
    # classic builds into an adjacent, possibly dirty lazily-kt source tree.
    # Both plugin generations expose the same explicit source-verification seam.
    classic_settings = open(
        os.path.join(repo_root, "editors", "jetbrains", "settings.gradle.kts"),
        encoding="utf-8",
    ).read()
    modular_settings = open(
        os.path.join(repo_root, "editors", "jetbrains-262", "settings.gradle.kts"),
        encoding="utf-8",
    ).read()
    explicit_override = 'System.getenv("AGENT_DOC_LAZILY_KT_DIR")?.let { sourceDir ->'
    assert explicit_override in classic_settings, (
        "classic JetBrains source substitution must require AGENT_DOC_LAZILY_KT_DIR"
    )
    assert explicit_override in modular_settings, (
        "modular JetBrains source substitution must require AGENT_DOC_LAZILY_KT_DIR"
    )
    assert '?: file("../../../lazily-kt")' not in classic_settings, (
        "classic JetBrains builds must not silently substitute an adjacent lazily-kt checkout"
    )
    assert 'lazilyKt.parentFile.resolve("lazily-spec/proto")' in classic_settings, (
        "the explicit Lazily checkout must resolve its own adjacent spec checkout"
    )

    editor_generation_self_test(repo_root)

    print("[self-test] check_plugin_versions: ok")
    return 0


def editor_generation_self_test(repo_root: str) -> None:
    """`#editorbumpgeneration`: `--bump` writes every file of a VS Code / Zed generation.

    On 2026-11-05 `--bump 'VS Code'` and `--bump Zed` failed with "no numeric
    pluginVersion line", so batch k bumped package.json by hand without
    native.ts EDITOR_PLUGIN_VERSION and reddened
    vscode_manifest_exposes_jetbrains_parity_commands on main f14a185b30. This
    runs the bump against copies of the REAL generation files in a scratch git
    repository, then re-checks the shape each named version-pin guard reads.
    """
    import os
    import shutil
    import tempfile

    for target in TARGETS:
        assert target.name.startswith("JetBrains") or target.name in GENERATION_PINS, (
            f"{target.name}: every non-JetBrains target needs GENERATION_PINS or --bump cannot write it"
        )
        if target.name in GENERATION_PINS:
            pinned = {pin.path for pin in GENERATION_PINS[target.name]}
            assert pinned == set(target.version_files), (
                f"{target.name}: the fence's version_files and the generation --bump writes must agree",
                pinned,
                target.version_files,
            )
            assert GENERATION_PINS[target.name][0].path == target.primary_version
            assert VERSION_PIN_GUARDS.get(target.name), f"{target.name}: name its version-pin guards"

    # The named guards exist, and still read the shapes the emulation below checks.
    for target_name, guards in VERSION_PIN_GUARDS.items():
        for path, test in guards:
            source = open(os.path.join(repo_root, path), encoding="utf-8").read()
            assert re.search(rf"(?m)^\s*fn {re.escape(test)}\(", source), f"{target_name}: {path}::{test} is gone"
    vscode_guard = open(os.path.join(repo_root, "tests/full_content_static_guard.rs"), encoding="utf-8").read()
    assert "export const EDITOR_PLUGIN_VERSION = '" in vscode_guard
    assert '"\\"version\\": \\"{reported_version}\\""' in vscode_guard, (
        "the VS Code guard changed what it pins; update vscode_guard_green below"
    )
    bake = open(os.path.join(repo_root, "agent-doc-reliable-sync-io/build.rs"), encoding="utf-8").read()
    assert 'read_json_string(&vscode_pkg, "version")' in bake
    assert 'read_property(&zed_manifest, "version")' in bake

    def vscode_guard_green() -> bool:
        """tests/full_content_static_guard.rs::vscode_manifest_exposes_jetbrains_parity_commands."""
        native = open(VSCODE_NATIVE, encoding="utf-8").read()
        marker = "export const EDITOR_PLUGIN_VERSION = '"
        reported = native[native.index(marker) + len(marker) :].split("'", 1)[0]
        return f'"version": "{reported}"' in open(VSCODE_PACKAGE, encoding="utf-8").read()

    def baked_zed_version() -> str | None:
        """agent-doc-reliable-sync-io/build.rs `read_property(extension.toml, "version")`."""
        for line in open(ZED_MANIFEST, encoding="utf-8").read().splitlines():
            line = line.strip()
            if line.startswith("version"):
                rest = line[len("version") :].lstrip()
                if rest.startswith("="):
                    value = rest[1:].strip().strip('"')
                    if value:
                        return value
        return None

    def assert_only_version_lines(path: str, before: str, old: str, new: str, pins: int) -> None:
        """A bump changes exactly `pins` lines of `path`, each only in its version."""
        after = open(path, encoding="utf-8").read()
        old_lines, new_lines = before.splitlines(keepends=True), after.splitlines(keepends=True)
        assert len(old_lines) == len(new_lines), f"{path}: a bump must not add or remove lines"
        differing = [(a, b) for a, b in zip(old_lines, new_lines) if a != b]
        assert len(differing) == pins, (path, differing)
        for a, b in differing:
            assert b == a.replace(old, new, 1), f"{path}: a bump must change only the version bytes: {a!r} -> {b!r}"

    saved_git_env = {key: value for key, value in os.environ.items() if key.startswith("GIT_")}
    cwd = os.getcwd()
    with tempfile.TemporaryDirectory() as tmp:
        try:
            # A hook-run `make check` exports GIT_DIR/GIT_INDEX_FILE for the real
            # repository; the scratch repository must not inherit them.
            for key in saved_git_env:
                del os.environ[key]
            for path in (VSCODE_PACKAGE, VSCODE_LOCK, VSCODE_NATIVE, ZED_MANIFEST):
                os.makedirs(os.path.join(tmp, os.path.dirname(path)), exist_ok=True)
                shutil.copyfile(os.path.join(repo_root, path), os.path.join(tmp, path))
            os.makedirs(os.path.join(tmp, "editors/zed/src"), exist_ok=True)
            with open(os.path.join(tmp, "editors/zed/src/lib.rs"), "w", encoding="utf-8") as handle:
                handle.write("pub fn zed() {}\n")
            os.chdir(tmp)

            def run_git(*args: str) -> None:
                subprocess.run(
                    ("git", "-c", "user.name=self-test", "-c", "user.email=self-test@invalid",
                     "-c", "commit.gpgsign=false", *args),
                    check=True,
                    capture_output=True,
                )

            run_git("init", "-q")
            run_git("add", "-A")
            run_git("commit", "-q", "-m", "generation")

            vscode, zed = (next(t for t in TARGETS if t.name == name) for name in ("VS Code", "Zed"))
            originals = {path: open(path, encoding="utf-8").read() for path in (VSCODE_PACKAGE, VSCODE_LOCK, VSCODE_NATIVE, ZED_MANIFEST)}
            start = generation_version(read_generation(vscode))
            zed_start = generation_version(read_generation(zed))
            assert start and zed_start and generation_coherence_failure(vscode) is None
            assert vscode_guard_green(), "the real VS Code generation must start coherent"

            def bumped(version: str) -> str:
                major, minor, patch = version_key(version)
                return f"{major}.{minor}.{patch + 1}"

            # Unchanged sources hold every file byte-for-byte.
            bump("VS Code")
            bump("Zed")
            assert all(open(path, encoding="utf-8").read() == text for path, text in originals.items())

            # A VS Code source change bumps all four pins, and only those bytes.
            with open(VSCODE_NATIVE, "a", encoding="utf-8") as handle:
                handle.write("// changed\n")
            native_edited = open(VSCODE_NATIVE, encoding="utf-8").read()
            bump("VS Code")
            new = bumped(start)
            assert [version for _, version in read_generation(vscode)] == [new] * 4, read_generation(vscode)
            assert generation_coherence_failure(vscode) is None
            assert vscode_guard_green(), "a VS Code bump must keep the manifest/FFI version guard green"
            for path, before, pins in (
                (VSCODE_PACKAGE, originals[VSCODE_PACKAGE], 1),
                (VSCODE_LOCK, originals[VSCODE_LOCK], 2),
                (VSCODE_NATIVE, native_edited, 1),
            ):
                assert_only_version_lines(path, before, start, new, pins)
            assert open(ZED_MANIFEST, encoding="utf-8").read() == originals[ZED_MANIFEST], "a VS Code bump must not touch Zed"

            # A repeated bump on the same dirty tree holds: one bump per distinct state.
            bump("VS Code")
            assert generation_version(read_generation(vscode)) == new, "a second --bump must not bump again"

            # A Zed source change bumps extension.toml, which is the whole generation.
            with open("editors/zed/src/lib.rs", "a", encoding="utf-8") as handle:
                handle.write("pub fn changed() {}\n")
            bump("Zed")
            zed_new = bumped(zed_start)
            assert baked_zed_version() == zed_new, "build.rs must bake the bumped Zed generation"
            assert_only_version_lines(ZED_MANIFEST, originals[ZED_MANIFEST], zed_start, zed_new, 1)
            assert generation_coherence_failure(zed) is None
            bump("Zed")
            assert baked_zed_version() == zed_new, "a second --bump must not bump Zed again"

            # The f14a185b30 shape: a committed hand bump of package.json and the
            # lockfile only. The fence names the guard; --bump completes it.
            run_git("checkout", "-q", "--", ".")
            run_git("clean", "-qfd")
            for path in (VSCODE_PACKAGE, VSCODE_LOCK):
                text = open(path, encoding="utf-8").read()
                for pin in GENERATION_PINS["VS Code"]:
                    if pin.path == path:
                        text = pin.write(text, new)
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(text)
            run_git("commit", "-qam", "hand bump")
            assert not vscode_guard_green()
            failure = generation_coherence_failure(vscode)
            assert failure and "vscode_manifest_exposes_jetbrains_parity_commands" in failure, failure
            assert f"EDITOR_PLUGIN_VERSION {start}" in failure, failure
            bump("VS Code")
            assert generation_coherence_failure(vscode) is None
            assert generation_version(read_generation(vscode)) == new, (
                "completing a generation reuses its highest version, never an older one"
            )
            assert vscode_guard_green()

            # A file without its pin is refused, never silently skipped.
            with open(VSCODE_NATIVE, "w", encoding="utf-8") as handle:
                handle.write("export const OTHER = 1;\n")
            try:
                write_generation(vscode, bumped(new))
            except SystemExit as error:
                assert "EDITOR_PLUGIN_VERSION" in str(error), error
            else:
                raise AssertionError("a missing EDITOR_PLUGIN_VERSION must refuse the bump")
        finally:
            os.chdir(cwd)
            os.environ.update(saved_git_env)


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

        coherence = generation_coherence_failure(target)
        if coherence is not None:
            failures.append(coherence)
        drifted_source = committed_source_drift(target)
        if (staged_source or drifted_source) and missing_versions:
            reason = "staged source changes" if staged_source else "source changes since the last package generation"
            failures.append(
                f"{target.name}: {reason} require staged generation files: "
                + ", ".join(missing_versions)
                + f"; run `python3 scripts/check_plugin_versions.py --bump {target.name!r}`, which "
                "writes the complete generation. Version-pin guards it must keep green: "
                + render_guards(target.name)
            )

    if failures:
        print("ERROR: editor package generation fence failed", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
