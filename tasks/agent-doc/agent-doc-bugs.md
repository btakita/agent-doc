---
agent_doc_session: 3f069c68-7119-4c7b-84a9-4baeb84be35c
resume:
  claude: e5a9d696-86fe-48f0-b162-4303a2c4670b
agent: claude
claude_model: opus
agent_doc_format: template
agent_doc_write: crdt
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
prompt_presets:
  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push
  '#spec-test-commit-push': update spec + tests. commit + push
queue: stop
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

### Re: Is #owned-pane-chat-writeback still relevant or out? — opus-5

**Out.** The work landed. Marked `--done` this cycle.

I checked each acceptance criterion in `tasks/agent-doc/plan-direct-chat-owned-pane-response-loss-2026-06-23.md` against the current `agent-doc` tree (v0.35.275) and ran the covering tests rather than trusting greps:

| Acceptance criterion | Implementation | Test (green) |
|---|---|---|
| Direct-chat answer cannot reach final success while the document lacks a matching response | `agent-doc-codex-stop-io/src/lib.rs` — `active_session_prompt_requires_writeback` / `prompt_writeback_debt` | `stop_blocks_direct_chat_manual_queue_response_after_recursive_guard` |
| Consecutive direct-chat answers each checked independently | same, keyed per `turn_id` | `stop_blocks_consecutive_direct_chat_manual_queue_answers` |
| Final gate separates chat-only from binary-owned closeout, prints one recovery command | `agent-doc-session-check-io/src/command.rs:1437` (`codex-final-gate`) | `codex_final_gate_blocks_on_recursive_invocation_without_captured_response` |
| `session-check` catches `no-free-text-queue-head-guard` + bare `###` residue | `agent-doc-workflow/src/session_check.rs:358`, `agent-doc-queue/src/queue_closeout_guard.rs:233` | `free_text_queue_head_provenance_decision_reports_bare_heading_residue` |
| Recovery after a successful write is a no-op | `agent-doc-session-check-io/src/command.rs:1423` — idempotent manual-patchback adoption | `codex_final_gate_adopts_manual_patchback_when_response_is_visible` |

All 5 pass. The Stop-hook block message names `agent-doc finalize`, `agent-doc write --commit`, and `agent-doc session-check`, and asserts on the live prompt text (`2300 words`) — so the diagnostic carries the actual unresolved prompt, not a generic warning.

**Two things I also cleaned up:**

- Updated the plan file with a Status header, checked boxes, and the criterion-to-test verification table. Committed as `44aab90`.
- Ran `agent-doc migrate` on this document to clear the `legacy_gated_in_backlog` warning — `#ha3h` moved from `agent:backlog` into `agent:review`, where a gated `[/]` item belongs. It stays gated legitimately: upstream issue #40 says not to ship recipient-side policy config before multi-user workspace support exists.

**One caveat on scope.** The enforcement is Codex-specific — it rides the Codex `Stop` hook (`agent-doc-codex-stop-io`) plus the `session-check` `codex-final-gate`. That matches the original repro, which was a Codex pane. OpenCode has neither a `Stop` hook nor `/loop`, so its equivalent protection is instruction-level (`#ocdrainstall`), not code-enforced. If you ever see this failure family from an OpenCode owned pane, that is a genuinely new item, not a reopen of this one.

### Re: agent-doc in a Coder workspace (JB Toolbox + VS Code) — opus-5

