from __future__ import annotations

import hashlib
import io
import os
import stat
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock


sys.path.insert(0, str(Path(__file__).parents[1]))

from agent_doc_bootstrap import cli


def release_tar(binary: bytes = b"binary", library: bytes = b"library") -> bytes:
    result = io.BytesIO()
    with tarfile.open(fileobj=result, mode="w:gz") as bundle:
        for name, data in (("agent-doc", binary), ("libagent_doc.so", library)):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o755 if name == "agent-doc" else 0o644
            bundle.addfile(info, io.BytesIO(data))
    return result.getvalue()


class BootstrapTests(unittest.TestCase):
    def test_release_target_covers_every_published_platform(self) -> None:
        cases = (
            ("Linux", "x86_64", False, "x86_64-unknown-linux-gnu"),
            ("Linux", "AMD64", True, "x86_64-unknown-linux-musl"),
            ("Linux", "arm64", False, "aarch64-unknown-linux-gnu"),
            ("Darwin", "x86_64", False, "x86_64-apple-darwin"),
            ("Darwin", "arm64", False, "aarch64-apple-darwin"),
            ("Windows", "AMD64", False, "x86_64-pc-windows-msvc"),
        )
        for system, machine, musl, expected in cases:
            with self.subTest(system=system, machine=machine, musl=musl):
                self.assertEqual(cli.release_target(system, machine, musl), expected)

    def test_unsupported_platform_fails_closed(self) -> None:
        with self.assertRaisesRegex(cli.BootstrapError, "no GitHub release asset"):
            cli.release_target("Linux", "riscv64", False)

    def test_manifest_requires_exact_asset_and_sha256(self) -> None:
        asset = "agent-doc-x86_64-unknown-linux-gnu.tar.gz"
        digest = "a" * 64
        self.assertEqual(cli._manifest_digest(f"{digest}  {asset}\n".encode(), asset), digest)
        with self.assertRaisesRegex(cli.BootstrapError, "no valid digest"):
            cli._manifest_digest(f"short  {asset}\n".encode(), asset)

    @mock.patch.object(cli, "release_target", return_value="x86_64-unknown-linux-gnu")
    def test_install_downloads_verifies_and_extracts_once(self, _target: mock.Mock) -> None:
        archive = release_tar()
        digest = hashlib.sha256(archive).hexdigest()
        asset = "agent-doc-x86_64-unknown-linux-gnu.tar.gz"
        responses = {
            "https://example.invalid/SHA256SUMS": f"{digest}  {asset}\n".encode(),
            f"https://example.invalid/{asset}": archive,
        }

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.dict(os.environ, {"AGENT_DOC_RELEASE_BASE_URL": "https://example.invalid"}), mock.patch.object(
                cli, "_download", side_effect=lambda url, _limit: responses[url]
            ) as download:
                binary = cli.ensure_installed("1.2.3", root)
                self.assertEqual(binary.read_bytes(), b"binary")
                self.assertEqual((binary.parent / "libagent_doc.so").read_bytes(), b"library")
                self.assertTrue(binary.stat().st_mode & stat.S_IXUSR)
                self.assertEqual(download.call_count, 2)
                self.assertEqual(cli.ensure_installed("1.2.3", root), binary)
                self.assertEqual(download.call_count, 2)

    @mock.patch.object(cli, "release_target", return_value="x86_64-unknown-linux-gnu")
    def test_install_rejects_checksum_mismatch(self, _target: mock.Mock) -> None:
        archive = release_tar()
        asset = "agent-doc-x86_64-unknown-linux-gnu.tar.gz"
        responses = {
            "https://example.invalid/SHA256SUMS": f"{'0' * 64}  {asset}\n".encode(),
            f"https://example.invalid/{asset}": archive,
        }
        with tempfile.TemporaryDirectory() as temporary, mock.patch.dict(
            os.environ, {"AGENT_DOC_RELEASE_BASE_URL": "https://example.invalid"}
        ), mock.patch.object(cli, "_download", side_effect=lambda url, _limit: responses[url]):
            with self.assertRaisesRegex(cli.BootstrapError, "SHA-256 mismatch"):
                cli.ensure_installed("1.2.3", Path(temporary))


if __name__ == "__main__":
    unittest.main()
