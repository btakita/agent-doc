#!/usr/bin/env python3
"""Affected-scope developer checks and content-addressed full-check receipts."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
CARGO_WRAPPER = ROOT / "scripts" / "with-cargo-cache"
BATCHED_TEST_PACKAGES = {
    "agent-doc-controller",
    "agent-doc-controller-io",
    "agent-doc-route-io",
    "agent-doc-session-check-io",
    "agent-doc-start-runtime-io",
}
DOC_SURFACES = ("SKILL.md", "AGENTS.md", "README.md", "SPEC.md", "runbooks/", "specs/")
PROOFS = {
    "full-check": ("AGENT_DOC_FULL_CHECK_SUCCEEDED", ".agent-doc-full-check.json"),
    "tmux-ci": ("AGENT_DOC_TMUX_CI_SUCCEEDED", ".agent-doc-tmux-ci.json"),
}


def output(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, stderr=subprocess.DEVNULL).strip()


def changed_paths(base: str) -> list[str]:
    try:
        merge_base = output("git", "merge-base", "HEAD", base)
    except subprocess.CalledProcessError:
        merge_base = "HEAD"
    paths: set[str] = set()
    for args in (
        ("git", "diff", "--name-only", f"{merge_base}...HEAD"),
        ("git", "diff", "--name-only"),
        ("git", "diff", "--cached", "--name-only"),
        ("git", "ls-files", "--others", "--exclude-standard"),
    ):
        text = subprocess.check_output(args, cwd=ROOT, text=True)
        paths.update(line for line in text.splitlines() if line)
    return sorted(paths)


def cargo_metadata() -> dict[str, object]:
    raw = subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1"], cwd=ROOT, text=True
    )
    return json.loads(raw)


def affected_package_names(metadata: dict[str, object], paths: list[str]) -> list[str]:
    packages = metadata["packages"]
    workspace_ids = set(metadata["workspace_members"])
    workspace = [package for package in packages if package["id"] in workspace_ids]
    root_manifest = (ROOT / "Cargo.toml").resolve()
    global_rust = any(
        (ROOT / path).resolve() == root_manifest or path == "Cargo.lock"
        for path in paths
    )
    if global_rust:
        selected = set(workspace_ids)
    else:
        selected: set[str] = set()
        rust_paths = [
            (ROOT / path).resolve()
            for path in paths
            if path.endswith(".rs")
            or path.endswith("Cargo.toml")
            or Path(path).name == "build.rs"
        ]
        package_roots = sorted(
            (
                (Path(package["manifest_path"]).resolve().parent, package["id"])
                for package in workspace
            ),
            key=lambda item: len(item[0].parts),
            reverse=True,
        )
        for source in rust_paths:
            for package_root, package_id in package_roots:
                try:
                    source.relative_to(package_root)
                except ValueError:
                    continue
                selected.add(package_id)
                break

        # Test every workspace package that directly or transitively consumes a
        # changed package. This is conservative affected-scope validation: an API
        # change cannot compile in its leaf crate while silently breaking a caller.
        reverse: dict[str, set[str]] = {package["id"]: set() for package in workspace}
        resolve = metadata.get("resolve") or {}
        for node in resolve.get("nodes", []):
            if node["id"] not in workspace_ids:
                continue
            for dependency in node.get("deps", []):
                dependency_id = dependency["pkg"]
                if dependency_id in workspace_ids:
                    reverse.setdefault(dependency_id, set()).add(node["id"])
        pending = list(selected)
        while pending:
            dependency = pending.pop()
            for consumer in reverse.get(dependency, set()):
                if consumer not in selected:
                    selected.add(consumer)
                    pending.append(consumer)

    names = {package["id"]: package["name"] for package in workspace}
    return sorted(names[package_id] for package_id in selected)


def plan(base: str) -> dict[str, object]:
    paths = changed_paths(base)
    packages = affected_package_names(cargo_metadata(), paths)
    return {
        "base": base,
        "paths": paths,
        "packages": packages,
        "batched_packages": sorted(set(packages) & BATCHED_TEST_PACKAGES),
        "nextest_packages": sorted(set(packages) - BATCHED_TEST_PACKAGES),
        "audit_docs": any(path == prefix or path.startswith(prefix) for path in paths for prefix in DOC_SURFACES),
        "jetbrains": any(path.startswith("editors/jetbrains/") for path in paths),
        "vscode": any(path.startswith("editors/vscode/") for path in paths),
        "python": any(path.startswith("python/") for path in paths),
    }


def run(
    command: list[str],
    env: dict[str, str] | None = None,
    cwd: Path = ROOT,
) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def cargo_command(*args: str) -> list[str]:
    return [str(CARGO_WRAPPER), "cargo", *args]


def package_args(packages: list[str]) -> list[str]:
    return [part for package in packages for part in ("-p", package)]


def run_affected(base: str) -> None:
    current = plan(base)
    print(json.dumps(current, indent=2, sort_keys=True), flush=True)
    packages = current["packages"]
    if packages:
        target_dir = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
        if not target_dir.is_absolute():
            target_dir = ROOT / target_dir
        test_env = os.environ | {
            "AGENT_DOC_BIN": str(target_dir / "debug" / ("agent-doc.exe" if os.name == "nt" else "agent-doc"))
        }
        run(cargo_command("build", "--bin", "agent-doc", "--lib", "--quiet"))
        selected = package_args(packages)
        run(cargo_command("clippy", *selected, "--all-targets", "--all-features", "--", "-D", "warnings"))
        nextest_packages = current["nextest_packages"]
        if nextest_packages:
            args = package_args(nextest_packages)
            if shutil.which("cargo-nextest"):
                run(
                    cargo_command("nextest", "run", *args, "--all-targets", "--profile", "dev-fast"),
                    env=test_env,
                )
            else:
                run(
                    cargo_command("test", *args, "--all-targets", "--quiet", "--", "--test-threads=2"),
                    env=test_env,
                )
        batched = current["batched_packages"]
        if batched:
            run(
                cargo_command("test", *package_args(batched), "--all-targets", "--quiet", "--", "--test-threads=2"),
                env=test_env,
            )
        run(cargo_command("test", *selected, "--doc", "--quiet"), env=test_env)
    else:
        print("check-fast: no affected Rust packages", flush=True)

    if current["audit_docs"]:
        run(cargo_command("run", "--quiet", "--", "audit-docs"))
    if current["jetbrains"]:
        run(
            ["./gradlew", "--no-daemon", "--console=plain", "-q", "test"],
            cwd=ROOT / "editors/jetbrains",
        )
    if current["vscode"]:
        run(["npm", "test"], cwd=ROOT / "editors/vscode")
    if current["python"]:
        run([sys.executable, "-m", "unittest", "discover", "-s", "python/tests", "-v"])


def repository_fingerprint() -> str:
    digest = hashlib.sha256()
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT).split(b"\0")
    untracked = subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=ROOT
    ).split(b"\0")
    target = receipt_path().parent.resolve()
    for raw_path in sorted(path for path in tracked + untracked if path):
        path = ROOT / os.fsdecode(raw_path)
        try:
            path.resolve().relative_to(target)
        except ValueError:
            pass
        else:
            continue
        digest.update(raw_path + b"\0")
        if path.is_symlink():
            digest.update(b"link\0" + os.readlink(path).encode())
        elif path.is_file():
            digest.update(b"file\0" + path.read_bytes())
        else:
            digest.update(b"missing\0")
        digest.update(b"\0")
    # Keep the receipt tied to every external runtime exercised by `check` or
    # `tmux-ci`.  The proof is shared only when both repository bytes and these
    # toolchain identities still match.
    for tool in ("rustc", "cargo", sys.executable, "node", "npm", "java", "lake", "tmux"):
        executable = shutil.which(tool) if tool != sys.executable else sys.executable
        digest.update(tool.encode() + b"\0")
        if executable:
            try:
                version = subprocess.check_output(
                    [executable, "--version"], stderr=subprocess.STDOUT, timeout=10
                )
            except (subprocess.SubprocessError, OSError):
                version = b"version-unavailable"
            digest.update(version)
        else:
            digest.update(b"absent")
        digest.update(b"\0")
    return digest.hexdigest()


def receipt_path(proof: str = "full-check") -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target")
    if not target.is_absolute():
        target = ROOT / target
    return target / PROOFS[proof][1]


def record_proof(proof: str) -> None:
    guard, _ = PROOFS[proof]
    if os.environ.get(guard) != "1":
        target = "check" if proof == "full-check" else proof
        raise SystemExit(
            f"record-{proof} is private to the successful `make {target}` recipe"
        )
    receipt = receipt_path(proof)
    receipt.parent.mkdir(parents=True, exist_ok=True)
    data = {"schema": 2, "proof": proof, "fingerprint": repository_fingerprint()}
    temporary = receipt.with_suffix(".tmp")
    temporary.write_text(json.dumps(data, sort_keys=True) + "\n")
    temporary.replace(receipt)
    print(f"{proof} receipt: {receipt}")


def verify_proof(proof: str) -> bool:
    receipt = receipt_path(proof)
    try:
        data = json.loads(receipt.read_text())
    except (OSError, json.JSONDecodeError):
        print(f"release-check: no reusable {proof} receipt", file=sys.stderr)
        return False
    valid = data == {
        "schema": 2,
        "proof": proof,
        "fingerprint": repository_fingerprint(),
    }
    print(
        f"release-check: reusing content-identical {proof} proof"
        if valid
        else f"release-check: repository/toolchain changed since {proof}",
        file=sys.stderr,
    )
    return valid


def record_full_check() -> None:
    record_proof("full-check")


def verify_full_check() -> bool:
    return verify_proof("full-check")


def record_tmux_ci() -> None:
    record_proof("tmux-ci")


def verify_tmux_ci() -> bool:
    return verify_proof("tmux-ci")


def self_test() -> None:
    fake_root = Path("/workspace")
    metadata = {
        "workspace_members": ["leaf", "middle", "app"],
        "packages": [
            {"id": "leaf", "name": "leaf", "manifest_path": str(fake_root / "leaf/Cargo.toml")},
            {"id": "middle", "name": "middle", "manifest_path": str(fake_root / "middle/Cargo.toml")},
            {"id": "app", "name": "app", "manifest_path": str(fake_root / "Cargo.toml")},
        ],
        "resolve": {"nodes": [
            {"id": "leaf", "deps": []},
            {"id": "middle", "deps": [{"pkg": "leaf"}]},
            {"id": "app", "deps": [{"pkg": "middle"}]},
        ]},
    }
    global ROOT
    real_root = ROOT
    ROOT = fake_root
    try:
        assert affected_package_names(metadata, ["leaf/src/lib.rs"]) == ["app", "leaf", "middle"]
        assert affected_package_names(metadata, ["README.md"]) == []
        assert affected_package_names(metadata, ["Cargo.lock"]) == ["app", "leaf", "middle"]
    finally:
        ROOT = real_root

    with tempfile.TemporaryDirectory() as directory:
        tmp = Path(directory)
        fake_sccache = tmp / "sccache"
        fake_sccache.write_text("#!/bin/sh\nexit 0\n")
        fake_sccache.chmod(0o755)
        capture = tmp / "capture"
        capture.write_text(
            "#!/bin/sh\n"
            "printf '%s\\n' \"${CARGO_TARGET_DIR-}\" \"${RUSTC_WRAPPER-}\" \"${SCCACHE_DIR-}\"\n"
        )
        capture.chmod(0o755)
        env = {key: value for key, value in os.environ.items() if key not in {"CARGO_TARGET_DIR", "RUSTC_WRAPPER", "SCCACHE_DIR"}}
        env["PATH"] = f"{tmp}:{env.get('PATH', '')}"
        values = subprocess.check_output([str(CARGO_WRAPPER), str(capture)], cwd=ROOT, env=env, text=True).splitlines()
        assert values[0] == str(ROOT / "target")
        assert values[1] == str(fake_sccache)
        assert values[2].endswith("/agent-doc/sccache")

        env["RUSTC_WRAPPER"] = "explicit-wrapper"
        values = subprocess.check_output([str(CARGO_WRAPPER), str(capture)], cwd=ROOT, env=env, text=True).splitlines()
        assert values[1] == "explicit-wrapper"
        assert values[2] == ""

    with tempfile.TemporaryDirectory() as directory:
        tmp = Path(directory)
        subprocess.run(["git", "init", "-q"], cwd=tmp, check=True)
        tracked = tmp / "tracked.txt"
        tracked.write_text("first\n")
        subprocess.run(["git", "add", "tracked.txt"], cwd=tmp, check=True)
        real_root = ROOT
        real_target = os.environ.get("CARGO_TARGET_DIR")
        real_full_proof = os.environ.get("AGENT_DOC_FULL_CHECK_SUCCEEDED")
        real_tmux_proof = os.environ.get("AGENT_DOC_TMUX_CI_SUCCEEDED")
        ROOT = tmp
        os.environ["CARGO_TARGET_DIR"] = str(tmp / "target")
        os.environ["AGENT_DOC_FULL_CHECK_SUCCEEDED"] = "1"
        os.environ["AGENT_DOC_TMUX_CI_SUCCEEDED"] = "1"
        try:
            tracked.write_text("second\n")
            record_full_check()
            record_tmux_ci()
            assert verify_full_check(), "an unchanged tree must reuse its proof"
            assert verify_tmux_ci(), "an unchanged tree must reuse its tmux proof"
            fingerprint = repository_fingerprint()
            subprocess.run(["git", "add", "tracked.txt"], cwd=tmp, check=True)
            assert repository_fingerprint() == fingerprint, "index metadata is not repository content"
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=agent-doc self-test",
                    "-c",
                    "user.email=agent-doc@example.invalid",
                    "commit",
                    "-q",
                    "-m",
                    "metadata-only proof",
                ],
                cwd=tmp,
                check=True,
            )
            assert repository_fingerprint() == fingerprint, "commit metadata is not repository content"
            assert verify_full_check(), "staging and committing identical bytes must preserve proof"
            assert verify_tmux_ci(), "tmux proof must survive metadata-only commits"
            tracked.write_text("third\n")
            assert not verify_full_check(), "repository drift must invalidate proof"
            assert not verify_tmux_ci(), "repository drift must invalidate tmux proof"
        finally:
            ROOT = real_root
            if real_target is None:
                os.environ.pop("CARGO_TARGET_DIR", None)
            else:
                os.environ["CARGO_TARGET_DIR"] = real_target
            if real_full_proof is None:
                os.environ.pop("AGENT_DOC_FULL_CHECK_SUCCEEDED", None)
            else:
                os.environ["AGENT_DOC_FULL_CHECK_SUCCEEDED"] = real_full_proof
            if real_tmux_proof is None:
                os.environ.pop("AGENT_DOC_TMUX_CI_SUCCEEDED", None)
            else:
                os.environ["AGENT_DOC_TMUX_CI_SUCCEEDED"] = real_tmux_proof

    makefile = (ROOT / "Makefile").read_text()
    preflight_start = makefile.index("release-preflight: version-sync")
    release_start = makefile.index("release-check: release-preflight", preflight_start)
    preflight = makefile[preflight_start:release_start]
    release_check = makefile[release_start:makefile.index("\nrelease-macos-cadence-check:", release_start)]
    assert "scripts/check_editor_parity.py" in preflight
    assert release_check.index("verify-full-check") < release_check.index("$(MAKE) check")
    assert release_check.index("$(MAKE) check") < release_check.index("verify-tmux-ci")
    assert release_check.index("verify-tmux-ci") < release_check.index("$(MAKE) tmux-ci")
    tmux_start = makefile.index("tmux-ci:")
    tmux_recipe = makefile[tmux_start:makefile.index("\n# Lint", tmux_start)]
    assert "AGENT_DOC_TMUX_CI_SUCCEEDED=1" in tmux_recipe
    assert "record-tmux-ci" in tmux_recipe
    install_start = makefile.index("install-full: editor-generation-bump")
    install_recipe = makefile[install_start:makefile.index("\n# Keep every existing", install_start)]
    assert '--target-dir "$(CARGO_TARGET_DIR_ABS)"' in install_recipe
    assert install_recipe.count('"$(CARGO_TARGET_DIR_ABS)/release/agent-doc"') >= 4
    assert 'CARGO_TARGET_DIR="$(CARGO_TARGET_DIR_ABS)" agent-doc lib-install --profile release' in install_recipe
    assert "target/release/agent-doc" not in install_recipe
    print("dev-check self-test: ok")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=(
            "plan",
            "run",
            "self-test",
            "record-full-check",
            "verify-full-check",
            "record-tmux-ci",
            "verify-tmux-ci",
        ),
    )
    parser.add_argument("--base", default=os.environ.get("AGENT_DOC_CHECK_BASE", "origin/main"))
    args = parser.parse_args()
    if args.command == "plan":
        print(json.dumps(plan(args.base), indent=2, sort_keys=True))
    elif args.command == "run":
        run_affected(args.base)
    elif args.command == "self-test":
        self_test()
    elif args.command == "record-full-check":
        record_full_check()
    elif args.command == "verify-full-check":
        return 0 if verify_full_check() else 1
    elif args.command == "record-tmux-ci":
        record_tmux_ci()
    elif args.command == "verify-tmux-ci":
        return 0 if verify_tmux_ci() else 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
