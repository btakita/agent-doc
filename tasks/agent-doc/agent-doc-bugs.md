---
agent_doc_session: 1ef5f0e7-0687-4f35-b9bc-2b9c330d5688
agent: opencode
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue_active: false
---

## Status

<!-- agent:status patch=replace -->
Prior frontmatter bug fix remains complete. New work this cycle is planning only: added a concrete reliability plan for the “response patchback not committed / no patchback cycle ran” failure family. The current diagnosis is that two bugs can surface as the same symptom: (1) fresh Codex sessions can miss the initial route trigger entirely, so `preflight -> write -> commit` never starts; (2) a write/IPC + snapshot save can succeed, but the cycle can still die before `commit_*`, leaving a visible uncommitted assistant patchback in the working tree.
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260601-032435.md`*

Compacted content:
- Archived 7 response topic(s): Slot/Cell invalidation mid-run; CellHandle::set vs Context::set_cell; lazily-rs integration status; 4 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d2...
❯ Evaluate the agent-doc codebase. Where else can lazily-rs be used to improve performance and reliability?

### Re: Where else can lazily-rs be used — glm-5.1

Identified 14 opportunities beyond the 5 completed phases.

High-impact (5-30 redundant ops/cycle):

1. Doc content + frontmatter dedup in session_check guards - Guard functions each independently read_to_string + frontmatter parse. Single session-check reads/parses same doc 4-6x. Add DocContentCell + FrontmatterSlot.

2. canonicalize + find_project_root + doc_hash triple in snapshot.rs - Path functions each call all three independently, 8-15x per preflight for same file. Add SnapshotPathsSlot depending on existing project_root + canonical_path slots.

3. cycle_state load dedup - Recomputes canonicalize + find_project_root + doc_hash + JSON parse each call, at 5-15 sites per cycle. Add CycleStateSlot.

4. git show_head subprocess dedup - Spawns git subprocess 3-8x per cycle. HEAD unchanged within CLI invocation. Add HeadContentSlot.

5. snapshot load flock dedup - Acquires advisory flock + reads disk 5-12x per cycle. Add SnapshotContentSlot.

6. component parse dedup - Full-document regex scan 10-30x per cycle on identical content. Add ComponentsSlot depending on DocContentCell.

Medium-impact (3-8 redundant ops/cycle):

7. ops_log root walk - ~445 call sites each walk find_project_root. Thread RunContext into log_op.

8. config load global TOML dedup - Global config parsed 2-3x per invocation. Add GlobalConfigSlot.

9. sessions load registry dedup - Sessions JSON parsed 3-8x per route/sync. Add SessionRegistrySlot.

10. verify_snapshot_committed composite dedup - Loads snapshot + HEAD 2-3x per preflight. Add SnapshotCommitStatusSlot.

Low-impact / wiring fixes:

11. template_io.rs - load_component_configs + load_max_lines_configs each walk independently. Wire through Phase 3 slot.

12. patch.rs load_configs - Manual walk duplicating project_root_for_doc. Wire RunContext.

13. harness_detection caching - detect_harness checks 11 env vars 3-5x. Compute once at RunContext creation.

14. agent resolve_for_file - which subprocess 1-3x per cycle. Cache in slot.

Suggested phase ordering: 6 (DocContent+Frontmatter+Components slots) then 7 (SnapshotPaths+CycleState+SnapshotContent) then 8 (HeadContent+SnapshotCommitStatus) then 9 (wire RunContext into ops_log/template_io/patch) then 10 (GlobalConfig+SessionRegistry for ActorContext).
#spec-test-build-install-commit-push

### Re: Where else can lazily-rs be used — glm-5.1

#### What was done

Updated graph.rs with 4 new slot types and path convenience methods for phases 6-10. Added 12 new unit tests. Updated the plan at tasks/agent-doc/plans/lazily-rs-integration.md with phase 6-10 specifications. Fixed clippy warnings in run.rs and watch.rs.

New slots: DocContentCell, FrontmatterSlot, ComponentsSlot, DocHashSlot. New path methods: snapshot_path_for, lock_path_for, baseline_path_for, pending_path_for. Invalidation: invalidate_doc_content cascades to frontmatter and components.

12 new tests all pass. make check: 2102 tests, 0 failures, clippy clean. cargo install succeeds.

Commits: 5dc56ffb (agent-doc), d5e3f7a29 (superproject). Pushed.

Remaining wiring (phases 7-10) threads these slots into session_check, snapshot, cycle_state, git, ops_log, template_io, patch, and harness detection callers.
Would agent-doc benefit from using herdr (https://herdr.dev/compare/, https://github.com/ogulcancelik/herdr) instead of tmux?

### Re: Would agent-doc benefit from using herdr — glm-5.1

Short answer: not in its current form, but the idea of replacing tmux with an agent-aware multiplexer is strategically sound and already partially planned.

#### What herdr offers that agent-doc does not have today

1. **Semantic agent state** (blocked/working/done/idle) — agent-doc currently derives this from session logs, supervisor IPC, and process-tree heuristics. Herdr's built-in detection would collapse some of that inference.

2. **Agent-shaped socket API** (read, send, wait, split, attach) — agent-doc's supervisor IPC + tmux `send-keys`/`capture-pane` surface is essentially a hand-built version of this, spread across `route.rs`, `start.rs`, `sync.rs`, `sessions.rs`, and `tmux-router`.

3. **Direct agent attach** — connecting to one agent's terminal without navigating tmux windows/tabs.

#### Why it does not fit as a tmux replacement today

1. **Depth of integration.** agent-doc has ~2,600+ tmux references across 43+ files. The `tmux-router` crate (`src/tmux-router/`) is already a dedicated abstraction over tmux — `Tmux`, `TmuxBatch`, `IsolatedTmux`, `HybridPaneStrategy`, `PaneInputBackend`, `PaneLifecycleBackend`, `PaneOutputBackend`, `TmuxControlMode`, registry, sync. Replacing tmux underneath means rewriting `tmux-router` entirely, then auditing every caller in route/sync/start/sessions/watch.

2. **Maturity gap.** Herdr is v0.6.6, AGPL-licensed, 682 commits. agent-doc depends on tmux control mode (`-CC`), `pipe-pane`, advisory file locks, `send-keys -H` hex encoding, pane PID introspection, window indexing, session isolation, and break-pane session preservation — a surface area that took months to stabilize against real tmux edge cases. Herdr's socket API is newer and has not been stress-tested against agent-doc's specific failure classes (starting-actor readiness windows, dispatch-proof timing, cross-project stash pane isolation, Kitty keyboard protocol passthrough).

3. **The right abstraction already exists.** `tmux-router` already defines `PaneInputBackend`, `PaneLifecycleBackend`, and `PaneOutputBackend` traits in `hybrid.rs`. The `#multiplexertrait` backlog item planned a `Multiplexer` trait over tmux. That is the correct insertion point for any backend swap — not a hard migration.

