# Pre-Release Verification Checklist

Before starting any release, verify ALL of the following. Block the release if any fail.

| Check | How |
|-------|-----|
| Tests pass | `make check` or equivalent |
| Audit docs pass | `make precommit` (if available) |
| README.md is current | Review README against current features/API. Flag outdated sections. |
| VERSIONS.md is current | Verify the new version has an entry. If not, draft one from `git log` since last tag. |
| Tests cover the change | For bug fixes: verify a regression test exists. For features: verify basic coverage. |
| SPEC.md is current | If a SPEC.md exists, verify it reflects current architecture. Flag drift. |
| No architecture-docs drift | Compare CLAUDE.md, README.md, and SPEC.md against actual code structure. Flag inconsistencies. |
| Version bump is done | `Cargo.toml` (and `pyproject.toml` for Maturin projects) have the new version |
| No dirty working tree | `git status` is clean in the submodule |
