# corky — workspace-split crates.io publish

**Status:** DONE 2026-06-08 — user approved claiming the 5 `corky-*` crate names. Lockstep `0.29.0` published to crates.io in dependency order (corky-core → corky-google/social/transcribe → corky-mail → root corky), all six verified live. Tag `v0.29.0` pushed (CI publishes PyPI wheel + GitHub release). `cargo install --features transcribe-cuda` + `make release` rebuilt local binaries; superproject pointer bumped.

## Why it's not a one-shot publish

Since `v0.28.2`, corky was split into a workspace (`feat: split corky into workspace crates`):

```
corky (bin)
├── corky-core
├── corky-google
├── corky-mail
├── corky-social
└── corky-transcribe   (optional, transcribe/diarize features)
```

The root `corky` Cargo.toml depends on each member by `version + path`
(`corky-core = { version = "0.28.2", path = "crates/corky-core" }`, etc.).

crates.io rejects path-only deps, so publishing `corky` now requires **all 5 member
crates published first**, at matching versions, in dependency order. None are on
crates.io yet (all five names are currently **available** — HTTP 404 as of 2026-06-08,
not squatted). This is a first-ever 5-name public namespace claim — irreversible.

DEV.md still lists `publication_targets: crates.io, pypi, github-release`, so crates.io
is still an intended target; the split just broke the single-crate path.

## Open decisions

1. **Claim the 5 `corky-*` crate names on crates.io?** (irreversible namespace grab) — or
   keep corky binary/PyPI-only and mark members `publish = false`?
2. **Version** for the workspace release — recommend lockstep `0.29.0` (minor: workspace
   split + Google Workspace Drive/Sheets/Docs features).
3. Confirm each member crate has crates.io metadata (`description`, `license`,
   `repository`) before publishing — publish fails without it.

## Recommended path (if claiming the names)

1. Add `description`/`license`/`repository` to each member crate missing them.
2. Lockstep-bump root + 5 members to `0.29.0`; change root deps to `version = "0.29.0"`.
3. Run `make check` (with `--features diarize` for the transcribe crate per project rule).
4. `cargo publish` member crates in dependency order: corky-core → corky-mail/google/social/transcribe → root corky.
5. Tag `v0.29.0`, push → CI publishes the PyPI wheel + GitHub release (maturin).
6. `cargo install --path .` + `cargo build --release` (DEV.md post_release).
7. Update VERSIONS.md + keep pyproject.toml in sync, then bump the superproject pointer.

Uncommitted `Cargo.lock` in the working tree is lockfile drift from the committed split;
fold it into the release commit.