4. **Herdr runs *inside* tmux.** Its own docs say it works inside tmux. So adding herdr would layer another multiplexer on top, not replace tmux.

#### What is worth borrowing

- **Herdr's socket API design** is a good reference for simplifying agent-doc's supervisor IPC surface. The current `supervisor::ipc` protocol could converge toward something like `read`/`send`/`wait`/`split`/`attach` as first-class operations.

- **Agent state as a first-class primitive** validates the session-actor model agent-doc already built (specs/08a-session-actor-contract.md). The sidebar/blocked/working/done model maps directly onto agent-doc's controller actor states.

- **The `#multiplexertrait` abstraction boundary** is still the right long-term move. If herdr (or another agent-native multiplexer) matures, the `Multiplexer` trait is the swap point — same way `tmux-router` currently wraps tmux.

#### Recommendation

Keep tmux. Extract the `Multiplexer` trait from `tmux-router`'s existing `hybrid.rs` backend traits when the backlog reaches that item. Watch herdr's socket API and control surface for ideas, but do not add it as a dependency until it can demonstrate stable support for the specific tmux primitives agent-doc requires (control mode, hex sends, pane PID, session-scoped registry, break-pane preservation).
❯ How many LoC are the following projects?
❯ - agent-doc
❯ - tsift
❯ - tagpath
❯ - lazily-rs
❯ - module-harness
❯ - skill-harness
❯ - existence-lang
❯ - any other of my projects in my current agent-loop stack

### Re: LoC across agent-loop projects — glm-5.1

Rust `.rs` lines (excluding `target/`), unless noted:

| Project | LoC |
|---------|-----|
| agent-doc | 224,558 |
| agent-doc-js | 195,969 (TS/JS) |
| session-share | 107,679 |
| tsift | 98,842 |
| tmux-router | 8,627 |
| tagpath | 18,583 |
| lazily-rs | 2,387 |
| agent-kit | 2,304 |
| existence-lang | 1,905 |
| skill-harness | 1,511 |
| module-harness | 1,302 |
| instruction-files | 1,169 |
| agent-runbooks | 1,090 |
| agent-rules | 999 |
| eval-runner | 954 |
| agent-memories | 687 |
| agent-resume | 683 |

**Total: ~669,199 LoC** across all tracked projects.

