#!/usr/bin/env python3
"""Render the Homebrew formula for agent-doc from a release's SHA256SUMS (GH #31).

The formula lives in the tap repo `btakita/homebrew-tap` as `Formula/agent-doc.rb`.
The `homebrew` workflow runs this script after every tag's hosted builds, so the
formula pins the per-platform archives and digests of one release.

Inputs are the release's own `SHA256SUMS` manifest — the platform-archive-only
manifest the Release workflow publishes — never digests recomputed from a
second download.

Layout (GH #52): every archive carries the `agent-doc` binary AND the cdylib.
`agent-doc lib-path` resolves the library as a SIBLING of `current_exe()`, and
macOS reports the invoked path (the Homebrew `bin/` symlink), not its target.
So both files go into `libexec/` and `bin/agent-doc` is an exec wrapper, which
makes `current_exe()` the `libexec/agent-doc` real path on every platform.

The renderer still accepts a legacy or partially repaired release with no
Darwin archives by emitting a Linux-only formula. A partial Darwin pair is
always refused.

Usage:
  bump-homebrew-formula.py --tag v0.35.451 --sums SHA256SUMS [--output Formula/agent-doc.rb]
  bump-homebrew-formula.py --self-test

When `--output` already holds a NEWER version the script writes nothing and
exits 0 (a re-run for an older tag must not downgrade the tap). The same
version is rewritten, which is how the Darwin refresh lands.

Exit codes: 0 = written / up to date / skipped as older, 1 = refused input.
"""

from __future__ import annotations

import re
import sys
import tempfile
from pathlib import Path
from typing import Dict, List, Optional, Tuple

REPO = "btakita/agent-doc"
HOMEPAGE = "https://github.com/" + REPO
DESC = "Interactive document sessions with AI agents"
LICENSE = "MIT"

