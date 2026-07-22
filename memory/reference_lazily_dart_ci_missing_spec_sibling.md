---
name: reference_lazily_dart_ci_missing_spec_sibling
description: "lazily-dart CI test job doesn't check out ../lazily-spec, so sibling-only conformance tests fail red as a standing baseline"
metadata: 
  node_type: memory
  type: reference
  originSessionId: 715c0ea7-d98c-40fe-b61e-08fd89f2edeb
---

lazily-dart's GitHub Actions `test` job (`.github/workflows/ci.yml`) checks out
lazily-formal (into `_lazily-formal`) but **not** the sibling `../lazily-spec`.
Any conformance test that reads fixtures only from `../lazily-spec/conformance/...`
therefore fails in CI with `FileSystemException: Cannot resolve symbolic links,
path = '../lazily-spec/conformance/...'` or `Bad state: fixture not found`.
Affected pre-existing tests: `command_conformance` (message-passing),
`statechart_conformance` (statechart), `distributed_conformance`,
`collections_extra_conformance` (textcrdt_delta_sync.json).

This is a **standing baseline** — many consecutive runs are red for this reason,
independent of the pushed change. It does **not** reproduce locally because a
full workspace has `../lazily-spec` checked out. Don't treat a red CI on a
lazily-dart commit as "my change broke it" without checking whether the failures
are these sibling-fixture path errors.

**How to apply:** New conformance tests must **copy their fixtures locally** into
`test/conformance/...` (with a `../lazily-spec` fallback) so they pass in CI —
this is what `collections_conformance`, and the 0.9.0 `materialization_conformance`
+ `transport` tests do (all green in CI). The real fix (out of scope for a
feature+release turn) is either checking out lazily-spec in the `test` job or
giving every conformance test a local fixture copy. Related: the release/publish
path is `dart pub publish` to pub.dev (package name `lazily`) + `vX.Y.Z` tag.
[[reference_lazily_release_targets]]