agent-doc alone is 34% of the stack. The top three (agent-doc, agent-doc-js, session-share) account for 79%.
❯ Please run LoC excluding comments.

### Re: LoC excluding comments — glm-5.1

Using `tokei` for accurate code/comments/blanks breakdown:

| Project | Code | Comments | Blanks | Total |
|---------|-----:|---------:|-------:|------:|
| agent-doc | 194,653 | 5,581 | 17,748 | 217,982 |
| session-share | 57,594 | 4,755 | 4,749 | 67,098 |
| tsift | 92,848 | 350 | 6,045 | 99,243 |
| agent-doc-js | 1,034 | 48 | 131 | 1,213 |
| tagpath | 15,897 | 518 | 1,239 | 17,654 |
| tmux-router | 6,781 | 427 | 764 | 7,972 |
| lazily-rs | 1,769 | 144 | 348 | 2,261 |
| agent-kit | 1,730 | 49 | 278 | 2,057 |
| existence-lang | 1,522 | 58 | 221 | 1,801 |
| skill-harness | 1,309 | 0 | 148 | 1,457 |
| module-harness | 1,114 | 38 | 108 | 1,260 |
| instruction-files | 957 | 22 | 119 | 1,098 |
| agent-runbooks | 1,016 | 3 | 45 | 1,064 |
| agent-rules | 860 | 13 | 101 | 974 |
| eval-runner | 777 | 29 | 123 | 929 |
| agent-memories | 589 | 6 | 62 | 657 |
| agent-resume | 572 | 16 | 60 | 648 |

**Total code (no comments, no blanks): ~380,022 LoC**

Comment ratio is low across the stack: ~12K comment lines out of ~424K total non-blank lines (~2.9%). agent-doc and session-share have the highest comment counts; tsift and tagpath are comment-sparse.
<!-- agent:boundary:b653e6e3 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#lr-content-6] Phase 6: Wire DocContentCell + FrontmatterSlot + ComponentsSlot into session_check guard functions. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-cycle-7] Phase 7: Add CycleStateSlot + SnapshotContentSlot; consolidate snapshot path lookups. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-head-8] Phase 8: Add HeadContentSlot + SnapshotCommitStatusSlot for git show HEAD dedup. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-wire-9] Phase 9: Wire RunContext into ops_log, template_io, patch, harness detection. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [ ] [#lr-actor-10] Phase 10: Add GlobalConfigSlot + SessionRegistrySlot for ActorContext. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->
- 2026-05-31 [#pbc1] Commit-boundary reliability for Codex/IPC sessions: fail closed when fresh-start route trigger never fires, and auto-resume commit when write/snapshot succeeded but commit boundary was interrupted. Plan: `tasks/agent-doc/plans/commit-boundary-reliability-codex-ipc.md`
- 2026-05-31 [#dtmc] scan_text_source parallelized via std::thread::scope in diagnose_cycle
- 2026-05-31 [#editor-buffer-state-ffi-debounce] [recommended] Replace `wait_for_stable_content`'s last-added-line/truncation heuristic with editor-authoritative buffer stability when an IDE plugin is attached. JetBrains and VS Code plugins should report normalized editor state through the FFI bridge to the Session Actor: canonical doc path/session, monotonic editor document version, dirty flag, last edit timestamp, optional save timestamp, optional buffer hash, and ideally current buffer content. Run requests should include the editor version/hash observed at click time so the Session Actor can detect edits after invocation, debounce until the version/hash is stable, and compute the diff against the exact stable editor buffer when available. If no editor state exists, fall back to whole-file content + mtime stability polling. This removes reliance on `extract_last_added_line`, which misses edits above the last inserted line, and reduces disk/snapshot/editor-buffer race classes seen in live IPC/JB/VSCode flows.
- 2026-05-31 [#lr-found-1] Phase 1: Add lazily-rs dependency + define graph types in agent-doc-orchestration/src/graph.rs (RunContext, slot/cell handles, compute closures, unit tests). Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- 2026-05-31 [#lr-dedup-2] Phase 2: Deduplicate project_root_for_doc — consolidate 8+ find_project_root implementations into single graph slot. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- 2026-05-31 [#lr-config-3] Phase 3: Move load_project_for_doc and resolve_ssh_context_inputs into graph slots. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- 2026-05-31 [#lr-fmt-4] Phase 4: Move parse_for_file into graph slot. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
- 2026-05-31 [#lr-actor-5] Phase 5: Wire ActorContext into watch daemon and supervisor with invalidation on file/config change events. Plan: tasks/agent-doc/plans/lazily-rs-integration.md
<!-- /agent:done -->