# Rust target triple -> (Homebrew OS block, Homebrew arch block). The musl and
# Windows archives are deliberately absent: Homebrew on Linux provides glibc, and
# the GNU archives carry the dynamically linked cdylib the editor plugins load.
TARGETS: Dict[str, Tuple[str, str]] = {
    "x86_64-apple-darwin": ("macos", "intel"),
    "aarch64-apple-darwin": ("macos", "arm"),
    "x86_64-unknown-linux-gnu": ("linux", "intel"),
    "aarch64-unknown-linux-gnu": ("linux", "arm"),
}
ARCHIVE = re.compile(r"^agent-doc-(?P<target>[A-Za-z0-9_\-]+)\.tar\.gz$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
TAG = re.compile(r"^v?(?P<version>\d+\.\d+\.\d+)$")
VERSION_LINE = re.compile(r'^\s*version\s+"(?P<version>[^"]+)"', re.MULTILINE)


class Refused(Exception):
    """Input that must not produce a formula."""


def parse_version(tag: str) -> str:
    match = TAG.match(tag.strip())
    if match is None:
        raise Refused("tag {!r} is not vMAJOR.MINOR.PATCH".format(tag))
    return match.group("version")


def version_key(version: str) -> Tuple[int, ...]:
    return tuple(int(part) for part in version.split("."))


def parse_sums(text: str) -> Dict[str, str]:
    """Map Rust target -> sha256 for the Homebrew-relevant archives in SHA256SUMS.

    Accepts both `sha256sum` (`<hex>  <name>`) and binary-mode (`<hex> *<name>`)
    lines. A malformed digest on a relevant archive refuses the whole manifest.
    """
    digests: Dict[str, str] = {}
    for lineno, raw in enumerate(text.splitlines(), start=1):
        line = raw.strip()
        if not line:
            continue
        parts = line.split(None, 1)
        if len(parts) != 2:
            raise Refused("SHA256SUMS line {}: expected '<sha256>  <file>'".format(lineno))
        digest, name = parts[0].lower(), parts[1].strip().lstrip("*")
        match = ARCHIVE.match(name)
        if match is None or match.group("target") not in TARGETS:
            continue
        if SHA256.match(digest) is None:
            raise Refused("SHA256SUMS line {}: {!r} is not a sha256 digest".format(lineno, parts[0]))
        target = match.group("target")
        if target in digests and digests[target] != digest:
            raise Refused("SHA256SUMS lists {} twice with different digests".format(name))
        digests[target] = digest
    return digests


def _platform_block(os_name: str, digests: Dict[str, str]) -> List[str]:
    lines = ["  on_{} do".format(os_name)]
    arms = [
        (arch, target)
        for target, (target_os, arch) in TARGETS.items()
        if target_os == os_name and target in digests
    ]
    for index, (arch, target) in enumerate(sorted(arms, key=lambda item: item[0] != "intel")):
        if index:
            lines.append("")
        lines.extend(
            [
                "    on_{} do".format(arch),
                '      url "{}/releases/download/v#{{version}}/agent-doc-{}.tar.gz"'.format(HOMEPAGE, target),
                '      sha256 "{}"'.format(digests[target]),
                "    end",
            ]
        )
    lines.append("  end")
    return lines


def render_formula(version: str, digests: Dict[str, str]) -> str:
    linux = [t for t, (os_name, _) in TARGETS.items() if os_name == "linux"]
    macos = [t for t, (os_name, _) in TARGETS.items() if os_name == "macos"]
    missing_linux = [t for t in linux if t not in digests]
    if missing_linux:
        raise Refused("SHA256SUMS has no archive for {}".format(", ".join(missing_linux)))
    present_macos = [t for t in macos if t in digests]
    if present_macos and len(present_macos) != len(macos):
        # release-macos-assets builds both or uploads neither; half a macOS block
        # would install the wrong architecture's binary on the other Mac.
        missing = [t for t in macos if t not in digests]
        raise Refused("SHA256SUMS has a partial Darwin set; missing {}".format(", ".join(missing)))

    lines = [
        "# typed: false",
        "# frozen_string_literal: true",
        "",
        "# Generated by agent-doc scripts/bump-homebrew-formula.py from the v{} SHA256SUMS.".format(version),
        "# Do not edit by hand: the agent-doc Release workflow rewrites it on every tag.",
        "class AgentDoc < Formula",
        '  desc "{}"'.format(DESC),
        '  homepage "{}"'.format(HOMEPAGE),
        '  version "{}"'.format(version),
        '  license "{}"'.format(LICENSE),
        "",
    ]
    if present_macos:
        lines.extend(_platform_block("macos", digests))
        lines.append("")
    else:
        lines.extend(
            [
                "  # This legacy release has no complete Darwin archive pair.",
                "  # A later repair and formula bump may add macOS archives.",
                "  depends_on :linux",
                "",
            ]
        )
    lines.extend(_platform_block("linux", digests))
    lines.extend(
        [
            "",
            "  def install",
            "    # GH #52: `agent-doc lib-path` resolves the cdylib as a sibling of the running",
            "    # executable, and macOS reports the invoked symlink rather than its target. Keep",
            "    # both files in libexec and expose bin/agent-doc as an exec wrapper so the",
            "    # running executable is always libexec/agent-doc.",
            '    library = OS.mac? ? "libagent_doc.dylib" : "libagent_doc.so"',
            '    libexec.install "agent-doc", library',
            '    bin.write_exec_script libexec/"agent-doc"',
            "  end",
            "",
            "  test do",
            '    assert_match version.to_s, shell_output("#{bin}/agent-doc --version")',
            '    assert_match "#{libexec}/libagent_doc", shell_output("#{bin}/agent-doc lib-path")',
            "  end",
            "end",
            "",
        ]
    )
    return "\n".join(lines)


def existing_version(path: Path) -> Optional[str]:
    if not path.exists():
        return None
    match = VERSION_LINE.search(path.read_text(encoding="utf-8"))
    return match.group("version") if match else None


def bump(tag: str, sums_text: str, output: Optional[Path]) -> str:
    version = parse_version(tag)
    formula = render_formula(version, parse_sums(sums_text))
    if output is None:
        sys.stdout.write(formula)
        return "printed"
    current = existing_version(output)
    if current is not None and TAG.match(current) and version_key(current) > version_key(version):
        return "skipped: {} already pins newer {}".format(output, current)
    if output.exists() and output.read_text(encoding="utf-8") == formula:
        return "unchanged: {} already pins {}".format(output, version)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(formula, encoding="utf-8")
    return "wrote {} for {}".format(output, version)


# --------------------------------------------------------------------------- self-test

_HEX = "0123456789abcdef"


def _digest(seed: int) -> str:
    return (_HEX[seed % 16] * 64)


def _fixture_sums(targets: List[str], extra: str = "") -> str:
    names = ["agent-doc-{}.tar.gz".format(t) for t in targets]
    names.append("agent-doc-x86_64-unknown-linux-musl.tar.gz")
    names.append("agent-doc-x86_64-pc-windows-msvc.zip")
    rows = ["{}  {}".format(_digest(i + 1), name) for i, name in enumerate(sorted(names))]
    return "\n".join(rows) + "\n" + extra


def _expect_refused(fn, *args) -> str:
    try:
        fn(*args)
    except Refused as error:
        return str(error)
    raise AssertionError("expected refusal from {}{}".format(fn.__name__, args))


def self_test() -> str:
    linux = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
    darwin = ["aarch64-apple-darwin", "x86_64-apple-darwin"]

    # Real v0.35.451 manifest shape: Linux + Windows from CI, no Darwin yet.
    linux_only = render_formula("0.35.451", parse_sums(_fixture_sums(linux)))
    assert 'version "0.35.451"' in linux_only
    assert "depends_on :linux" in linux_only
    assert "on_macos" not in linux_only
    assert linux_only.count("on_intel do") == 1 and linux_only.count("on_arm do") == 1
    assert "agent-doc-x86_64-unknown-linux-gnu.tar.gz" in linux_only
    assert "agent-doc-aarch64-unknown-linux-gnu.tar.gz" in linux_only
    assert "musl" not in linux_only and "windows" not in linux_only
    assert 'libexec.install "agent-doc", library' in linux_only
    assert 'bin.write_exec_script libexec/"agent-doc"' in linux_only
    assert "/releases/download/v#{version}/" in linux_only
    # each url is followed by ITS OWN digest, not a neighbour's
    sums = parse_sums(_fixture_sums(linux))
    for target in linux:
        url_line = "agent-doc-{}.tar.gz".format(target)
        after = linux_only.split(url_line, 1)[1].splitlines()[1]
        assert after.strip() == 'sha256 "{}"'.format(sums[target]), after

    # After make release-macos-assets refreshes SHA256SUMS: both OS blocks.
    full = render_formula("0.35.451", parse_sums(_fixture_sums(linux + darwin)))
    assert "depends_on :linux" not in full
    assert full.index("on_macos do") < full.index("on_linux do")
    assert full.count("on_intel do") == 2 and full.count("on_arm do") == 2
    assert "agent-doc-aarch64-apple-darwin.tar.gz" in full

    # Binary-mode sums lines (shasum -b) and blank lines parse identically.
    binary_mode = _fixture_sums(linux).replace("  agent-doc", " *agent-doc") + "\n\n"
    assert parse_sums(binary_mode) == sums

    # Refusals: missing Linux arch, partial Darwin, bad digest, bad tag, duplicates.
    assert "x86_64-unknown-linux-gnu" in _expect_refused(
        render_formula, "1.2.3", parse_sums(_fixture_sums(["aarch64-unknown-linux-gnu"]))
    )
    assert "partial Darwin" in _expect_refused(
        render_formula, "1.2.3", parse_sums(_fixture_sums(linux + ["aarch64-apple-darwin"]))
    )
    assert "not a sha256" in _expect_refused(
        parse_sums, "deadbeef  agent-doc-x86_64-unknown-linux-gnu.tar.gz\n"
    )
    assert "twice" in _expect_refused(
        parse_sums,
        _fixture_sums(linux, "{}  agent-doc-x86_64-unknown-linux-gnu.tar.gz\n".format("e" * 64)),
    )
    assert "vMAJOR" in _expect_refused(parse_version, "latest")
    assert parse_version("v0.35.451") == "0.35.451"
    # irrelevant assets with odd digests never refuse the manifest
    assert parse_sums(_fixture_sums(linux, "zz  EDITOR-PACKAGES.sha256\n")) == sums

    # Output handling: write, idempotent rewrite, Darwin refresh, no downgrade.
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "Formula" / "agent-doc.rb"
        assert bump("v0.35.451", _fixture_sums(linux), out).startswith("wrote")
        assert bump("v0.35.451", _fixture_sums(linux), out).startswith("unchanged")
        assert bump("v0.35.451", _fixture_sums(linux + darwin), out).startswith("wrote")
        assert "on_macos do" in out.read_text(encoding="utf-8")
        assert bump("v0.35.452", _fixture_sums(linux), out).startswith("wrote")
        before = out.read_text(encoding="utf-8")
        assert bump("v0.35.99", _fixture_sums(linux + darwin), out).startswith("skipped")
        assert out.read_text(encoding="utf-8") == before
        assert existing_version(out) == "0.35.452"

    return "[self-test] bump-homebrew-formula: ok"


def main(argv: List[str]) -> int:
    if argv == ["--self-test"]:
        print(self_test())
        return 0
    import argparse

    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument("--tag", required=True, help="release tag, e.g. v0.35.451")
    parser.add_argument("--sums", required=True, type=Path, help="the release's SHA256SUMS file")
    parser.add_argument("--output", type=Path, help="formula path to write (default: stdout)")
    args = parser.parse_args(argv)
    try:
        result = bump(args.tag, args.sums.read_text(encoding="utf-8"), args.output)
    except Refused as error:
        print("[homebrew-formula] refused: {}".format(error), file=sys.stderr)
        return 1
    if result != "printed":
        print("[homebrew-formula] {}".format(result))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
