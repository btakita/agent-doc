#!/usr/bin/env python3
"""Verify both copies of a pinned JetBrains Remote Dev preview artifact."""

from __future__ import annotations

import argparse
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from typing import Any, Callable, Dict, Iterable
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
import zipfile


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "preview-artifacts/jetbrains-262-preview-34842a2.json"
GITHUB_API = "https://api.github.com"
PINNED_SOURCE_COMMIT = "34842a23a57739c9ad97e0de7c80b036770a7dc9"
PINNED_RELEASE_ID = 408534966
PINNED_RELEASE_TAG = "jetbrains-262-preview-34842a2"
PINNED_ASSET_ID = 626772959
PINNED_ASSET_NAME = "agent-doc-jetbrains-262-0.2.511.zip"
PINNED_ASSET_SIZE = 2556136
PINNED_SHA256 = "2ef328588d1491446f1a45a83775ee220c13507f5cdd9bca4f5c2111f9e864a7"
# Recording the manifest is itself a commit on the PR head branch, so the PR
# head necessarily moves past the pinned source commit. A descendant head is
# admissible only when every change since the source commit is provenance
# bookkeeping that cannot reach the plugin ZIP (#previewrepin).
PROVENANCE_ONLY_PREFIXES = ("preview-artifacts/",)
PROVENANCE_ONLY_PATHS = frozenset(
    {
        "scripts/verify_jetbrains_preview.py",
        "docs/reference/jetbrains-262-preview-provenance.md",
    }
)
# GitHub's compare API truncates the file list at 300 entries.
COMPARE_FILE_LIMIT = 300


class VerificationError(RuntimeError):
    """A preview artifact or its recorded provenance does not match."""


def require_equal(label: str, actual: Any, expected: Any) -> None:
    if actual != expected:
        raise VerificationError(f"{label}: expected {expected!r}, got {actual!r}")


def load_manifest(path: Path) -> Dict[str, Any]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise VerificationError(f"cannot read manifest {path}: {error}") from error
    require_equal("manifest schema_version", manifest.get("schema_version"), 1)
    require_equal(
        "pinned source commit", manifest.get("source_commit"), PINNED_SOURCE_COMMIT
    )
    require_equal(
        "pinned release id", manifest.get("release", {}).get("id"), PINNED_RELEASE_ID
    )
    require_equal(
        "pinned release tag",
        manifest.get("release", {}).get("tag"),
        PINNED_RELEASE_TAG,
    )
    require_equal(
        "pinned asset id", manifest.get("asset", {}).get("id"), PINNED_ASSET_ID
    )
    require_equal(
        "pinned asset name", manifest.get("asset", {}).get("name"), PINNED_ASSET_NAME
    )
    require_equal(
        "pinned asset size", manifest.get("asset", {}).get("size"), PINNED_ASSET_SIZE
    )
    require_equal(
        "pinned SHA-256", manifest.get("asset", {}).get("sha256"), PINNED_SHA256
    )
    return manifest


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        raise VerificationError(f"cannot read {path}: {error}") from error
    return digest.hexdigest()


def unique_members(archive: zipfile.ZipFile, label: str) -> Dict[str, zipfile.ZipInfo]:
    members: Dict[str, zipfile.ZipInfo] = {}
    for info in archive.infolist():
        if info.filename in members:
            raise VerificationError(f"{label}: duplicate ZIP member {info.filename!r}")
        members[info.filename] = info
    return members


def required_text(element: ET.Element, name: str, label: str) -> str:
    child = element.find(name)
    if child is None or child.text is None or not child.text.strip():
        raise VerificationError(f"{label}: plugin.xml has no {name!r} value")
    return child.text.strip()


