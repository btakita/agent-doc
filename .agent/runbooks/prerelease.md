# Prerelease

Steps to run before publishing a release. Execute all applicable target sections.

## Common (all targets)

1. `make check` (or equivalent test suite)
2. Verify version bumped in manifest (Cargo.toml / package.json / pyproject.toml)
3. Update VERSIONS.md / CHANGELOG.md with new version entry
4. Audit docs: `module-harness audit` (if available)
5. Update ontology: if new domain terms were introduced, verify they're in README.md domain ontology and existence-lang kernel
6. No secrets in the diff or release notes
7. No machine-local paths in released files (all references must resolve within the repo or declared dependencies)
8. **Manual testing gate:** Before entering this gate, rebuild and install: `make build-release && cargo install --path .`. Both must complete. Do not invoke the new behavior until these succeed — you will test the old build otherwise. Then verify the changed behavior end-to-end in a live session. Do NOT publish to crates.io, PyPI, or GitHub Releases until the user has confirmed the feature works.

## Target: cargo (crates.io)

- `cargo publish --dry-run`
- Verify `Cargo.toml` metadata (description, license, repository)

## Target: pypi

- Sync pyproject.toml version with Cargo.toml
- Build manylinux wheel via Docker (mask local `.cargo/config.toml` to use crates.io deps):
  ```bash
  docker run --rm \
    -v <submodule-path>:/io \
    -w /io \
    -v /dev/null:/io/.cargo/config.toml:ro \
    quay.io/pypa/manylinux_2_28_x86_64 \
    bash -c "curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable && source ~/.cargo/env && yum install -y openssl-devel clang && /opt/python/cp312-cp312/bin/pip install maturin && /opt/python/cp312-cp312/bin/maturin build --release --compatibility manylinux_2_28 --no-default-features"
  ```
  **Note:** Use `manylinux_2_28` (not manylinux2014). The old manylinux2014 image is CentOS 7 with OpenSSL 1.0.x — too old for modern Rust crates that need OpenSSL ≥ 1.1.0. If the crate has transcription/bindgen features, `--no-default-features` skips them for the PyPI wheel.
- Upload: `MATURIN_PYPI_TOKEN="$(passage show pypi/publish_token)" maturin upload target/wheels/<wheel>.whl`
- **Never mount the entire workspace** — only mount the specific submodule directory

## Target: npm

- `npm pack --dry-run`
- Verify `package.json` fields (name, version, main/exports)

## Target: github-release

**Default: Let CI handle it.** Push a version tag (`git tag v<version> && git push origin v<version>`) and CI creates the release with cross-platform binaries.

**Local release (edge case):** If CI is unavailable or you need to publish immediately:
- `cargo build --release`
- `gh release create v<version> --generate-notes target/release/<binary>`
- CI workflows have `skip_existing` / `--clobber` so they won't fail if the release already exists

## Target: binary-install

- `cargo install --path .` (update ~/.cargo/bin)
- Verify symlinks (e.g., .bin/agent-doc)
