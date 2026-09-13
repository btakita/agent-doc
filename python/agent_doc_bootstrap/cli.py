"""Download and execute the native agent-doc release pinned by the wheel."""

from __future__ import annotations

import hashlib
import importlib.metadata
import io
import os
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path


REPOSITORY = "btakita/agent-doc"
MAX_DOWNLOAD_BYTES = 256 * 1024 * 1024
MAX_MANIFEST_BYTES = 1024 * 1024
LOCK_TIMEOUT_SECONDS = 120.0


class BootstrapError(RuntimeError):
    """A native release could not be selected, verified, or installed."""


def _is_musl() -> bool:
    libc, _ = platform.libc_ver()
    if libc.lower() == "musl":
        return True
    return any(Path("/lib").glob("ld-musl-*.so.1"))


def release_target(
    system: str | None = None,
    machine: str | None = None,
    musl: bool | None = None,
) -> str:
    system = (system or platform.system()).lower()
    machine = (machine or platform.machine()).lower()
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)

    if system == "darwin" and machine in {"x86_64", "aarch64"}:
        return f"{machine}-apple-darwin"
    if system == "windows" and machine == "x86_64":
        return "x86_64-pc-windows-msvc"
    if system == "linux" and machine == "aarch64":
        return "aarch64-unknown-linux-gnu"
    if system == "linux" and machine == "x86_64":
        return "x86_64-unknown-linux-musl" if (musl if musl is not None else _is_musl()) else "x86_64-unknown-linux-gnu"
    raise BootstrapError(f"agent-doc has no GitHub release asset for {system}/{machine}")


def archive_name(target: str) -> str:
    suffix = ".zip" if target.endswith("windows-msvc") else ".tar.gz"
    return f"agent-doc-{target}{suffix}"


def expected_members(target: str) -> tuple[str, str]:
    if target.endswith("windows-msvc"):
        return "agent-doc.exe", "agent_doc.dll"
    library = "libagent_doc.dylib" if target.endswith("apple-darwin") else "libagent_doc.so"
    return "agent-doc", library


def cache_root() -> Path:
    override = os.environ.get("AGENT_DOC_BOOTSTRAP_CACHE")
    if override:
        return Path(override).expanduser()
    if os.name == "nt":
        base = os.environ.get("LOCALAPPDATA")
        return Path(base) / "agent-doc" if base else Path.home() / "AppData" / "Local" / "agent-doc"
    base = os.environ.get("XDG_CACHE_HOME")
    return Path(base).expanduser() / "agent-doc" if base else Path.home() / ".cache" / "agent-doc"


def _download(url: str, limit: int) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "agent-doc-pypi-bootstrap"})
    with urllib.request.urlopen(request, timeout=60) as response:
        declared = response.headers.get("Content-Length")
        if declared is not None and int(declared) > limit:
            raise BootstrapError(f"download exceeds {limit} bytes: {url}")
        body = response.read(limit + 1)
    if len(body) > limit:
        raise BootstrapError(f"download exceeds {limit} bytes: {url}")
    return body


def _manifest_digest(manifest: bytes, asset: str) -> str:
    try:
        text = manifest.decode("utf-8")
    except UnicodeDecodeError as error:
        raise BootstrapError("GitHub release checksum manifest is not UTF-8") from error
    for line in text.splitlines():
        fields = line.split()
        if len(fields) == 2 and fields[1].lstrip("*") == asset:
            digest = fields[0].lower()
            if len(digest) == 64 and all(char in "0123456789abcdef" for char in digest):
                return digest
            break
    raise BootstrapError(f"SHA256SUMS has no valid digest for {asset}")


def _write_member(destination: Path, data: bytes, executable: bool) -> None:
    if len(data) > MAX_DOWNLOAD_BYTES:
        raise BootstrapError(f"release member is unexpectedly large: {destination.name}")
    destination.write_bytes(data)
    destination.chmod(0o755 if executable else 0o644)


