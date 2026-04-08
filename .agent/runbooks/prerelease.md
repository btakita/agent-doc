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
8. **Manual testing gate:** Install the local build and verify the changed behavior end-to-end. Do NOT publish to crates.io, PyPI, or GitHub Releases until the user has confirmed the feature works in a live session. `cargo install --path .` is fine for local testing; upstream publish waits for explicit approval.

## Target: cargo (crates.io)

- `cargo publish --dry-run`
- Verify `Cargo.toml` metadata (description, license, repository)

## Target: pypi

- Sync pyproject.toml version with Cargo.toml
- Build manylinux wheel via Docker (mask local `.cargo/config.toml` to use crates.io deps):
  ```
  docker run --rm \
    -v <submodule-path>:/io \
    -w /io \
    -v /dev/null:/io/.cargo/config.toml:ro \
    ghcr.io/pyo3/maturin build --release --compatibility manylinux2014
  ```
- Upload: `MATURIN_PYPI_TOKEN="$(pass pypi/publish_token)" maturin upload target/wheels/<wheel>.whl`
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
