#!/usr/bin/env python3
"""Offline regressions for install.sh platform selection and verification."""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile


ROOT = Path(__file__).resolve().parent.parent
INSTALLER = ROOT / "install.sh"


def write_executable(path: Path, body: str) -> None:
    path.write_text(body)
    path.chmod(0o755)


def make_archive(path: Path) -> None:
    payload = path.parent / "payload"
    payload.mkdir()
    write_executable(payload / "agent-doc", "#!/bin/sh\necho 'agent-doc test'\n")
    (payload / "libagent_doc.so").write_bytes(b"test library")
    with tarfile.open(path, "w:gz") as archive:
        archive.add(payload / "agent-doc", arcname="agent-doc")
        archive.add(payload / "libagent_doc.so", arcname="libagent_doc.so")


def run_installer(
    *, libc: str, corrupt_checksum: bool = False
) -> tuple[subprocess.CompletedProcess[str], list[str], bool]:
    with tempfile.TemporaryDirectory(prefix="agent-doc-install-script-") as temp_dir:
        temp = Path(temp_dir)
        fake_bin = temp / "bin"
        fake_bin.mkdir()
        install_dir = temp / "install"
        request_log = temp / "requests.log"
        archive_name = f"agent-doc-x86_64-unknown-linux-{libc}.tar.gz"
        archive = temp / archive_name
        make_archive(archive)
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if corrupt_checksum:
            digest = "0" * 64
        sidecar = temp / f"{archive_name}.sha256"
        sidecar.write_text(f"{digest}  {archive_name}\n")

        write_executable(
            fake_bin / "uname",
            "#!/bin/sh\n"
            "case \"${1:-}\" in\n"
            "  -s) echo Linux ;;\n"
            "  -m) echo x86_64 ;;\n"
            "  *) echo Linux ;;\n"
            "esac\n",
        )
        ldd_report = "musl libc (x86_64)" if libc == "musl" else "ldd (GNU libc) 2.40"
        write_executable(fake_bin / "ldd", f"#!/bin/sh\necho '{ldd_report}'\n")
        write_executable(
            fake_bin / "curl",
            "#!/bin/sh\n"
            "out=\nurl=\n"
            "while [ \"$#\" -gt 0 ]; do\n"
            "  case \"$1\" in\n"
            "    -o) out=$2; shift 2 ;;\n"
            "    http*) url=$1; shift ;;\n"
            "    *) shift ;;\n"
            "  esac\n"
            "done\n"
            "printf '%s\\n' \"$url\" >> \"$FAKE_REQUEST_LOG\"\n"
            "case \"$url\" in\n"
            "  *.sha256) cp \"$FAKE_SIDECAR\" \"$out\" ;;\n"
            "  *) cp \"$FAKE_ARCHIVE\" \"$out\" ;;\n"
            "esac\n",
        )

        env = os.environ.copy()
        env.update(
            {
                "AGENT_DOC_INSTALL_DIR": str(install_dir),
                "AGENT_DOC_VERSION": "v0.test",
                "FAKE_ARCHIVE": str(archive),
                "FAKE_REQUEST_LOG": str(request_log),
                "FAKE_SIDECAR": str(sidecar),
                "PATH": f"{fake_bin}:{env['PATH']}",
            }
        )
        result = subprocess.run(
            ["sh", str(INSTALLER)],
            cwd=ROOT,
            env=env,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        requests = request_log.read_text().splitlines() if request_log.exists() else []
        installed = (install_dir / "agent-doc").is_file()
        # The editor plugins resolve the native library as a sibling of the
        # binary; an install that drops it leaves them in degraded IPC mode.
        if installed:
            assert (install_dir / "libagent_doc.so").read_bytes() == b"test library", result.stdout
            assert not list(install_dir.glob(".*.tmp")), list(install_dir.iterdir())
        return result, requests, installed


def main() -> None:
    gnu, requests, installed = run_installer(libc="gnu")
    assert gnu.returncode == 0, gnu.stdout
    assert requests[-2].endswith("agent-doc-x86_64-unknown-linux-gnu.tar.gz"), requests
    assert requests[-1].endswith("agent-doc-x86_64-unknown-linux-gnu.tar.gz.sha256"), requests
    assert installed
    assert "Checksum verified" in gnu.stdout
    assert "Installed native library" in gnu.stdout
    assert "Next steps" in gnu.stdout

    musl, requests, _ = run_installer(libc="musl")
    assert musl.returncode == 0, musl.stdout
    assert requests[-2].endswith("agent-doc-x86_64-unknown-linux-musl.tar.gz"), requests

    corrupt, _, installed = run_installer(libc="gnu", corrupt_checksum=True)
    assert corrupt.returncode != 0, corrupt.stdout
    assert "Integrity check failed" in corrupt.stdout
    assert not installed

    print("install.sh self-test: ok")


if __name__ == "__main__":
    main()
