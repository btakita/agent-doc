---
name: reference_agent_doc_034x_publish_reality
description: agent-doc 0.34.x is NOT published to crates.io or GitHub Releases — distribution is local cargo install + lib-install; the Release workflow is structurally broken.
metadata: 
  node_type: memory
  type: reference
  originSessionId: 7a673895-0253-4fb2-b352-ddd58629579d
---

For the agent-doc 0.34.x series, the only working distribution channel is **local
`cargo install --path .` + `cargo build --release` + `agent-doc lib-install`** (which
the tmux-spawned route-owned supervisors hot-reload at their next idle boundary via
`AGENT_DOC_RECYCLE_ON_INSTALL`). Do NOT treat crates.io or a GitHub Release as a
required release step for agent-doc in this repo:

- **crates.io publish is structurally impossible**: the workspace member deps in
  `src/agent-doc/Cargo.toml` (`agent-doc-core`, `agent-doc-markdown-ast`,
  `agent-doc-orchestration`) are declared `path = "..."` with **no `version =`**, so
  `cargo publish` can't resolve them from the registry. agent-doc is a path-only
  workspace, never published to crates.io.
- **The GitHub `Release` workflow has failed on EVERY tag since ~v0.34.0** (verified
  through v0.34.24/.25/.26). Root cause: `agent-doc-orchestration` depends on
  `agent-kit` via `../../agent-kit`, a SIBLING repo in the agent-loop superproject
  that is absent from the standalone `github.com/btakita/agent-doc` CI checkout
  (`failed to read .../agent-kit/Cargo.toml`). The last successful GitHub Release is
  **v0.33.16** (2026-04-27); no 0.34.x version has one. This is pre-existing infra,
  NOT a per-release regression — don't re-investigate it each cycle.

So a "full release" of agent-doc here = version bump (Cargo.toml + pyproject.toml) +
VERSIONS.md + make check + commit/push + tag/push + `cargo install` + `cargo build
--release` + `lib-install` (+ boundary auto-recycle). Fixing the Release workflow
(check out agent-kit as a sibling, or vendor it) is a separate, out-of-scope task.
Relates to [[feedback_cargo_install_on_release]] and
[[feedback_cargo_build_release_after_route_edit]].
