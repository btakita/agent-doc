#!/usr/bin/env python3
"""Build a verified, rollback-aware per-target stable channel manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import tempfile
from pathlib import Path
from typing import Optional

SCHEMA = 1
EDITOR_MANIFEST = "EDITOR-PACKAGES.sha256"
VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
TARGET_RE = re.compile(r"^[a-z0-9_]+(?:-[a-z0-9_]+)+$")


def version_tuple(version: str) -> tuple[int, int, int]:
    if not VERSION_RE.fullmatch(version):
        raise ValueError(f"invalid release version: {version!r}")
    return tuple(int(part) for part in version.split("."))  # type: ignore[return-value]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def checksum_entry(sidecar: Path, asset_name: str) -> str:
    matches: list[str] = []
    for line in sidecar.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[1].lstrip("*") == asset_name:
            matches.append(fields[0].lower())
    if len(matches) != 1 or not re.fullmatch(r"[0-9a-f]{64}", matches[0]):
        raise ValueError(f"{sidecar} must contain exactly one SHA-256 entry for {asset_name}")
    return matches[0]


def validate_editor_manifest(path: Path) -> None:
    names: set[str] = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        fields = line.split()
        if len(fields) != 2 or not re.fullmatch(r"[0-9a-fA-F]{64}", fields[0]):
            raise ValueError(f"{path} contains an invalid checksum entry")
        names.add(fields[1].lstrip("*"))
    required = (
        re.compile(r"^agent-doc-jetbrains-[0-9]+(?:\.[0-9]+)*\.zip$"),
        re.compile(r"^agent-doc-jetbrains-262-[0-9]+(?:\.[0-9]+)*\.zip$"),
        re.compile(r"^agent-doc-jetbrains-263-[0-9]+(?:\.[0-9]+)*\.zip$"),
    )
    if any(sum(pattern.fullmatch(name) is not None for name in names) != 1 for pattern in required):
        raise ValueError(f"{path} must bind exactly one classic, 262, and 263 JetBrains package")


def load_current(path: Optional[Path], target: str) -> Optional[dict[str, object]]:
    if path is None or not path.exists():
        return None
    value = json.loads(path.read_text(encoding="utf-8"))
    if value.get("schema") != SCHEMA or value.get("target") != target:
        raise ValueError(f"current channel manifest is incompatible with target {target}")
    version_tuple(str(value.get("version", "")))
    return value


def build_manifest(
    *,
    target: str,
    version: str,
    archive: Path,
    archive_sidecar: Path,
    editor_manifest: Path,
    current: Optional[Path] = None,
    allow_rollback: bool = False,
) -> tuple[str, dict[str, object]]:
    if not TARGET_RE.fullmatch(target):
        raise ValueError(f"invalid target: {target!r}")
    requested = version_tuple(version)
    expected_archive = f"agent-doc-{target}.zip" if target.endswith("windows-msvc") else f"agent-doc-{target}.tar.gz"
    if archive.name != expected_archive:
        raise ValueError(f"archive must be named {expected_archive}, got {archive.name}")
    if editor_manifest.name != EDITOR_MANIFEST:
        raise ValueError(f"editor manifest must be named {EDITOR_MANIFEST}")
    validate_editor_manifest(editor_manifest)

    sidecar_digest = checksum_entry(archive_sidecar, archive.name)
    archive_digest = sha256(archive)
    if sidecar_digest != archive_digest:
        raise ValueError(f"archive checksum mismatch for {archive.name}")

    previous = load_current(current, target)
    if previous is not None and requested < version_tuple(str(previous["version"])) and not allow_rollback:
        return "stale", previous

    manifest: dict[str, object] = {
        "schema": SCHEMA,
        "target": target,
        "version": version,
        "release_tag": f"v{version}",
        "archive": archive.name,
        "archive_sha256": archive_digest,
        "editor_manifest": EDITOR_MANIFEST,
        "editor_manifest_sha256": sha256(editor_manifest),
    }
    return ("rollback" if previous and requested < version_tuple(str(previous["version"])) else "promote"), manifest


def write_manifest(output_dir: Path, target: str, manifest: dict[str, object]) -> tuple[Path, Path]:
    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / f"stable-{target}.json"
    data = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    path.write_bytes(data)
    sidecar = output_dir / f"{path.name}.sha256"
    sidecar.write_text(f"{hashlib.sha256(data).hexdigest()}  {path.name}\n", encoding="utf-8")
    return path, sidecar


def self_test() -> None:
    with tempfile.TemporaryDirectory() as raw:
        root = Path(raw)
        target = "x86_64-unknown-linux-gnu"
        archive = root / f"agent-doc-{target}.tar.gz"
        archive.write_bytes(b"archive")
        archive_sidecar = root / f"{archive.name}.sha256"
        archive_sidecar.write_text(f"{sha256(archive)}  {archive.name}\n", encoding="utf-8")
        editor = root / EDITOR_MANIFEST
        editor.write_text(
            "\n".join(
                [
                    f"{'1' * 64}  agent-doc-jetbrains-800.zip",
                    f"{'2' * 64}  agent-doc-jetbrains-262-801.zip",
                    f"{'3' * 64}  agent-doc-jetbrains-263-802.zip",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        action, manifest = build_manifest(
            target=target,
            version="1.2.3",
            archive=archive,
            archive_sidecar=archive_sidecar,
            editor_manifest=editor,
        )
        assert action == "promote"
        channel, sidecar = write_manifest(root / "out", target, manifest)
        assert checksum_entry(sidecar, channel.name) == sha256(channel)

        current = root / "current.json"
        current.write_text(json.dumps({**manifest, "version": "2.0.0"}), encoding="utf-8")
        action, retained = build_manifest(
            target=target,
            version="1.2.3",
            archive=archive,
            archive_sidecar=archive_sidecar,
            editor_manifest=editor,
            current=current,
        )
        assert action == "stale" and retained["version"] == "2.0.0"
        action, rolled_back = build_manifest(
            target=target,
            version="1.2.3",
            archive=archive,
            archive_sidecar=archive_sidecar,
            editor_manifest=editor,
            current=current,
            allow_rollback=True,
        )
        assert action == "rollback" and rolled_back["version"] == "1.2.3"

        archive.write_bytes(b"tampered")
        try:
            build_manifest(
                target=target,
                version="1.2.3",
                archive=archive,
                archive_sidecar=archive_sidecar,
                editor_manifest=editor,
            )
        except ValueError as error:
            assert "checksum mismatch" in str(error)
        else:
            raise AssertionError("tampered archive was accepted")
    print("target-stable-channel self-test: ok")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--target")
    parser.add_argument("--version")
    parser.add_argument("--archive", type=Path)
    parser.add_argument("--archive-sidecar", type=Path)
    parser.add_argument("--editor-manifest", type=Path)
    parser.add_argument("--current", type=Path)
    parser.add_argument("--allow-rollback", action="store_true")
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    required = (args.target, args.version, args.archive, args.archive_sidecar, args.editor_manifest, args.output_dir)
    if any(value is None for value in required):
        parser.error("--target, --version, --archive, --archive-sidecar, --editor-manifest, and --output-dir are required")
    action, manifest = build_manifest(
        target=args.target,
        version=args.version,
        archive=args.archive,
        archive_sidecar=args.archive_sidecar,
        editor_manifest=args.editor_manifest,
        current=args.current,
        allow_rollback=args.allow_rollback,
    )
    if action != "stale":
        path, sidecar = write_manifest(args.output_dir, args.target, manifest)
        print(f"action={action}")
        print(f"manifest={path}")
        print(f"sidecar={sidecar}")
    else:
        print("action=stale")


if __name__ == "__main__":
    main()
