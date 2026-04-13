# Release: Rust Library

Publication targets: crates.io only

## Steps

1. Run pre-release checklist (see `runbooks/pre-release-checklist.md`)
2. Commit and tag from within the submodule
3. Push: `git push && git push origin v<version>`
4. Publish: `CARGO_REGISTRY_TOKEN="$(passage btak/CARGO_REGISTRY_TOKEN)" cargo publish`
5. Verify: `cargo search <crate>`
6. Update parent submodule pointer
