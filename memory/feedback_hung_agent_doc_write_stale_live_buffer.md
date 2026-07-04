---
name: feedback_hung_agent_doc_write_stale_live_buffer
description: "a hung background agent-doc write/finalize silently races later finalizes via a stale .agent-doc/live-buffer sidecar (exit 75 / \"visible editor buffer differs\")"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: 08bdde9b-eeac-4a3b-b18a-6dd07ee901ee
---

A backgrounded `agent-doc write`/`finalize` can hang indefinitely (observed `STAT S<l`, 40 min) instead of exiting, and a hung writer leaves a **stale `.agent-doc/live-buffer/<pathhash>` sidecar** (a short 16-hex DefaultHasher of the canonical path, NOT the doc hash). Every later `finalize` — even `--force-disk` — then fails the visible-buffer guard with exit 75 then exit 1: *"visible editor buffer for <FILE> differs from the expected disk state; save or discard the editor buffer, then retry"*, plus repeated 2s IPC ACK timeouts.

**Why:** `guard_visible_write_idle_and_current` (write.rs) calls `debounce::live_buffer_diverges_from_content`, which reads that sidecar. A wedged/dead listener never updates it, so its stale `len`/`hash` diverges from disk forever. Compounds [[feedback_agent_doc_supervisor_self_race_finalize_block]] (route-owned supervisor race) but the decisive culprit here was the orphaned writer, not the supervisor.

**How to apply:**
1. Diagnose: `pgrep -af agent-doc` → look for a long-`ELAPSED` `agent-doc write`/`finalize` (a write should finish in ms). `kill` it + its wrapper shell.
2. Find the stale sidecar: `for f in .agent-doc/live-buffer/*; do python3 -c "import json;d=json.load(open('$f')); print(d['path'],d['len'])"; done` — match by your doc's path; its `len` will equal the failed merge `base_len`. `rm` it.
3. The response is capture-backed (preflight: "captured response body ... not present in HEAD; recover via agent-doc write --commit"); retry `finalize --force-disk`. It consumes the head + commits.
4. PREVENTION: never launch `agent-doc write`/`finalize` with Bash `run_in_background` and walk away — confirm it exits. Read-only `agent-doc` (preflight/session-check) is fine to background. Related [[feedback_agent_doc_pending_flag_backtick_substitution]].

**Two follow-on failure modes from the same wedged listener (also seen this session):**
- **Late-patch DUPLICATE → queue stall:** a finalize whose boundary-reposition IPC times out queues a fallback patch; the wedged listener applies a *stale* patch minutes late, appending a SECOND `### Re:` block to the working tree after the committed one. `git diff HEAD` shows `+N` lines = one duplicate block; HEAD has the correct single copy. Recovery: `git checkout HEAD -- <file>` to discard the duplicate, then preflight to reconcile. (Captured as agent-doc bug `#late-ipc-patch-duplicate-stall`.)
- **`write --commit` leaves a stuck cycle** (`response_hash: null`): recovering a missed response with `agent-doc write --commit` (instead of `finalize`) commits the response into `agent:exchange` but does NOT bind a response capture to the open cycle, so session-check loops forever on "committed binary-owned work … but no assistant response body was captured." `cancel` won't take it (it's `committed`, not `preflight_started`). Fix: the git content is authoritative — `rm .agent-doc/state/cycles/<dochash>.json`, run `preflight` (reconciles, opens a fresh empty cycle), then `agent-doc cancel <FILE>` to abandon that empty cycle → session-check OK. Prefer `finalize` over `write --commit` for response recovery so the capture binds.
