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
    @mock.patch.object(cli.Path, "glob")
    @mock.patch.object(cli.platform, "libc_ver", return_value=("glibc", "2.31"))
    def test_glibc_report_wins_over_stray_musl_loader(
        self, _libc_ver: mock.Mock, glob: mock.Mock
    ) -> None:
        glob.return_value = iter([Path("/lib/ld-musl-x86_64.so.1")])

        self.assertFalse(cli._is_musl())
        glob.assert_not_called()

    @mock.patch.object(cli.Path, "glob")
    @mock.patch.object(cli.platform, "libc_ver", return_value=("", ""))
    def test_musl_loader_is_only_a_fallback_when_libc_is_unknown(
        self, _libc_ver: mock.Mock, glob: mock.Mock
    ) -> None:
        glob.return_value = iter([Path("/lib/ld-musl-x86_64.so.1")])

        self.assertTrue(cli._is_musl())
        glob.assert_called_once_with("ld-musl-*.so.1")

    @mock.patch.object(cli.shutil, "which", return_value="/home/u/.local/bin/agent-doc")
    @mock.patch.object(cli.os, "execv", side_effect=OSError("test stop"))
    @mock.patch.object(cli, "ensure_installed", return_value=Path("/cache/agent-doc"))
    def test_bootstrap_advertises_pypi_entrypoint_to_native_binary(
        self, _installed: mock.Mock, execv: mock.Mock, _which: mock.Mock
    ) -> None:
        with mock.patch.object(cli.sys, "argv", ["agent-doc", "--version"]), mock.patch.dict(
            os.environ, {}, clear=True
        ), mock.patch("builtins.print"):
            self.assertEqual(cli.main(), 1)
            self.assertEqual(os.environ["AGENT_DOC_INSTALL_SOURCE"], "pypi")
            self.assertEqual(
                os.environ["AGENT_DOC_PYPI_ENTRYPOINT"], "/home/u/.local/bin/agent-doc"
            )
            execv.assert_called_once_with(Path("/cache/agent-doc"), ["/cache/agent-doc", "--version"])

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