def verify_plugin_metadata(path: Path, manifest: Dict[str, Any], label: str) -> None:
    plugin = manifest["plugin"]
    root = plugin["archive_root"]
    expected_jar = f"{root}/lib/agent.doc-{plugin['version']}.jar"
    required_module_files = {
        f"{root}/lib/modules/{name}.jar" for name in plugin["required_modules"]
    }
    try:
        with zipfile.ZipFile(path) as archive:
            members = unique_members(archive, label)
            if expected_jar not in members:
                raise VerificationError(
                    f"{label}: missing versioned plugin JAR {expected_jar!r}"
                )
            missing_modules = sorted(required_module_files.difference(members))
            if missing_modules:
                raise VerificationError(
                    f"{label}: missing modular plugin JAR(s): {', '.join(missing_modules)}"
                )
            plugin_jar = archive.read(expected_jar)
        with zipfile.ZipFile(io.BytesIO(plugin_jar)) as archive:
            unique_members(archive, f"{label} plugin JAR")
            try:
                plugin_xml = archive.read("META-INF/plugin.xml")
            except KeyError as error:
                raise VerificationError(
                    f"{label}: plugin JAR has no META-INF/plugin.xml"
                ) from error
    except (OSError, zipfile.BadZipFile) as error:
        raise VerificationError(f"{label}: invalid plugin archive: {error}") from error

    try:
        idea_plugin = ET.fromstring(plugin_xml)
    except ET.ParseError as error:
        raise VerificationError(f"{label}: invalid plugin.xml: {error}") from error
    require_equal(f"{label} plugin id", required_text(idea_plugin, "id", label), plugin["id"])
    require_equal(
        f"{label} plugin version",
        required_text(idea_plugin, "version", label),
        plugin["version"],
    )
    idea_version = idea_plugin.find("idea-version")
    if idea_version is None:
        raise VerificationError(f"{label}: plugin.xml has no idea-version")
    require_equal(
        f"{label} since-build", idea_version.get("since-build"), plugin["since_build"]
    )
    require_equal(
        f"{label} until-build", idea_version.get("until-build"), plugin["until_build"]
    )
    content = idea_plugin.find("content")
    if content is None:
        raise VerificationError(f"{label}: plugin.xml has no modular content declaration")
    modules = {element.get("name") for element in content.findall("module")}
    require_equal(
        f"{label} module set", modules, set(plugin["required_modules"])
    )


def verify_artifact(path: Path, manifest: Dict[str, Any], label: str) -> None:
    if not path.is_file():
        raise VerificationError(f"{label}: not a regular file: {path}")
    asset = manifest["asset"]
    require_equal(f"{label} byte length", path.stat().st_size, asset["size"])
    require_equal(f"{label} SHA-256", sha256_file(path), asset["sha256"])
    verify_plugin_metadata(path, manifest, label)


