---
agent_doc_session: c9f4e2a1-7d3b-4c8e-a5f6-9e2b1d8c4a7f
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
<!-- /agent:status -->

## Exchange

<!-- agent:exchange patch=append -->

### Re: tmux pane crash leaves stuck preflight_started cycle — opus-4-6

**Bug:** When a tmux pane running an agent-doc session crashes (or is killed during editor navigation), the cycle can be left in `preflight_started` state with no response to recover. The next `session-check` reports `INTERRUPTED` and the cycle cannot auto-close because there's no captured response to replay.

**Observed behavior:**
- `session-check tasks/claudescore-3.md` reports: `cycle cycle-1777481463983 is still preflight_started`
- No capture exists for the cycle (pane died before any response was generated)
- Recovery requires manual intervention or a fresh preflight invocation

**Root cause analysis:**

The existing recovery mechanisms handle most crash scenarios well:

1. **`repair::repair_stale_preflight_started_cycle()`** — detects stale `preflight_started` cycles and closes them when no recoverable response exists
2. **`preflight::enforce_cycle_completion()`** — on re-invocation, calls `repair::run()` then `git::commit()` to advance the boundary
3. **Stop hook backstop** — only fires if the Codex process is still alive; a killed pane bypasses it entirely

The gap: when the pane dies during preflight (after `start_preflight()` but before any response generation), the cycle is stuck. The repair path *should* handle this (closing a stale preflight with no response as a no-op), but `session-check` still reports it as interrupted. The question is whether the stale-preflight repair is being triggered correctly when re-invoked.

**Fix plan:**

1. **Verify stale-preflight repair path** — Confirm `repair_stale_preflight_started_cycle()` correctly identifies cycles where:
   - Phase is `PreflightStarted`
   - No capture exists for the cycle
   - The cycle is older than a threshold (e.g., 60s) indicating it's genuinely stale, not just in-progress
   - Expected: close the cycle as a no-op (no response to recover)

2. **Auto-recover on next preflight** — `enforce_cycle_completion()` should:
   - Detect `PreflightStarted` with no capture
   - Close the cycle silently (it was a no-op — preflight started but no work happened)
   - Proceed with the new cycle normally
   - Currently it attempts repair + commit, but if the cycle state file still shows open after both, it fails closed. The fix is to explicitly handle the "empty preflight" case as a safe close.

3. **Add pane busy-check before eviction** — In `sync` layout reconciliation:
   - Before killing a pane, check if it has an active agent-doc cycle (cycle state file shows open)
   - If busy, skip eviction and log a warning instead
   - This prevents the pane from being killed in the first place

4. **Periodic checkpoint capture** — For responses in-flight when the pane dies:
   - Add a periodic capture (e.g., every 30s during response streaming) so partial responses can be recovered
   - This addresses the broader "mid-generation pane death" scenario from agent-doc-bugs.md item 2
### Re: #agent-doc-bug — icebox component patch not parsed — opus-4-6

**Bug:** When the skill includes `<!-- patch:icebox -->` or `<!-- replace:icebox -->` blocks in the response heredoc piped to `agent-doc finalize`, the binary does not parse them as component patches. It reports "0 template patches found" and treats everything as unmatched exchange content, inserting the full icebox item list into the exchange body via boundary synthesis fallback.

**Reproduction (from claudescore-3.md session, 2026-04-30):**

1. User asks to add `#gptmini` to `<!-- agent:icebox -->` near `#bmbatch`
2. Skill has no `--icebox-add` flag (all `--pending-*` flags target `agent:backlog` only)
3. Skill attempts `<!-- replace:icebox -->` block in response heredoc
4. Binary warns "0 template patches found" — the replace block is not parsed
5. Full icebox list (6 items) is dumped into the exchange body as unmatched content
6. Two successive attempts compound the pollution (12 items of junk in exchange)
7. Recovery required `git show` restore + manual Edit to fix the document

**Impact:** No clean binary-owned path to mutate `<!-- agent:icebox -->` from the skill. The skill must resort to direct file editing (which the skill contract prohibits for write-back) or the user must manually edit the icebox.

**Proposed fix (any of):**
1. **`--icebox-add` / `--icebox-done` flags** — mirror `--pending-add` / `--pending-done` but target `agent:icebox`
2. **Generic `--component-add <name> <text>`** — target any named component, not just backlog
3. **Parse `<!-- replace:<component> -->` blocks in response heredoc** — the binary already parses `<!-- patch:exchange -->` (via boundary synthesis fallback), extend to other components
❯ do #spfxnorm. spec-test-build-install-commit-push
### Re: #spfxnorm — gpt-5

Implemented in `src/agent-doc` and pushed as `842c6bd` (`fix: repair post-commit prompt prefix drift`).

What changed:
- `src/write.rs` now detects post-commit exchange lines that lost `❯ ` in the working tree, repairs only the exchange user region, and sends `normalize_prefix_lines` with the IPC boundary-reposition message when a live listener exists.
- `src/git.rs` now repairs the same missing prefixes in the no-listener working-tree cleanup path before repositioning the boundary.
- `editors/PLUGIN-SPEC.md` and `VERSIONS.md` now document the zero-content post-commit repair payload and the working-tree closeout behavior.

