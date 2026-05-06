---
agent_doc_session: b26b9957-8de6-4eae-bb98-5d7fe4d6b781
agent_doc_format: template
agent_doc_write: crdt
agent: codex
claude_args: --dangerously-skip-permissions
codex_args: -s danger-full-access
claude_model: claude-opus-4-6
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
  '#code-review': Please review agent-doc codebase. Inspect the architecture and look for edge cases. Isolate concurrency issues and other workflow bugs. Let's find root causes to fix. #follow-up-backlog
  '#commit-push': commit + push all uncommitted files
  '#follow-up-backlog': Any follow-up items to place in the backlog?
  '#next-steps': #follow-up-backlog Please ensure the backlog is in priority order. Print the item id of the top of the backlog.
  "#spec-test-build-install-commit-push": update spec + tests. build + install for local testing. commit + push
  "#spec-test-commit-push": update spec + tests. commit + push
---

## Status

<!-- agent:status patch=replace -->
<!-- normalization -->
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

*Compacted. Content archived to `/home/brian/work/btakita/agent-loop/.agent-doc/archives/483039e7a9b103bd5b68223dac05a4c449ac03899aba53d6022e82100578b5b0-20260506-002651.md`*

Active backlog:
- [ ] [#sgown2] [recommended] Land phase-2 actor store from tasks/agent-doc/plan-single-owner-session-actor-transition.md: durable per-document authoritative record with monotonic generation CAS, while keeping sessions.json as a projection.
- [ ] [#sgown3] [recommended] Land phase-3 supervisor integration from tasks/agent-doc/plan-single-owner-session-actor-transition.md: route supervisor startup/readiness/busy/waiting-input/close transitions through the actor record instead of direct registry/session-log ownership writes.
- [ ] [#sgown4] [recommended] Land phase-4 actor dispatch from tasks/agent-doc/plan-single-owner-session-actor-transition.md: make route dispatch validate actor generation/state and wait on actor acknowledgement instead of normal-path owner election or later document-side inference.
- 2 more active item(s)

Icebox:
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the
❯ do [#sgown2]. spec-test-build-install-commit-push
### Re: #sgown2 — gpt-5

Phase 2 was already implemented in `src/agent-doc` in `3acf5f5` (`feat: add durable session actor store`), so this cycle closed the remaining spec drift instead of rewriting the actor-store code. I updated `specs/08a-session-actor-contract.md` so the opening contract text now matches the shipped `.agent-doc/session-actors.json` phase-2 store.

Verification:
- `cargo test session_actor --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `2b2f801 docs: align session actor phase-2 contract wording`
- `agent-loop`: `6ca870ed2 chore: update agent-doc submodule`

`#sgown2` is complete. Top open backlog item is `#sgown3`.
❯ do [#sgown3]. spec-test-build-install-commit-push
### Re: #sgown3 — gpt-5

Phase 3 was already implemented in `src/agent-doc` across the shipped supervisor/actor commits (`f89ddaa`, `312851e`, and follow-up fixes), so this cycle closed the remaining spec/test drift instead of inventing new ownership logic. I updated `specs/supervisor.md` to reflect the live supervisor stack and added `session_actor` regression coverage that pins same-generation `waiting_input`, `blocked`, and `closed` lifecycle transitions.

Verification:
- `cargo test session_actor --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `ad64d12 docs: align phase-3 supervisor actor status`
- `agent-loop`: `5079c3883 chore: update agent-doc submodule`

Both commits are pushed. `#sgown3` is complete.
❯ do [#sgown4]. spec-test-build-install-commit-push
### Re: #sgown4 — gpt-5

Phase 4 was already implemented in `src/agent-doc` in `312851e` (`feat: route through authoritative actor dispatch`), so this cycle closed the remaining proof/spec gap instead of rewriting the route path. I added a tmux-backed regression that proves authoritative `blocked` and `closed` actor states fail closed without injecting a duplicate reopen into either the actor pane or a stale registered pane, and updated the phase-4 session-actor contract plus `VERSIONS.md` to record the full route-state matrix.

Verification:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test resolve_or_create_pane_fails_closed_for_blocked_or_closed_authoritative_actor --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
  Parallel `cargo test` exposed two existing tmux-suite race failures (`resolve_or_create_pane_restarts_fresh_for_busy_registered_pane_after_noop_fix` and `resolve_or_create_pane_waits_for_busy_restart_handoff_before_retrying_route`), both of which passed on isolated rerun.
- `cargo test --manifest-path src/agent-doc/Cargo.toml -- --test-threads=1`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `cf7ff25 test: close out phase-4 actor dispatch`
- `agent-loop`: `7b00a330f chore: update agent-doc submodule`

`#sgown4` is complete.
❯ do [#sshmeta]. spec-test-build-install-commit-push
### Re: #sshmeta — gpt-5

Implemented the SSH metadata guard in `src/agent-doc` instead of relying on prompt inference. `parse_for_file()` now resolves `required_ssh_targets` from frontmatter or project-local `.agent-doc/config.toml` mappings (`[ssh.docs."..."]`, `[ssh.profiles.<name>]`), and `preflight`, `plan`, and `run` all fail closed when an SSH-dependent doc resolves no targets. The full suite exposed one route handoff regression in the busy-pane fresh-restart path; I fixed that too so a restart handoff now follows the authoritative replacement pane instead of tripping the cross-file dispatch guard.

Verification:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test parse_for_file_resolves_required_ssh_targets_from_project_doc_mapping --manifest-path src/agent-doc/Cargo.toml`
- `cargo test preflight_fails_closed_when_required_ssh_doc_mapping_resolves_no_targets --manifest-path src/agent-doc/Cargo.toml`
- `cargo test resolve_or_create_pane_restarts_fresh_for_busy_registered_pane_after_noop_fix --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `16bed91` (`feat: resolve required ssh metadata from project config`)
- `agent-loop`: `0edc1877f` (`chore: update agent-doc submodule`)

`#sshmeta` is complete.
❯ Please look in agent-doc-bugs5.md backlog. Please transfer any valid items into this document's backlog.
### Re: bugs5 backlog transfer — gpt-5

I reviewed `agent-doc-bugs5.md` and transferred the still-open backlog items into this document in the same priority order: `#oobpmt`, `#staleflt`, `#busychk`, `#chkptcap`, and `#257p`.

I did not transfer `#iceboxpatch` because that fix is already closed there in `src/agent-doc` commit `a2c89a6` (`fix: parse replace icebox patches`). The existing SQLite follow-ups remain below these transferred agent-doc bugs.
❯ do [#oobpmt]. spec-test-build-install-commit-push
### Re: #oobpmt — gpt-5

`#oobpmt` was already fixed in `src/agent-doc`, so this cycle closed stale backlog drift instead of changing code again. The current tree still contains the status guard in `src/git.rs` (`status_mutation_introduces_prompt_work()` / `is_safe_out_of_band_status_mutation()`), the matching preflight regression `preflight_status_prompt_preset_addition_does_not_swallow_diff()`, and the closeout spec text that plain user prompts must remain uncommitted. The landed fixes are `475269b` (`fix: preserve prompt-bearing status edits`) and `8a1c214` (`fix: preserve prompt-prefixed status guidance`).

Verification:
- `cargo test preflight_status_prompt_preset_addition_does_not_swallow_diff --manifest-path src/agent-doc/Cargo.toml`
- `cargo test commit_does_not_absorb_out_of_band_user_prompt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

No additional source/spec/test edits were necessary, so there was nothing new to commit or push this cycle.

`#oobpmt` is complete. Top open backlog item is `#staleflt`.
❯ do [#staleflt]. spec-test-build-install-commit-push
### Re: #staleflt — gpt-5
Implemented in `src/agent-doc` and pushed as `d65498b` (`fix: auto-close stale empty preflight cycles`).

What changed:
- `repair::repair_stale_preflight_started_cycle()` now has a bounded stale-empty-cycle fallback: if a cycle is still `preflight_started`, has no capture for that cycle, shows no visible response patchback, and is older than 60 seconds, repair closes it as a no-op instead of leaving the next preflight stuck behind a fail-closed open cycle.
- Added regressions in `src/repair.rs` and `src/preflight.rs` for both sides of the new contract: stale empty cycles now auto-close, recent empty cycles still stay open.
- Updated the closeout/spec/skill surfaces (`SPEC.md`, `specs/07-closeout-commands.md`, `SKILL.md`, `.claude/skills/agent-doc/SKILL.md`, `CLAUDE.md`, `VERSIONS.md`) so the stale-empty timeout behavior is explicit.

About the startup-miss line you called out: the retry text is the startup-miss diagnostic payload that route constructs when a routed trigger times out before any new document cycle starts. In current `agent-doc`, that diagnostic is emitted via tmux `display-message` in `src/route.rs`, not by pressing Enter on a drafted shell command, so if you saw a literal `echo '[agent-doc] startup-miss: ...'` in the bound Codex pane it was stale prompt text from the earlier pane state rather than something the current route path intentionally left unsubmitted.

Verification:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test recover_repairs_stale_empty_preflight_started_cycle_without_hash_proof --manifest-path src/agent-doc/Cargo.toml`
- `cargo test preflight_closes_stale_empty_preflight_started_cycle_without_hash_proof --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `d65498b` (`fix: auto-close stale empty preflight cycles`)
- `agent-loop`: `c76b63d96` (`chore: update agent-doc submodule`)

`#staleflt` is complete. Top open backlog item is `#busychk`.
❯ do [#busychk]. spec-test-build-install-commit-push
### Re: #busychk — gpt-5

Implemented the sync reconcile guard for open closeout panes instead of relying on a generic busy-process check. `sync.rs` now resolves a pane's registered document, checks for open `preflight_started` / `response_captured` / `write_applied` cycle state, and uses tmux-router's `protect_pane` hook to leave that pane visible with a warning when DETACH would otherwise stash it. I also added regressions for the open-cycle detector and the reconcile replay, and updated the sync contract in `README.md`, `SPEC.md`, `specs/07-session-tmux-commands.md`, and `VERSIONS.md`.

Verification:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test open_cycle_protected_file_state_detects_open_closeout_cycle --manifest-path src/agent-doc/Cargo.toml`
- `cargo test sync_preserves_open_cycle_pane_during_reconcile_detach --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `5ea0283` (`fix: protect open closeout panes during sync`)
- `agent-loop`: `adbe26c07` (`chore: update agent-doc submodule`)

Both commits are pushed. `#busychk` is complete.
❯ do [#ctxacc]. spec-test-build-install-commit-push
### Re: #ctxacc — gpt-5

Implemented the context-accretion guardrails from `tasks/agent-doc/plan-context-accretion-guardrails.md` in `src/agent-doc`. The new `session_accretion` inspector derives bounded churn signals from the exchange body plus existing cycle/session logs, `preflight` now emits that advisory in JSON, and `plan` now fails closed on the hard-stop tier unless the current diff is already taking the explicit compact path. That gives the binary a deterministic compact-or-restart boundary before another expensive turn launches.

Verification:
- `cargo fmt --manifest-path src/agent-doc/Cargo.toml`
- `cargo test --manifest-path src/agent-doc/Cargo.toml`
- `cargo build --manifest-path src/agent-doc/Cargo.toml`
- `cargo install --path src/agent-doc --force`

Commits:
- `src/agent-doc`: `42fb7ed` (`fix: add session accretion guardrails`)
- `agent-loop`: `e8adddccd` (`chore: update agent-doc accretion guardrails`)

Both commits are pushed. `#ctxacc` is complete.
<!-- agent:boundary:2412f360 -->
<!-- /agent:exchange -->
###

<!--
"Press Enter to restart, or 'q' to exit."
-->

## Pending / Not Built

<!-- agent:backlog -->
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death does not lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
- [ ] [#sqliterepros] [recommended] Before prototyping .agent-doc/state.db, capture 2-3 current agent-doc failures where cross-file authority drift or non-atomic state writes would have been fixed by one transactional store rather than by the single-owner actor/supervisor model alone.
- [ ] [#sqliteeval] [recommended] Review plan tasks/agent-doc/plan-sqlite-session-store-concurrency.md against the single-owner actor rollout and decide whether to prototype .agent-doc/state.db as the authoritative session store.
<!-- /agent:backlog -->

<!-- agent:icebox -->
- [ ] [#dyvw] Empty placeholder item — moved to icebox to unblock preflight.
- [ ] [#xthm] Historical malformed duplicate backlog id retained for guard continuity. Canonical replacement:
  `#jberr`.
- [ ] [#tmuxcrash] Closed on April 30, 2026 after the
  `session_end` metadata closeout fix in
  `startup_miss.rs` and the routed-dispatch binding hardening in
  `route.rs`. Final child closed:
  `#tmuxcrash-rn2d`.

<!-- /agent:icebox -->



<!--
I see the following in the binding codex session prompt. Why is it there? Why was it not Entered? spec-test-build-install-commit-push
```
› echo '[agent-doc] startup-miss: routed trigger accepted but no document cycle started for pending prompt_target: do
  #ppct. spec-test-build-install-commit-push. Run '\''agent-doc start tasks/agent-doc/agent-doc-bugs2.md'\'' to retry.'
```
-->
