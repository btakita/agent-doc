---
agent_doc_session: 7910a62f-ef5f-4f02-9703-dade1f077521
agent: claude
claude_model: opus
opencode_model: GLM-5.1
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix.
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': Any follow-up items to place in the acklog? Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777e65629bf156ef66517a44d0d9c829c7e89b02b868f8ca9d0519d3ddc-20260529-064430.md`*

Compacted content:
- Archived 6 response topic(s): do [#adoc-sqlite-isolation]; do [#adoc-sqlite-seam]; do [#adoc-sqlite-seam]; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/fb8de777...

### Re: do [#adoc-orch-shim-cleanup] — opus-4-7

Finished the `agent-doc-orchestration` extraction's final cleanup pass — the step `plan-agent-doc-orchestration-extraction.md:130` deferred until last ("rewrites call sites to `agent_doc_orchestration::`"). Confirmed scope with you first since it measured larger than "polish": **360 `crate::X` bin call sites across 24 files + 14 grouped imports + 92 bare refs in `main.rs`**, not just the ~8 lib shims.

**What changed (pure mechanical refactor, no behavior change):**
- **`lib.rs`** — deleted the 8 `pub use agent_doc_orchestration::{debounce, ipc_socket, merge, project_config_io, secret_redact, security, frontmatter_io, template_io}` shims. Lib-internal callers (`ffi`, `frontmatter`, `project_config`, `template`) now name `agent_doc_orchestration::` directly.
- **`main.rs`** — deleted the ~53 `pub(crate) use agent_doc_orchestration::*` re-exports plus the lib-routed `pub(crate) use agent_doc::ipc_socket`; rewrote bare + `crate::` bin call sites to `agent_doc_orchestration::`. Kept the genuine lib re-exports (`component`, `crdt`, `frontmatter`, `template`).
- **14 grouped `use crate::{…}` blocks** — split so orchestration members import from `agent_doc_orchestration` while retained lib/bin-local modules (`component`, `frontmatter`, `parallel`, `queue_dispatch`, `plan`, `PatchMode`, `AgentDocMode`, …) stay `crate::`.
- **Integration tests** (`debounce_gaps`, `equityfundingsource_ipc_replay`, `ipc_closeout_model`) repointed to `agent_doc_orchestration::{debounce, ipc_socket}`.

Net **36 files, +520/−579**. The 3 unrelated pre-existing WIP files in the tree (`model_tier.rs`, `preflight.rs`, installed `SKILL.md`) were left out of the commit.

**Verification — `make check` green:**
- `cargo clippy -- -D warnings`: clean
- workspace tests: **0 failed** (incl. 1854 orchestration tests + the IPC/debounce integration tests)
- `sim-medium`: 1 passed · `version-sync`: ok · doc tests: ok
- Built both binaries: `cargo build --release` + `cargo install --path .` (v0.34.0).

Committed in the submodule (`ff8cb811`, pushed to `20260422/codex-agent-backend`) and the superproject pointer (`b4f733f48`). The migration is now complete — no shims remain; module provenance is explicit at every call site.
<!-- agent:boundary:def6d66a -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/open-code.done.md -->

<!-- completed work archived in tasks/agent-doc/open-code.done.md -->
<!-- /agent:done -->
