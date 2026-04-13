# Release: Rust + Maturin

Publication targets: crates.io + PyPI + GitHub Release + binary install

## Steps

1. Run pre-release checklist (see `runbooks/pre-release-checklist.md`)
2. Verify `Cargo.toml` and `pyproject.toml` versions match
3. Commit from within the submodule (never from parent)
4. Tag: `git tag v<version>`
5. Push: `git push && git push origin v<version>`
6. Publish to crates.io: `CARGO_REGISTRY_TOKEN="$(passage btak/CARGO_REGISTRY_TOKEN)" cargo publish`
7. Publish to PyPI: `MATURIN_PYPI_TOKEN="$(passage pypi/publish_token)" maturin publish --no-sdist`
8. Create GitHub Release: `gh release create v<version> --generate-notes` with prebuilt binary
9. Rebuild local binary: `make release` or `cargo build --release`
10. Install to cargo bin: `cargo install --path .`
11. **Verify each target:**
    - crates.io: `cargo search <crate>` shows new version
    - PyPI: `pip index versions <package>` shows new version
    - GitHub: `gh release view v<version>` exists
12. Update parent submodule pointer: `cd ../.. && git add src/<submodule> && git commit -m "update <submodule> to v<version>"`

## Dependency Ordering

Some submodules depend on others. Publish in order:
```
instruction-files -> agent-doc -> corky
```
If releasing a dependency, check if dependents need a version bump too.
