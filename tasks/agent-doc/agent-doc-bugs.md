---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
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

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d236cca996f590233b78ee7610f8c87e138c23d521f032418d6629e816-20260603-061046.md`*

Compacted content:
- Archived 6 response topic(s): Where else can lazily-rs be used; Where else can lazily-rs be used; Would agent-doc benefit from using herdr; 3 more
- Prior summary/context: ### Session Summary *Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/9b6e92d2...
❯ Should I remove all instructions from AGENTS.md? Would the harness be more effective? Do I have context debt?
### Re: Removing AGENTS.md instructions — glm-5.1

**Should you remove all instructions from AGENTS.md?** No. The current AGENTS.md is 160 lines (~20KB) and carries three distinct payloads: agent-doc workflow, tsift code navigation, and project-specific runbook references. These are actively used by the harness every cycle.

**Do you have context debt?** Mild, not critical:
- The agent-doc skill section is the heaviest piece (~12KB of workflow + runbook refs). Since it's already installed as a skill, there's redundancy between AGENTS.md and the skill file. The harness loads both on every turn.
- The tsift code-nav block (~3KB) is useful but duplicates what `tsift status` prints.
- The philosophy block from `~/.claude/CLAUDE.md` is loaded at the user level, not project-level — that's orthogonal.

**What would actually help:**
1. The agent-doc section in AGENTS.md exists because Claude Code reads `AGENTS.md` as a project instruction file. OpenCode does the same. If agent-doc's skill files are already installed and loaded, the AGENTS.md copy is confirmatory rather than load-bearing — but removing it risks a harness that only reads AGENTS.md (not skill files) missing the workflow entirely.
2. The real context debt is the compaction history in this session document. Each compact cycle carries a summary of prior summaries, creating a telescoping reference chain that the model must parse every turn.

**Recommendation:** Keep AGENTS.md as-is. The instructions are load-bearing for harnesses that read AGENTS.md but not skill files. If you want to trim, the tsift block is the safest to remove (it's self-service via `tsift status`).
### Re: Extract AGENTS.md into skills & runbooks — glm-5.1

Done. AGENTS.md reduced from **20,740 bytes (160 lines)** to **1,445 bytes (24 lines)** — a **93% reduction**.

**What changed:**
- Removed the full agent-doc workflow (hot path digest, workflow steps 0–2, pending ops, persist rules) — all of this lives verbatim in `.claude/skills/agent-doc/SKILL.md` (which is actually more complete, including the auto-loop section)
- Removed the tsift code-navigation block (24 lines) — already in `.claude/skills/tsift/SKILL.md` and `runbooks/code-navigation.md`
- Removed the runbooks catalog paragraph — replaced with a one-line pointer to `runbooks/`
- Kept: frontmatter, one-line description, harness-specific invocation list, and 3 pointer lines to skill files + runbooks

**What's preserved:**
- Harnesses that only read AGENTS.md still see the `/agent-doc` trigger and know what subcommands exist
- Full workflow is one click away in `.claude/skills/agent-doc/SKILL.md`
- Code nav is one click away in `.claude/skills/tsift/SKILL.md`
- All runbooks untouched

**Context savings:** ~19KB less instruction text loaded every turn for harnesses that read both AGENTS.md and skill files. The redundancy is eliminated; the authoritative source is now the skill file.
<!-- agent:boundary:1b85b380 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue -->
(queue cleared — cloc response already committed in prior cycle)
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#owned-pane-chat-writeback] [P1 Codex/closeout] Fix direct-chat owned-pane response loss: when a Codex pane already owns an agent-doc document and recursive direct invocation is blocked, a chat-only final response must fail closed or cross the `agent-doc write --commit` / finalize boundary before success. Repro: 2026-06-23 Big Bear species task in `src/boost-client/tasks/monsterrodholders.md` completed and pushed commits, but the final response was not added to the document. Plan: `tasks/agent-doc/plan-direct-chat-owned-pane-response-loss-2026-06-23.md`. Advances `#codex-final-response-not-written` / `#codex-queue-drain-no-response-body`.
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
