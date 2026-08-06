---
name: reference_lazily_v2_release_2026_07_22
description: "lazily v2 shipped 2026-07-22 across all 8 bindings — per-binding published versions, the kt 0.35.0 batch-flush race and its fix, and the corrected publish mechanism for dart vs js"
metadata: 
  node_type: memory
  type: reference
  originSessionId: c54d12a1-95e4-49a0-b66d-0178f0a3a3d8
  modified: 2026-07-25T18:08:01.401Z
---

**lazily v2 RELEASED 2026-07-22 — all 8 bindings published.**

| binding | version | target |
|---|---|---|
| rs | `lazily 0.47.0` + `lazily-macros 0.23.0` | crates.io |
| go | v0.21.0 | GitHub release |
| zig | v0.29.0 | GitHub release |
| cpp | v0.23.0 | GitHub release |
| kt | **0.35.1** | GH Packages |
| py | **0.35.0** (FULL ambient removal) | PyPI, tag → wheels.yml |
| js | 0.26.0 | npm |
| dart | 0.25.0 | pub.dev |

spec + formal pushed; superproject pointers all current.

**kt 0.35.1 exists because 0.35.0 had a CI-reproducible AsyncContext batch-flush race.**
The real library fix was a `flushSuppressed` guard that coalesces the batch flush to
once at the boundary, plus a `ConcurrentHashMap` node-map in the harness. Do not treat
it as a harness-only flake.

**Publish-mechanism correction:** dart is NOT 2FA-blocked. It is a manual
`dart pub publish --force` against cached OAuth in `~/.config/dart/pub-credentials.json`;
no pub.dev CI workflow exists. Only **js npm** has real 2FA (auth-and-writes) plus the
OIDC-E404 `release.yml`.

Related: [[reference_lazily_release_targets]], [[project_lazily_memo_rename_plan_status]],
[[reference_lazily_js_npm_trusted_publisher_e404]],
[[project_lazily_py_ambient_removal_blocked_on_bare_reads]].