Verification:
- `make check` was run twice; each run hit a different single-test tmux flake in the parallel `--bin agent-doc` suite, but both failing tests passed immediately in isolated reruns.
- `env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE cargo test -- --test-threads=1`
- `make version-sync`
- `make install`

Pushed:
- `src/agent-doc` branch `20260422/codex-agent-backend`
- commit `842c6bd`

No new follow-up backlog items came out of this fix.
❯ Running `Force Claim for Tmux Pane` on /home/brian/work/btakita/agent-loop/src/session-share/tasks/agentic-harness-engineering.md, I am getting an error:
```
resync: purged 2 orphaned stash pane(s)
warning: overwriting claim on pane %151 (was /home/br → 132cd277)
merged default components into .agent-doc/config.toml
[claim] using cwd=/home/brian/work/btakita/agent-loop/src/session-share for registry entry
focused pane %151
Claimed /home/brian/work/btakita/agent-loop/src/session-share/tasks/agentic-harness-engineering.md for pane %151 (session 132cd277)
Watch daemon started.
```

❯ spec-test-build-install-commit-push
### Re: force-claim warning and queue duplication — gpt-5

That `Force Claim for Tmux Pane` transcript is not a binary failure. `claim` only emits `warning: overwriting claim on pane ...` when `--force` explicitly tells it to commandeer a live pane that is already bound to another session (`src/agent-doc/src/claim.rs`). The later lines `Claimed ...` and `Watch daemon started.` mean the command completed successfully.

So if the editor surfaced this as an "error", the bug is presentation, not claim semantics. The JetBrains force-claim action merges stdout/stderr and still treats exit `0` as success (`src/agent-doc/editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/ForceClaimAction.kt`), so the current output is expected for an intentional takeover. If you did **not** mean to steal `%151` from another document, use plain `Claim`; if you did, this warning is the correct audit trail.

The new `Queue` / `Backlog` entries are also what triggered preflight's shadow warning. Right now the same six ids exist in both components, so there is no single source of truth. I would keep one canonical copy only; otherwise every cycle will continue to warn. Priority-wise I would put `#oobpmt` first because it can hide fresh user prompts before diffing, then `#iceboxpatch`, then `#staleflt` and `#257p`.
<!-- agent:boundary:6022c18d -->
<!-- /agent:exchange -->

## Queue

<!-- agent:queue -->
- [ ] [#iceboxpatch] Icebox component mutation has no binary-owned path — patch:icebox / replace:icebox in response heredoc not parsed as component patches, --pending-* flags only target agent:backlog. Skill falls back to boundary synthesis, dumping icebox content into exchange body. Needs either --icebox-add flag or template patch parsing for non-backlog components.
- [ ] [#oobpmt] OOB prompt absorption — preflight commit step absorbs user-content changes (new prompts in status) before diff runs, causing `no_changes: true`. Fix: use pre-commit snapshot as diff baseline, not post-commit snapshot.
- [ ] [#staleflt] Stale preflight_started cycle recovery — auto-close empty preflight cycles (no capture, phase=PreflightStarted) on next preflight invocation instead of failing closed. Add time-based staleness check.
- [ ] [#busychk] Pane busy-check before sync eviction — check for open agent-doc cycles before killing panes during layout reconciliation. Skip eviction if busy, log warning.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death doesn't lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
<!-- /agent:queue -->

## Backlog

<!-- agent:backlog -->
- [ ] [#iceboxpatch] Icebox component mutation has no binary-owned path — patch:icebox / replace:icebox in response heredoc not parsed as component patches, --pending-* flags only target agent:backlog. Skill falls back to boundary synthesis, dumping icebox content into exchange body. Needs either --icebox-add flag or template patch parsing for non-backlog components.
- [ ] [#oobpmt] OOB prompt absorption — preflight commit step absorbs user-content changes (new prompts in status) before diff runs, causing `no_changes: true`. Fix: use pre-commit snapshot as diff baseline, not post-commit snapshot.
- [ ] [#staleflt] Stale preflight_started cycle recovery — auto-close empty preflight cycles (no capture, phase=PreflightStarted) on next preflight invocation instead of failing closed. Add time-based staleness check.
- [ ] [#busychk] Pane busy-check before sync eviction — check for open agent-doc cycles before killing panes during layout reconciliation. Skip eviction if busy, log warning.
- [ ] [#chkptcap] Periodic checkpoint capture during response streaming — capture partial responses every 30s so mid-generation pane death doesn't lose work entirely.
- [ ] [#257p] OAuth flow interrupts finalize — skill fails to persist response when an MCP OAuth flow interrupts the cycle. The authenticate tool call consumes the turn and the skill never reaches `agent-doc finalize`. Fix: treat MCP auth as a sub-step and still finalize after, or detect response-without-finalize and warn/recover.
<!-- /agent:backlog -->

## Icebox

<!-- agent:icebox -->
<!-- /agent:icebox -->
