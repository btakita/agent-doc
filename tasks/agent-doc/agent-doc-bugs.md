---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent_doc_format: template
agent_doc_write: crdt
agent: claude
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
✅ Fixed the frontmatter body-slice bug in `src/agent-doc/src/frontmatter.rs` so repeated frontmatter rewrites no longer accumulate blank lines before the first body heading. Added regression coverage for exact body preservation across repeated parse/write cycles, then verified the crate with `cargo test`, `cargo build --release`, and `cargo install --path .`. The code change is committed and pushed in `src/agent-doc` as `f4b9f59` on `20260422/codex-agent-backend`.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
## Exchange Summary (compacted from ~960 lines)

### Features implemented

1. **Queue orchestration (#queue, Phases 1-3)** — QueueComponent parser + data model, preflight integration + trigger resolution, consumption + dispatch + halt detection. Added `agent:queue` to template scaffold. Prompt preset expansion in orchestrate.
2. **Orphaned socket GC (#gcsock)** — `start.rs` GCs stale supervisor sockets before binding, preventing CRDT corruption from orphaned sessions.
3. **Snapshot auto-migration on rename (#mvsnap)** — state files (snapshots, baselines, captures, CRDT) follow when document path changes.
4. **Snapshot integrity validation (#startval)** — validates snapshot before IPC listener start.
5. **Per-harness model override (#hmodel)** — `claude_model` / `codex_model` frontmatter fields for harness-specific model selection.
6. **Rename pending → backlog (#hbacklog)** — `agent:pending` → `agent:backlog` (alias preserved). `patch=replace` deprecated on backlog tags.
7. **Icebox component (#hicebox)** — `agent:icebox` added to template scaffold.
8. **Migrate command** — `agent-doc migrate` for deprecated component name/attribute migrations.
9. **Pane eviction guard (#pane1)** — `route.rs` skips tmux pane eviction when agent process active.
10. **Route trigger path resolution (#route1)** — trigger paths resolved to absolute, preventing submodule CWD misrouting.
11. **Pending-capture heuristic fix (#4wa1)** — detects unconditional follow-up patterns that false-positive-triggered recommendation batch guard.
12. **Strip guard markers (#npc1)** — `strip_guard_markers` removes `` and `` from within content lines in committed blobs.
13. **Compact --message - stdin support** — `--message -` reads from stdin (standard Unix convention).
14. **Supervisor model injection (v0.33.15)** — `start.rs` injects `--model` from `claude_model`/`codex_model`/`model` frontmatter when not already in agent args.
15. **Post-preflight planning command** — `agent-doc plan <FILE>` emits structured planning record.

### Bugs diagnosed

- **CRDT corruption after file rename** — orphaned sessions continued writing via stale IPC sockets. Root cause: no socket GC + no snapshot migration. Fixed by #gcsock + #mvsnap.
- **Patchback routing failures** — triggers resolved relative to CWD not document root. Fixed by #route1. Pane loss from eviction during active agent. Fixed by #pane1.
- **Compact --message -** — treated `-` as literal string instead of stdin marker. Fixed.

### Releases

- **v0.33.14** — published to crates.io + GitHub Releases (tag on `20260422/codex-agent-backend`)
- **v0.33.15** — version bumped, binaries built locally, waiting on manual testing gate before publish

### Open / gated

- v0.33.15 publish gated on manual testing confirmation
<!-- agent:boundary:3305dd7a -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:pending patch=replace -->
- [/] [#hfix8] Bump JB plugin version, build + install, manual verify (HEAD) appears after patchback with IPC listener active
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:pending -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
