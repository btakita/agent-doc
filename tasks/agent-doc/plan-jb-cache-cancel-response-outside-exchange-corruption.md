# Plan — denied File Cache Conflict → response writes outside agent:exchange (base-level duplicate corruption)

**Backlog:** `#jb-cache-cancel-response-outside-exchange` in `tasks/agent-doc/agent-doc-bugs2.md`
**Status:** Draft — needs the corrupted-document artifact (or a faithful divergence repro) before a guard change; the operator already manually fixed the live document.
**Reported:** 2026-06-05 — *"A equityfundingsource.md queue task response wrote outside of the agent:exchange area into the base document level and the document was corrupted with duplicate content. I did deny a JB `File Cache Conflict` by accident. I manually fixed the document."*

## Signature

- Trigger: operator **Cancel/deny** ("Keep memory changes") on a JetBrains **File Cache Conflict** dialog (here "by accident").
- Result: a response (queue-task cycle) landed **outside** `<!-- agent:exchange -->`, at **base document level**, and the document gained **duplicate content**.
- Operator manually repaired the live document; no corrupted artifact captured.

## How this differs from already-covered cases

`runbooks/jb-cache-conflict.md` + existing SimWorld scenarios cover:

- **Cancel → clean auto-recovery** (`jb_cache_conflict_cancel_auto_recovery_*`): binary-owned write had already applied the response inside exchange; preflight re-commits. No corruption.
- **Late-Accept replay** (`#jbccacceptdup`): a duplicate `### Re:` block **inside** exchange from accepting after the IPC ack window.

Neither covers **Cancel → response content placed OUTSIDE exchange at base level + duplicate**. That is a new, higher-severity corruption class (structure damage, not just an in-component duplicate).

## Code finding — the out-of-exchange guard has a hole under malformed exchange

`agent_doc_core::template::guard_no_conversation_tail_outside_exchange` (core `template.rs:614`) is the write-path fail-closed guard for conversation content escaping the exchange block. Two gaps make it ineffective for this signature:

1. **Missing/unparseable exchange → no-op.** Lines 616-618: if `component::parse` does not find an `exchange` component, the guard `return Ok(())`. A denied File Cache Conflict can leave the buffer/disk with **malformed or duplicate** `agent:exchange` markers (IDE kept a stale memory buffer; a deferred IPC patch later applies against it). If the exchange opener/closer no longer parses as a single clean component, the guard silently passes and content can land at base level.
2. **Only checks *after* the close tag.** `escaped_conversation_range_outside_exchange` filters components with `open_start >= exchange.close_end`. Content corrupted to sit **between duplicate exchange openers**, or above the first opener, is not in that range — so it is not flagged.

`repair_conversation_tail_outside_exchange` (core `template.rs:639`) and `repair_duplicate_exchange_opener_*` exist, but: the repair also early-returns when `exchange` is missing, and `tail_is_safe_exchange_content` bails on "ambiguous" structure — so a genuinely corrupted duplicate-content shape is left for manual fixing (which is what the operator had to do).

## Plugin-side angle

Per the runbook Cancel branch: the plugin returns a failed ack and **leaves file-IPC patch files in place**. A subsequent write/sync (or a later deferred-apply) against the stale memory buffer — whose exchange markers diverged from disk — is the suspected vehicle that placed the response at base level and duplicated content. The corruption may therefore originate at **plugin apply time against a stale buffer**, where the binary's pre-write guard never runs.

## Hypotheses to confirm with the artifact / repro

1. The denied-cancel divergence produced **duplicate `agent:exchange` openers** (or an exchange close tag above later content), so the patch routed overflow to base level and the duplicate-opener merge could not cleanly reconcile.
2. The exchange component failed to parse entirely (markers split across the stale-buffer/disk boundary), so `guard_no_conversation_tail_outside_exchange` no-opped.
3. A deferred IPC patch applied plugin-side after the cancel, bypassing the binary guard, and wrote `### Re:` at base level.

## Fix directions (decide after repro)

- **A. Tighten the guard for corrupted exchange.** When conversation/response content is being written and an `exchange` component is expected (template mode) but cannot be located as exactly one clean component (missing / duplicate / split markers), **fail closed** instead of no-op — distinguishing this from a legitimately exchange-less inline doc via document mode (`mode.rs`).
- **B. Detect base-level `### Re:` / prompt content above or between exchange markers**, not only after the close tag, and route it back inside (extend `escaped_*_outside_exchange`).
- **C. Plugin revalidates against HEAD before apply** (shared with `#jbccacceptdup`): reject a deferred patch whose base no longer matches, so a post-cancel stale-buffer apply cannot corrupt.
- **D. Never leave a corrupting deferred patch:** on Cancel, the plugin should drop (not retain) the file-IPC patch once the cycle has moved on, or the binary should invalidate it.

## Exit criteria

- Repro (artifact or SimWorld) of Cancel → base-level out-of-exchange duplicate.
- Deterministic SimWorld coverage in `src/agent-doc/src/sim_world.rs` alongside the existing `jb_cache_conflict_*` scenarios.
- Write path / plugin apply refuses to place response content outside a single clean `agent:exchange`, and the duplicate-content shape is auto-repaired or fails closed with a precise recovery message — no silent base-level corruption.

## Why deferred (not blind-fixed in the auto-loop)

No corrupted artifact (operator already fixed the document), the corruption likely originates plugin-side where the binary guard never runs, and tightening `guard_no_conversation_tail_outside_exchange` touches a guard with a legitimate early-return for inline docs. A wrong change either breaks inline-mode writes or masks the real plugin-apply path. Resolve in a reviewed session with the artifact or a faithful divergence repro.

## See also

- `runbooks/jb-cache-conflict.md` — Dialog Contract, Cancel recovery, Late-Accept replay.
- `tasks/agent-doc/plan-jb-cache-conflict-accept-duplicates-response.md` — `#jbccacceptdup` late-accept duplicate.
- `tasks/agent-doc/plan-jb-cache-cancel-stuck-cycle.md` — cancel-branch stuck-cycle phases.
- `tasks/agent-doc/plan-finalize-visible-buffer-ipc-timeout-race.md` — sibling visible-buffer race at finalize.