> **Queue prompt:**
>
> do [#createplanbacklog]


Plan written: **`tasks/agent-doc/plan-coder-ide-hosted-tmux.md`** (committed `74c9526`), with phases `#coder1`..`#coder7` now in backlog. Planning only — no code changed.

**The good news first.** I expected the hard part to be transport, and it isn't. In both Coder modes the plugin's process runs *on the workspace*: JetBrains Toolbox/Gateway starts a remote IDE **backend** there and the local window is a thin client; VS Code Remote-SSH / code-server runs the **extension host** there. agent-doc's editor endpoints are unix sockets at `<project_root>/.agent-doc/agent-doc-<pid>.sock` (`agent-doc-ipc-io/src/lib.rs:165`), so plugin and binary are same-host, same-user, same filesystem. CRDT replicas, reliable-sync registrations, endpoint fan-out — all unchanged. No new transport, no SSH tunnel.

**What actually breaks** is only the bootstrap:

1. `ensure_inside_tmux` (`agent-doc-start-io/src/lib.rs:934`) hard-bails with "start a tmux session first" — and in Coder there is nowhere to type that except the IDE terminal, which nothing mentions.
2. `agent-doc terminal` spawns a **GUI terminal emulator** (`src/terminal.rs`, `[terminal] command` / `$TERMINAL`). A workspace container has no `$DISPLAY` and no emulator installed, so the one bootstrap affordance for plugin users is dead there.
3. Neither plugin can open a terminal. I grepped: the JB plugin never touches `TerminalToolWindowManager`/`ShellTerminalWidget`, and the VS Code extension never calls `window.createTerminal`/`sendText`. There is no IDE-hosted shell path at all today.
4. Workspace stop/start kills the tmux server while `.agent-doc/` registry rows and `*.sock` files survive on the persistent volume. Coder autostops aggressively, so that is the normal case there, not an edge case.

The primitive we need already exists: `tmux_router::Tmux::new_session` (`src/tmux-router/src/tmux.rs:422`) runs `tmux new-session -d`, so the binary can stand up a server with no TTY of its own. Pane creation inside an existing server is already controller-owned (`editors/SPEC.md:25`).

**Your four questions.**

**Auto-start or require the operator?** Auto-start by default, with an explicit opt-out that shows the dialog. The primitive is idempotent and the registry already dedupes, so auto-start is re-entrant and costs nothing; requiring a manual `tmux new-session` on every workspace start is friction that will silently strand routes, and a Coder workspace restarts far more often than a laptop. The dialog is still needed for the three cases auto-start can't fix: tmux missing from the workspace image (where the fix is an image/`coder_script` change, not `apt install` — the operator may have no sudo), `auto_start_tmux = false`, and auto-start succeeding with no terminal API to attach a client.

**If we integrate with the IDE's remote, should we also support the IDE's terminal?** They're the same surface — there's no second thing to build. The "remote tab" *is* the terminal tool window, and because the backend/extension host runs on the workspace, `createTerminal` already lands on the workspace shell. `vscode.env.remoteName` and the JB remote flag change what we print in diagnostics, not which API we call.

**IDE-hosted tmux locally, or wait?** Ship it now, in the same change — the plugin terminal API can't tell local from remote, so excluding local means *adding* a guard for less capability. Your separate-shell workflow is protected by resolution order, not by a local/remote switch: **a live registry session for the project always wins** — attach, never create a second one (`src/terminal.rs` `find_active_project_session` already does this; it stays rule 1). Host selection only runs on a cold start with no session, which is exactly the Coder case. Locally your external shell keeps ownership as today.

**How to configure it?** Extend the existing `[terminal]` table (`agent-doc-config/src/lib.rs:114`) rather than adding a second surface:

```toml
[terminal]
host = "auto"                                  # auto | external | ide | none
command = "wezterm start -- {tmux_command}"    # external only; unchanged
auto_start_tmux = true                         # false => dialog instead
session = "agent-doc"
attach_command = "tmux new-session -A -s {session}"
```

`auto` resolves: live attached session → no-op; live detached → attach in place; IDE-hosted env **and** a registered plugin endpoint → `ide`; `command`/`$TERMINAL` → `external`; else dialog. Per-document override as `terminal_host:` frontmatter, so one workspace can mix an IDE-hosted document with an external-shell one.

**One thing I'd flag.** Phase 1's detection signals — the exact `CODER_*` variable names, the JetBrains remote-backend probe, and the `vscode.env.remoteName` values — are asserted from general knowledge here, not read out of a live workspace. Everything downstream keys off that classification, so `#coder1` verifies them against a real Coder workspace before anything else lands. `#coder7` is tagged `[operator-verify]` because the integration matrix (both IDEs attached at once, stop/start mid-turn, an image without tmux) can't be proven offline.

### Re: A command to associate the tmux session with the project — opus-5

> **Queue prompt:** Should we have an agent-doc command that associates the tmux session with the agent-doc project? If yes, this should work both locally and remotely (e.g. Coder environments).

**Yes — and it already exists.** I went looking to design one and found the whole binding is already built:

- `agent-doc session` — show the configured session, or `(auto-detect)`
- `agent-doc session set <name>` — pin it, migrate the `agent-doc` + `stash` windows via `tmux move-window`, and close a superseded orphan session
- `agent-doc session clear` — back to auto-detect

It persists as `tmux_session` in `<project_root>/.agent-doc/config.toml` (`agent-doc-project-config-io`), and `start` self-heals it: `rebind_project_tmux_session_if_expected_dead` (`agent-doc-start-io/src/lib.rs:1130`) rewrites the binding when the configured session turns out to be dead. In this repo it currently reads `0`.

**Local and remote need no extra scoping.** `.agent-doc/` is gitignored, so your laptop checkout and a Coder workspace checkout each carry their own binding — and since tmux runs on the cloud instance, the workspace's own `config.toml` is what governs it there. Nothing to sync, nothing to namespace.

**But one path ignored it, and it was exactly the path Coder needs.** `resolve_session_name` in `src/terminal.rs` hardcoded `"0"` whenever no live pane existed. Its doc comment says "`tmux_session` frontmatter is deprecated and no longer consulted" — true, but when the frontmatter field was retired the *project config* replacement never got wired into this function. So the binding governed the sync/start paths and was silently dropped by the bootstrap path, which is the only one available on a cold workspace where no tmux server is running and the registry scan has nothing to derive from.

Fixed and landed (`35efb28`, submodule pointer `6dfdff7`). Precedence is now:

1. explicit `--session` flag
2. project `tmux_session` binding
3. `"0"`

A live registry pane still outranks all three — `resolve_target_session` scans the registry first, so your separate-shell session keeps ownership exactly as before. Frontmatter stays deprecated and unconsulted; the unused `_file` parameter is what makes that structurally true.

I refactored the precedence into a pure `resolve_session_name_from(explicit, configured)` and added two regression tests (`resolve_session_name_prefers_project_binding`, `resolve_session_name_explicit_beats_project_binding`) alongside the existing two. Full suite: **9020 passed, 253 skipped** (`make check` — clippy, nextest, sim-medium, version-sync, audit-docs, dev-harness-test, lean, TLA+ all green). Built and installed; latest CI on `main` is green.

**One correction to yesterday's plan.** I'd proposed a `[terminal] session` config key — that would have been a second, competing binding. Dropped it; `#coder5` now says to reuse `tmux_session` and `agent-doc session set` instead. The plan file has a new "Session→project association" section recording what exists, so Phases 2 and 5 wire *into* that command rather than around it.

The remaining Coder gap is unchanged and still real: `session set` tells agent-doc *which* session to use, but on a cold workspace nothing creates or attaches it. That is `#coder2` (headless bootstrap) and `#coder3`/`#coder4` (IDE terminal attach).
<!-- agent:boundary:1b85b380 -->
<!-- /agent:exchange -->

<!--
-->

## Pending / Not Built

<!-- agent:queue priority go -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#retainconv] respond/finalize retained a capture as "editor state projection has not converged" while the SAME cycle logged live_editors=1 delivery_converged=true delivery_version=14, and the visible write plus all tracked-work mutations had already landed on disk (19404 bytes == crdt hash 398373016fd0). A bare `agent-doc commit` cleared it instantly with "repaired committed historical committed_capture snapshot drift". Observed 2026-08-23, cycle-1787510294962, agent-doc 0.35.275, JB idea pid 2077735 live, no replica storm (6 deregisters in 4000 log lines). Surviving shape of the needs_operator latch class fixed in a739c20b6: make the retention decision read the same delivery_converged signal the ops log reports, or self-resume via the commit path instead of latching.
- [ ] [#coder1] Phase 1 — terminal-host environment classification (CODER_* / DISPLAY / SSH_CONNECTION / tmux presence) + `agent-doc env --json`. VERIFY the real Coder env var names, the JB remote-backend probe, and vscode.env.remoteName against a live workspace before landing. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder2] Phase 2 — headless tmux bootstrap: `agent-doc tmux ensure <FILE>` (idempotent detached new-session) and rewrite ensure_inside_tmux (agent-doc-start-io/src/lib.rs:934) to auto-start + re-exec into the pane instead of bailing. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder3] Phase 3 — JetBrains IDE-hosted terminal: open/reuse an `agent-doc` tab via TerminalToolWindowManager, run the attach command, dialog fallback with a Copy action. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder4] Phase 4 — VS Code IDE-hosted terminal: window.createTerminal/sendText parity, reuse live terminals, report vscode.env.remoteName in diagnostics, update editors/plugin-parity.tsv. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder5] Phase 5 — `[terminal] host/auto_start_tmux/attach_command` config + `terminal_host:` frontmatter override + pure resolution function shared by CLI and both plugins + runbooks/coder-workspace.md. Do NOT add a `[terminal] session` key — reuse the existing project `tmux_session` binding owned by `agent-doc session set`. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder6] Phase 6 — workspace restart resilience: record tmux server identity, GC stale registry rows + orphaned .sock files on server-identity mismatch, guard AF_UNIX-incapable project-root filesystems. Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
- [ ] [#coder7] [operator-verify] Phase 7 — live Coder workspace validation matrix (JB Gateway cold start, VS Code Remote-SSH cold start, both IDEs attached at once, workspace stop/start, image without tmux). Plan: tasks/agent-doc/plan-coder-ide-hosted-tmux.md
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#ha3h] [MULTI-USER FOLLOW-UP] Recipient-side policy config: .agent-doc/config.toml → receive.auto_create_components = ["pending"] (or = false). Flip notify/write default from auto-create to consult-recipient-config. Single-author workspaces set list once; multi-user recipients gate what senders materialize. Blocked until multi-user workspace support exists; issue #40 explicitly says not to ship this before that prerequisite. Tracked: https://github.com/btakita/agent-doc/issues/40
<!-- /agent:review -->

<!-- agent:icebox -->
- [ ] [#pdar] [recommended] Evaluate optional binary auto-resolution only for the narrow case where the prompt is explicitly do #id and there is exactly one matching open pending item
<!-- /agent:icebox -->

## Completed / Reaped

<!-- agent:done archive=tasks/agent-doc/agent-doc-bugs.done.md -->

<!-- completed work archived in tasks/agent-doc/agent-doc-bugs.done.md -->
<!-- /agent:done -->