def github_json(path: str) -> Dict[str, Any]:
    headers = {
        "Accept": "application/vnd.github+json",
        "User-Agent": "agent-doc-preview-verifier",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(f"{GITHUB_API}{path}", headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except (OSError, urllib.error.URLError, json.JSONDecodeError) as error:
        raise VerificationError(f"GitHub provenance lookup failed for {path}: {error}") from error


def is_provenance_only_path(path: Any) -> bool:
    return isinstance(path, str) and (
        path in PROVENANCE_ONLY_PATHS
        or any(path.startswith(prefix) for prefix in PROVENANCE_ONLY_PREFIXES)
    )


def verify_head_descends_by_provenance_only(
    source_commit: str, head_sha: Any, comparison: Dict[str, Any] | None
) -> None:
    """Admit a PR head past the source commit only for provenance-only commits."""
    if comparison is None:
        raise VerificationError(
            f"PR head: expected {source_commit!r} or a provenance-only descendant, "
            f"got {head_sha!r} with no comparison"
        )
    require_equal("PR head comparison status", comparison.get("status"), "ahead")
    require_equal(
        "PR head comparison base",
        comparison.get("merge_base_commit", {}).get("sha"),
        source_commit,
    )
    require_equal(
        "PR head comparison base",
        comparison.get("base_commit", {}).get("sha"),
        source_commit,
    )
    commits = comparison.get("commits") or []
    require_equal(
        "PR head comparison tip", commits[-1].get("sha") if commits else None, head_sha
    )
    files = comparison.get("files")
    if not isinstance(files, list) or not files:
        raise VerificationError("PR head: descendant comparison lists no changed files")
    if len(files) >= COMPARE_FILE_LIMIT:
        raise VerificationError(
            "PR head: descendant comparison file list may be truncated"
        )
    for entry in files:
        for key in ("filename", "previous_filename"):
            if key in entry and not is_provenance_only_path(entry[key]):
                raise VerificationError(
                    f"PR head: descendant {head_sha!r} changes non-provenance path "
                    f"{entry[key]!r} since {source_commit!r}"
                )


def verify_remote_provenance(
    manifest: Dict[str, Any],
    pull_request: Dict[str, Any],
    release: Dict[str, Any],
    comparison: Dict[str, Any] | None = None,
) -> None:
    repository = manifest["repository"]
    source_commit = manifest["source_commit"]
    require_equal("PR number", pull_request.get("number"), manifest["pull_request"])
    head_sha = pull_request.get("head", {}).get("sha")
    if head_sha != source_commit:
        verify_head_descends_by_provenance_only(source_commit, head_sha, comparison)
    require_equal(
        "PR head repository",
        pull_request.get("head", {}).get("repo", {}).get("full_name"),
        repository,
    )

    recorded_release = manifest["release"]
    require_equal("release id", release.get("id"), recorded_release["id"])
    require_equal("release tag", release.get("tag_name"), recorded_release["tag"])
    require_equal("release target commit", release.get("target_commitish"), source_commit)
    require_equal("release prerelease flag", release.get("prerelease"), recorded_release["prerelease"])
    require_equal("release publication time", release.get("published_at"), recorded_release["published_at"])

    expected = manifest["asset"]
    matching = [asset for asset in release.get("assets", []) if asset.get("name") == expected["name"]]
    require_equal("release asset count", len(matching), 1)
    asset = matching[0]
    for key in ("id", "name", "size", "created_at", "updated_at"):
        require_equal(f"release asset {key}", asset.get(key), expected[key])
    require_equal("release asset digest", asset.get("digest"), f"sha256:{expected['sha256']}")


def fetch_and_verify_remote_provenance(manifest: Dict[str, Any]) -> None:
    repository = manifest["repository"]
    pr_number = manifest["pull_request"]
    tag = manifest["release"]["tag"]
    pull_request = github_json(f"/repos/{repository}/pulls/{pr_number}")
    release = github_json(f"/repos/{repository}/releases/tags/{tag}")
    comparison = None
    head_sha = pull_request.get("head", {}).get("sha")
    if isinstance(head_sha, str) and head_sha != manifest["source_commit"]:
        comparison = github_json(
            f"/repos/{repository}/compare/{manifest['source_commit']}...{head_sha}"
        )
    verify_remote_provenance(manifest, pull_request, release, comparison)


def build_fixture(path: Path, manifest: Dict[str, Any], **overrides: str) -> None:
    plugin = manifest["plugin"]
    version = overrides.get("version", plugin["version"])
    since_build = overrides.get("since_build", plugin["since_build"])
    until_build = overrides.get("until_build", plugin["until_build"])
    plugin_id = overrides.get("plugin_id", plugin["id"])
    module_names: Iterable[str] = list(plugin["required_modules"])
    omit_module = overrides.get("omit_module")
    module_xml = "".join(f'<module name="{name}" />' for name in module_names)
    plugin_xml = f"""<idea-plugin>
  <idea-version since-build="{since_build}" until-build="{until_build}" />
  <version>{version}</version>
  <id>{plugin_id}</id>
  <content>
    {module_xml}
  </content>
</idea-plugin>
""".encode()
    jar_buffer = io.BytesIO()
    with zipfile.ZipFile(jar_buffer, "w") as archive:
        archive.writestr("META-INF/plugin.xml", plugin_xml)
    root = plugin["archive_root"]
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(f"{root}/lib/agent.doc-{plugin['version']}.jar", jar_buffer.getvalue())
        for name in module_names:
            if name == omit_module:
                continue
            archive.writestr(f"{root}/lib/modules/{name}.jar", b"fixture")


def fixture_manifest(path: Path, manifest: Dict[str, Any]) -> Dict[str, Any]:
    updated = copy.deepcopy(manifest)
    updated["asset"]["size"] = path.stat().st_size
    updated["asset"]["sha256"] = sha256_file(path)
    return updated


def expect_failure(fragment: str, operation: Callable[[], None]) -> None:
    try:
        operation()
    except VerificationError as error:
        if fragment not in str(error):
            raise AssertionError(f"expected {fragment!r} in {error!r}") from error
    else:
        raise AssertionError(f"expected VerificationError containing {fragment!r}")


def self_test() -> int:
    recorded = load_manifest(DEFAULT_MANIFEST)
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        valid = root / recorded["asset"]["name"]
        build_fixture(valid, recorded)
        local = fixture_manifest(valid, recorded)
        verify_artifact(valid, local, "backend")

        tampered = root / "tampered.zip"
        tampered_bytes = bytearray(valid.read_bytes())
        tampered_bytes[-1] ^= 0x01
        tampered.write_bytes(tampered_bytes)
        expect_failure("SHA-256", lambda: verify_artifact(tampered, local, "backend"))

        wrong_version = root / "wrong-version.zip"
        build_fixture(wrong_version, recorded, version="0.2.512")
        wrong_version_manifest = fixture_manifest(wrong_version, recorded)
        expect_failure(
            "plugin version",
            lambda: verify_artifact(wrong_version, wrong_version_manifest, "client/Gateway"),
        )

        wrong_since = root / "wrong-since.zip"
        build_fixture(wrong_since, recorded, since_build="261")
        wrong_since_manifest = fixture_manifest(wrong_since, recorded)
        expect_failure(
            "since-build",
            lambda: verify_artifact(wrong_since, wrong_since_manifest, "backend"),
        )

        wrong_id = root / "wrong-id.zip"
        build_fixture(wrong_id, recorded, plugin_id="com.example.impostor")
        wrong_id_manifest = fixture_manifest(wrong_id, recorded)
        expect_failure(
            "plugin id",
            lambda: verify_artifact(wrong_id, wrong_id_manifest, "backend"),
        )

        missing_module = root / "missing-module.zip"
        build_fixture(missing_module, recorded, omit_module="agent.doc.frontend")
        missing_module_manifest = fixture_manifest(missing_module, recorded)
        expect_failure(
            "missing modular plugin JAR",
            lambda: verify_artifact(
                missing_module, missing_module_manifest, "client/Gateway"
            ),
        )

        truncated = root / "truncated.zip"
        truncated.write_bytes(valid.read_bytes()[:-1])
        expect_failure("byte length", lambda: verify_artifact(truncated, local, "backend"))

        # A manifest edited to bless different bytes must not load.
        for key, value in (
            ("sha256", "0" * 64),
            ("size", PINNED_ASSET_SIZE + 1),
            ("id", PINNED_ASSET_ID + 1),
        ):
            drifted = copy.deepcopy(recorded)
            drifted["asset"][key] = value
            drifted_path = root / f"drifted-{key}.json"
            drifted_path.write_text(json.dumps(drifted), encoding="utf-8")
            expect_failure("pinned", lambda: load_manifest(drifted_path))
        drifted = copy.deepcopy(recorded)
        drifted["source_commit"] = "1" * 40
        drifted_path = root / "drifted-commit.json"
        drifted_path.write_text(json.dumps(drifted), encoding="utf-8")
        expect_failure("pinned source commit", lambda: load_manifest(drifted_path))

        # CLI: tampered bytes and a single reused copy both exit non-zero,
        # with the remote lookup stubbed so the self-test stays offline.
        global fetch_and_verify_remote_provenance
        real_fetch = fetch_and_verify_remote_provenance
        real_argv = sys.argv
        real_stderr = sys.stderr
        fetch_and_verify_remote_provenance = lambda manifest: None
        try:
            sys.stderr = io.StringIO()
            sys.argv = ["verify", "--backend", str(tampered), "--client-gateway", str(valid)]
            require_equal("CLI exit on tampered bytes", main(), 1)
            # The real pinned manifest gates the CLI, so non-genuine bytes are
            # refused on the first recorded property they violate.
            require_equal(
                "CLI tamper message",
                "preview verification FAILED: backend byte length" in sys.stderr.getvalue(),
                True,
            )
            sys.stderr = io.StringIO()
            sys.argv = ["verify", "--backend", str(valid), "--client-gateway", str(valid)]
            require_equal("CLI exit on reused copy", main(), 1)
            require_equal(
                "CLI same-file message", "same file" in sys.stderr.getvalue(), True
            )
        finally:
            fetch_and_verify_remote_provenance = real_fetch
            sys.argv = real_argv
            sys.stderr = real_stderr

        wrong_range = root / "wrong-range.zip"
        build_fixture(wrong_range, recorded, until_build="263.*")
        wrong_range_manifest = fixture_manifest(wrong_range, recorded)
        expect_failure(
            "until-build",
            lambda: verify_artifact(wrong_range, wrong_range_manifest, "backend"),
        )

    pr = {
        "number": recorded["pull_request"],
        "head": {"sha": recorded["source_commit"], "repo": {"full_name": recorded["repository"]}},
    }
    asset = dict(recorded["asset"])
    asset["digest"] = f"sha256:{asset.pop('sha256')}"
    release = {
        "id": recorded["release"]["id"],
        "tag_name": recorded["release"]["tag"],
        "target_commitish": recorded["source_commit"],
        "prerelease": recorded["release"]["prerelease"],
        "published_at": recorded["release"]["published_at"],
        "assets": [asset],
    }
    verify_remote_provenance(recorded, pr, release)
    replaced = copy.deepcopy(release)
    replaced["assets"][0]["id"] += 1
    expect_failure(
        "release asset id", lambda: verify_remote_provenance(recorded, pr, replaced)
    )
    advanced = copy.deepcopy(pr)
    advanced["head"]["sha"] = "0" * 40
    expect_failure("PR head", lambda: verify_remote_provenance(recorded, advanced, release))

    # The manifest commit itself advances the head: provenance-only descendants pass.
    def comparison(*paths: str, status: str = "ahead", renamed_from: str = "") -> Dict[str, Any]:
        files = [{"filename": path} for path in paths]
        if renamed_from:
            files[0]["previous_filename"] = renamed_from
        return {
            "status": status,
            "base_commit": {"sha": recorded["source_commit"]},
            "merge_base_commit": {"sha": recorded["source_commit"]},
            "commits": [{"sha": "0" * 40}],
            "files": files,
        }

    provenance = comparison(
        "preview-artifacts/jetbrains-262-preview-next.json",
        "scripts/verify_jetbrains_preview.py",
        "docs/reference/jetbrains-262-preview-provenance.md",
        renamed_from="preview-artifacts/jetbrains-262-preview-prev.json",
    )
    verify_remote_provenance(recorded, advanced, release, provenance)
    plugin_edit = comparison(
        "preview-artifacts/x.json", "editors/jetbrains-262/backend/src/main/kotlin/X.kt"
    )
    expect_failure(
        "non-provenance path",
        lambda: verify_remote_provenance(recorded, advanced, release, plugin_edit),
    )
    smuggled_rename = comparison(
        "preview-artifacts/x.json", renamed_from="editors/jetbrains-262/gradle.properties"
    )
    expect_failure(
        "non-provenance path",
        lambda: verify_remote_provenance(recorded, advanced, release, smuggled_rename),
    )
    diverged = comparison("preview-artifacts/x.json", status="diverged")
    expect_failure(
        "comparison status",
        lambda: verify_remote_provenance(recorded, advanced, release, diverged),
    )
    rebased = comparison("preview-artifacts/x.json")
    rebased["merge_base_commit"]["sha"] = "2" * 40
    expect_failure(
        "comparison base",
        lambda: verify_remote_provenance(recorded, advanced, release, rebased),
    )
    other_tip = comparison("preview-artifacts/x.json")
    other_tip["commits"][-1]["sha"] = "3" * 40
    expect_failure(
        "comparison tip",
        lambda: verify_remote_provenance(recorded, advanced, release, other_tip),
    )
    truncated_list = comparison(*[f"preview-artifacts/{i}.json" for i in range(COMPARE_FILE_LIMIT)])
    expect_failure(
        "truncated",
        lambda: verify_remote_provenance(recorded, advanced, release, truncated_list),
    )
    empty = comparison()
    expect_failure(
        "no changed files",
        lambda: verify_remote_provenance(recorded, advanced, release, empty),
    )
    redigested = copy.deepcopy(release)
    redigested["assets"][0]["digest"] = "sha256:" + "0" * 64
    expect_failure(
        "release asset digest",
        lambda: verify_remote_provenance(recorded, pr, redigested),
    )
    forked = copy.deepcopy(pr)
    forked["head"]["repo"]["full_name"] = "attacker/agent-doc"
    expect_failure(
        "PR head repository", lambda: verify_remote_provenance(recorded, forked, release)
    )
    retargeted = copy.deepcopy(release)
    retargeted["target_commitish"] = "main"
    expect_failure(
        "release target commit",
        lambda: verify_remote_provenance(recorded, pr, retargeted),
    )
    promoted = copy.deepcopy(release)
    promoted["prerelease"] = False
    expect_failure(
        "release prerelease flag",
        lambda: verify_remote_provenance(recorded, pr, promoted),
    )
    resized = copy.deepcopy(release)
    resized["assets"][0]["size"] += 1
    expect_failure(
        "release asset size", lambda: verify_remote_provenance(recorded, pr, resized)
    )
    reuploaded = copy.deepcopy(release)
    reuploaded["assets"][0]["updated_at"] = "2026-10-10T00:00:00Z"
    expect_failure(
        "release asset updated_at",
        lambda: verify_remote_provenance(recorded, pr, reuploaded),
    )
    removed = copy.deepcopy(release)
    removed["assets"] = []
    expect_failure(
        "release asset count", lambda: verify_remote_provenance(recorded, pr, removed)
    )
    duplicated = copy.deepcopy(release)
    duplicated["assets"].append(copy.deepcopy(duplicated["assets"][0]))
    expect_failure(
        "release asset count",
        lambda: verify_remote_provenance(recorded, pr, duplicated),
    )
    print("verify_jetbrains_preview self-test: ok")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", type=Path)
    parser.add_argument("--client-gateway", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.backend is None or args.client_gateway is None:
        parser.error("--backend and --client-gateway are required")

    try:
        manifest = load_manifest(DEFAULT_MANIFEST)
        try:
            if os.path.samefile(args.backend, args.client_gateway):
                raise VerificationError(
                    "backend and client/Gateway paths resolve to the same file; "
                    "verify the two independently downloaded copies"
                )
        except OSError as error:
            raise VerificationError(f"cannot compare preview paths: {error}") from error
        fetch_and_verify_remote_provenance(manifest)
        verify_artifact(args.backend, manifest, "backend")
        verify_artifact(args.client_gateway, manifest, "client/Gateway")
    except VerificationError as error:
        print(f"preview verification FAILED: {error}", file=sys.stderr)
        return 1

    print(
        "preview verification OK: "
        f"PR #{manifest['pull_request']} @ {manifest['source_commit']}; "
        f"both copies match {manifest['asset']['sha256']} and plugin "
        f"{manifest['plugin']['version']} ({manifest['plugin']['since_build']} to "
        f"{manifest['plugin']['until_build']})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
