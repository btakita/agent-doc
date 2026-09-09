#!/usr/bin/env python3
"""Fail release verification on missing editor coverage or required capability drift."""

import argparse
from pathlib import Path
import tomllib


def rows(path: Path, width: int) -> list[list[str]]:
    result = []
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line or line.startswith("#"):
            continue
        fields = line.split("\t")
        if len(fields) != width or any(not field.strip() for field in fields):
            raise ValueError(f"{path}:{number}: expected {width} nonempty tab-separated fields")
        result.append(fields)
    return result


def verify(root: Path) -> str:
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    version = manifest["package"]["version"]
    releases = rows(root / "editors/release-parity.tsv", 6)
    matching = [row for row in releases if row[0] == version]
    if len(matching) != 1:
        raise ValueError(f"release {version} requires exactly one editor parity audit row")
    for feature, requirement, jetbrains, vscode, zed in rows(root / "editors/plugin-parity.tsv", 5):
        if requirement == "required":
            for editor, status in [("JetBrains", jetbrains), ("VS Code", vscode)]:
                if status not in {"supported", "conditional"}:
                    raise ValueError(f"required capability {feature} is {status} in {editor}")
        if zed not in {"staged", "supported", "conditional"}:
            raise ValueError(f"invalid Zed capability status for {feature}: {zed}")
    return f"Editor parity audit: {version} covered; required JetBrains/VS Code capabilities declared."


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    try:
        print(verify(args.root))
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"editor parity audit failed: {error}\n")
