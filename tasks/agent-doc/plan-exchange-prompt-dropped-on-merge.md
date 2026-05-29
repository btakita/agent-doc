# Plan: user exchange prompt silently dropped on IPC `content_ours` merge

**Bug id:** `#exchange-prompt-dropped-on-merge`
**Status:** investigation + fix plan (DO NOT IMPLEMENT yet)
**Severity:** high — silent loss of user input; no error, no response, prompt vanishes.

## Symptom

User types a short prompt (observed: `go`) into the `agent:exchange` of a session
document while an editor (IntelliJ IDEA, holding `.agent-doc/ipc.sock`) has the file
open. On the next `agent-doc` cycle the prompt is **gone** — it never reaches the
snapshot or HEAD, the assistant never responds, and subsequent `agent-doc preflight`
runs report `no_changes: true`. The user only discovers the loss later ("my prompt
'go' was deleted by agent-doc").

Reported 2026-05-29 on `src/boost-client/tasks/monsterrodholders.md`. The dropped `go`
was the user's authorization to start `#gfwconnect`; the assistant kept replying
"nothing changed" across two cycles because the prompt had been silently discarded.

## Root-cause hypothesis

Same multi-owner IPC race as [[agent-doc-ipc-drift-recovery]] /
`#adoc-queue-ipc-buffer-divergence` / `#duplicate-response-after-commit`, but the
victim here is **user-authored exchange content**, not the assistant response or the
queue component:

1. The editor's `ipc.sock` acks a **stale buffer** (`snap_source=ack_content_sidecar`,
   `content_ours`) that predates the user's just-typed prompt.
2. `finalize`/`write`/`commit` trusts the IPC ack `content_ours` over the larger,
   correct disk content that contains the new prompt (the same
   `live_prompt_drift_after_preflight` → `recovery=content_ours_snapshot_next_cycle`
   path seen earlier in this very session at the 04:07 cycle).
3. Because `content_ours` is adopted as the committed snapshot, the user's new
   exchange line is overwritten/never captured. Unlike the assistant-response or
   pending-mutation variants (which at least strand the edit in the working tree),
   an exchange prompt typed *into the editor buffer* can be lost entirely when the
   editor later re-saves its post-merge buffer to disk.
4. `ipc_socket::send_patch` carries component **CONTENT only** and cannot converge
   divergent buffers (noted in `#adoc-queue-ipc-buffer-divergence`); there is no seam
   that preserves user-authored exchange additions during an ours-wins adoption.

Net: the merge is effectively "ours (stale IPC buffer) wins" for the exchange
component, with no union of user-authored tail additions and no fail-closed guard
when a prompt-bearing line would be dropped.

## Why existing guards don't catch it

- `session-check` validates the exchange/prompt **tail that is present**; it cannot
  flag a prompt that was already discarded before commit (nothing to compare against).
- The `pending_done_guard` / pending false-pass note ([[agent-doc-ipc-drift-recovery]])
  covers backlog/queue mutations, not lost exchange prompts.
- Preflight reports `no_changes` precisely because the snapshot was frozen at
  `content_ours`, so the dropped prompt is invisible to the next cycle.

## Proposed fix options

**Option A (recommended) — never drop user-authored exchange lines; fail closed.**
Before adopting `content_ours` on an IPC-drift (`live_prompt_drift_after_preflight`),
diff the disk/working-tree `agent:exchange` tail against `content_ours`. If disk has
prompt-bearing lines absent from `content_ours`, **union** them into the committed
content (user prompts are append-only and must never be lost). If a safe union is not
possible, **abort the cycle** with an explicit recovery instruction instead of silently
committing `content_ours`. Add a `dropped_prompt_guard` to `session-check` that
interrupts when the pre-commit disk exchange tail contains prompt-bearing lines not in
the snapshot being committed.

**Option B — IPC ack authoritative for assistant region only.**
Treat the editor/IPC `content_ours` as authoritative only for the assistant-response
span; always reconcile user-authored exchange prompts from disk. Requires marking the
response region boundary in the IPC payload.

**Option C — plugin protocol extension (depends on `#adoc-queue-ipc-buffer-divergence`).**
Extend `PatchWatcher.kt` + FFI so the editor buffer converges to the merged disk
content (full-content IPC seam) rather than re-asserting its stale buffer, eliminating
the ours-wins overwrite at the source. Heaviest; shares the harness with the queue/
frontmatter convergence work.

## Repro harness (to build during implementation, not now)

Deterministic SimWorld scenario: (1) commit a baseline session doc; (2) simulate an
editor IPC listener acking a stale `content_ours` buffer; (3) append a prompt-bearing
line to the disk exchange tail out-of-band; (4) run `finalize`; (5) assert the new
prompt survives in HEAD (Option A) or the cycle fails closed with a recovery message.

## Out of scope / do not implement yet

Per `#agent-doc-bug` declaration: this is a plan only. No code changes until the
approach (A/B/C) is chosen. Recommend **Option A** — it is the smallest change that
makes the data-loss class impossible (user prompts become non-droppable) and adds a
detectable guard, independent of the heavier plugin-protocol work in Option C.

## Related

- [[agent-doc-ipc-drift-recovery]] (project memory) — `content_ours` / stale IDE
  buffer recovery; `--pending-add`/`--pending-edit` false-pass variants.
- `#duplicate-response-after-commit` — late IPC reposition re-inserts response body.
- `#adoc-queue-ipc-buffer-divergence` — IPC carries component content only; no seam
  to converge queue tag / frontmatter (same protocol limitation implicated here).