def _extract_archive(archive: bytes, asset: str, target: str, destination: Path) -> None:
    binary_name, library_name = expected_members(target)
    wanted = (binary_name, library_name)
    if asset.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
            for name in wanted:
                try:
                    info = bundle.getinfo(name)
                except KeyError as error:
                    raise BootstrapError(f"{asset} is missing {name}") from error
                if info.is_dir() or info.file_size > MAX_DOWNLOAD_BYTES:
                    raise BootstrapError(f"invalid release member: {name}")
                _write_member(destination / name, bundle.read(info), name == binary_name)
        return

    try:
        bundle = tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz")
    except tarfile.TarError as error:
        raise BootstrapError(f"invalid release archive: {asset}") from error
    with bundle:
        for name in wanted:
            try:
                member = bundle.getmember(name)
            except KeyError as error:
                raise BootstrapError(f"{asset} is missing {name}") from error
            if not member.isfile() or member.size > MAX_DOWNLOAD_BYTES:
                raise BootstrapError(f"invalid release member: {name}")
            source = bundle.extractfile(member)
            if source is None:
                raise BootstrapError(f"could not read release member: {name}")
            _write_member(destination / name, source.read(MAX_DOWNLOAD_BYTES + 1), name == binary_name)


def _cached_binary(directory: Path, target: str) -> Path | None:
    binary_name, library_name = expected_members(target)
    binary = directory / binary_name
    library = directory / library_name
    return binary if binary.is_file() and library.is_file() else None


def _acquire_lock(path: Path) -> int:
    deadline = time.monotonic() + LOCK_TIMEOUT_SECONDS
    while True:
        try:
            return os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        except FileExistsError:
            try:
                if time.time() - path.stat().st_mtime > LOCK_TIMEOUT_SECONDS:
                    path.unlink()
                    continue
            except FileNotFoundError:
                continue
            if time.monotonic() >= deadline:
                raise BootstrapError(f"timed out waiting for bootstrap lock {path}")
            time.sleep(0.05)


def ensure_installed(version: str | None = None, root: Path | None = None) -> Path:
    version = version or importlib.metadata.version("agent-doc")
    target = release_target()
    destination = (root or cache_root()) / version / target
    cached = _cached_binary(destination, target)
    if cached is not None:
        return cached

    destination.parent.mkdir(parents=True, exist_ok=True)
    lock_path = destination.parent / f".{target}.lock"
    lock_fd = _acquire_lock(lock_path)
    try:
        cached = _cached_binary(destination, target)
        if cached is not None:
            return cached

        asset = archive_name(target)
        base = os.environ.get(
            "AGENT_DOC_RELEASE_BASE_URL",
            f"https://github.com/{REPOSITORY}/releases/download/v{version}",
        ).rstrip("/")
        manifest = _download(f"{base}/SHA256SUMS", MAX_MANIFEST_BYTES)
        expected_digest = _manifest_digest(manifest, asset)
        archive = _download(f"{base}/{asset}", MAX_DOWNLOAD_BYTES)
        actual_digest = hashlib.sha256(archive).hexdigest()
        if actual_digest != expected_digest:
            raise BootstrapError(
                f"SHA-256 mismatch for {asset}: expected {expected_digest}, got {actual_digest}"
            )

        staging = Path(tempfile.mkdtemp(prefix=f".{target}.", dir=destination.parent))
        try:
            _extract_archive(archive, asset, target, staging)
            (staging / "SHA256").write_text(actual_digest + "\n", encoding="ascii")
            if destination.exists():
                shutil.rmtree(destination)
            staging.rename(destination)
        finally:
            if staging.exists():
                shutil.rmtree(staging)
    finally:
        os.close(lock_fd)
        try:
            lock_path.unlink()
        except FileNotFoundError:
            pass

    cached = _cached_binary(destination, target)
    if cached is None:
        raise BootstrapError(f"release installation did not produce {destination}")
    return cached


def main() -> int:
    try:
        binary = ensure_installed()
        argv = [str(binary), *sys.argv[1:]]
        if os.name == "nt":
            return subprocess.call(argv)
        os.execv(binary, argv)
    except (BootstrapError, OSError, urllib.error.URLError) as error:
        print(f"agent-doc bootstrap error: {error}", file=sys.stderr)
        return 1
    return 0
