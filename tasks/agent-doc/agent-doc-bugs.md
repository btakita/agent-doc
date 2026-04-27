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

Please use agent-doc to migrate the current document
### Re: migrate + pre-commit pending capture gate — opus-4-6

Ran `agent-doc migrate` — document updated to canonical format.

**Phase 1 implemented:** pre-commit pending capture gate in `finalize`. When `pending_capture_guard: strict`, `finalize` now scans the response for uncaptured recommendations *before* committing. If recommendation-like items are detected without `--pending-add` flags, finalize exits non-zero before the commit step — the agent can retry with `--pending-add` without needing a follow-up commit.

Changes:
- `src/session_check.rs` — made `resolve_pending_capture_guard_mode` and `response_text_for_guards` `pub(crate)` for reuse
- `src/write.rs` — added `precommit_pending_capture_check()` between write and commit phases, 5 unit tests
- `tests/finalize_integration.rs` — updated integration test for new pre-commit gate behavior (response written to disk but not committed)
- `specs/07-commands.md` — documented pre-commit gate contract under `finalize`

Tests: 5/5 new pass, full suite clean (autoclaim cascade is pre-existing). Clippy clean. Release built + installed. Committed + pushed.

Both phases added to backlog. Phase 2 (plan emits `pending_mutations`) is gated on Phase 1 validation.

❯ Did you retain `agent:pending` being an alias to `agent:backlog`?
### Re: agent:pending alias retained — opus-4-6

Yes. `agent:pending` is a fully functional alias for `agent:backlog`. The aliasing is handled at two levels:

1. **Component parser** (`component.rs:71-82`): `BACKLOG_ALIAS = "pending"` and `is_backlog_component()` accepts both `"backlog"` and `"pending"`. All pending-related operations (add, done, edit, gate, etc.) use `is_backlog_component()` to find the component, so either tag name works.

2. **Migration** (`migrate.rs`): `agent-doc migrate` rewrites `&lt;!-- agent:pending --&gt;` → `&lt;!-- agent:backlog --&gt;` in the document text for canonicalization, but documents that still use the old tag continue to work without migration.

The `migrate` run on this document renamed the tags to canonical `agent:backlog`, but if you had a document still using `agent:pending` tags, all binary operations would work identically.

❯ Please add both phase 1 and 2 from tasks/agent-doc/plan-pending-capture-precommit.md to backlog
❯ do phase 1. #spec-test-build-install-commit-push
### Re: add plan phases to backlog + do phase 1 — opus-4-6

Phase 1 (pre-commit pending capture gate) is already implemented, spec'd, tested, built, installed, committed, and pushed:

- **Spec**: `specs/07-commands.md` line 439 — documents the pre-commit gate contract under `finalize`
- **Tests**: 5/5 pass in `src/write.rs` (`precommit_pending_capture_tests` module) — covers strict blocks, pending-add passthrough, warn mode, default mode, suppression marker
- **Build**: `cargo build --release` clean
- **Install**: `cargo install --path .` → `~/.cargo/bin/agent-doc` v0.33.15
- **Commit**: `b7e2563 feat: pre-commit pending capture gate in finalize (strict mode)`
- **Push**: `origin/20260422/codex-agent-backend` up to date

Added Phase 1 (`#pcg1`) to backlog and marked done. Phase 2 (`#pcg2`) was already in backlog as gated.

❯ Is phase 2 done?
### Re: phase 2 status — opus-4-6

Partially. The infrastructure is in place but the core heuristic is missing.

**What's done:**
- `pending_mutations` field exists in the `PlanRecord` struct (`plan.rs:47`)
- `PendingMutationPlan` struct with `kind`, `id`, `text` fields
- `ResolveExisting` kind — detects `do #id` in repo actions and matches against open backlog items
- Test: `build_plan_includes_finalize_placeholder_for_template_docs` covers the `do #id` → `ResolveExisting` path

**What's missing (the actual Phase 2 from the plan):**
- `ExpectAdd` variant in `PendingMutationKind` — signals "this prompt will likely generate recommendations"
- Heuristic in `pending_mutations_for_doc()` to detect recommendation-generating prompts: user asks for "tasks", "todo", "backlog", "what's next", "recommendations"
- Tests from the plan: `test_plan_detects_backlog_request`, `test_plan_detects_recommendation_request`, `test_plan_no_false_positive_on_questions`

The `ResolveExisting` kind that exists now is an adjacent feature (resolve `do #id` against pending) rather than the Phase 2 `expect_add` heuristic. `#pcg2` should stay gated.

❯ Please complete #pcg2. spec-test-build-install-commit-push
### Re: complete #pcg2 — opus-4-6

Done. Phase 2 implemented, tested, built, installed, committed, and pushed.

**What was added:**

- `ExpectAdd` variant in `PendingMutationKind` — signals that the upcoming response will likely generate recommendations
- `prompt_requests_backlog_work()` heuristic in `pending_mutations_for_doc()` — detects when prompt targets contain backlog/recommendation signals ("tasks", "todo", "backlog", "what's next", "recommendations", "next steps", "action items", etc.)
- When detected, plan emits `{"kind": "expect_add", "id": "", "text": "user requested backlog/recommendations"}` in `pending_mutations`

**Tests (3 new, all pass):**

- `test_plan_detects_backlog_request` — "add to backlog" prompt → `ExpectAdd`
- `test_plan_detects_recommendation_request` — "What should we do next? Any recommendations?" → `ExpectAdd`
- `test_plan_no_false_positive_on_questions` — "How does the CRDT merge work?" → no `ExpectAdd`

**Spec:** updated `specs/07-commands.md` plan section — documents both `resolve_existing` and `expect_add` mutation kinds.

**Verification:** clippy clean, 5/5 plan tests pass, full suite clean (1 pre-existing failure in codex hook test). Release built + installed. Commit `f67049e`, pushed to `origin/20260422/codex-agent-backend`.

❯ release + publish
### Re: release + publish — opus-4-6

v0.33.15 released and published.

- **VERSIONS.md** updated with all 0.33.15 changes (supervisor model injection, pre-commit pending capture gate, ExpectAdd pending mutations, post-preflight planning command)
- **crates.io**: published
- **GitHub Release**: https://github.com/btakita/agent-doc/releases/tag/v0.33.15
- **Tag**: `v0.33.15` on `20260422/codex-agent-backend`
- **PyPI**: publishes automatically via CI on the GitHub release event
<!-- agent:boundary:0a117b9b -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:backlog -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!--
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
-->
