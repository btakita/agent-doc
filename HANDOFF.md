# HANDOFF: #claimstrike

Branch `claimstrike` (from main). Do NOT push, install, release or merge.

## Defect

A free-text `agent:queue` head held by a worker claim (`agent-doc queue claim --owner subagent:...`) was not
struck when the response echoed it as `> **Queue prompt:** <line>`. The commit landed, then session-check
INTERRUPTED on `#qheadresidue` (completed residue). The repair needed `queue release` + `preflight` + `commit`.

## Root cause

`#deferstrike` (0.35.453) made every strike path skip heads with a live claim:
`answered_free_text_head_node_keys_excluding_claimed` in `agent-doc-queue/src/queue_consume.rs` (`if
claimed.claims(text) { continue; }`), fed by `claimed_live_head_texts_for_content` from the finalize strike
(`agent-doc-queue-io/src/queue_consume.rs` `strike_answered_free_text_queue_heads`), the controller projection,
the commit-time late-strike recovery, and preflight's `#qheadresidue` catch-up (`!claimed.claims(&p.text)`).
The session-check residue guard (`free_text_queue_head_is_completed_residue`) never consulted claims, so the two
disagreed on every echoed claimed head.

## Fix (done)

- Pure strike selection no longer consults claims; `_excluding_claimed` variants removed. The deferral detection
  (`response_defers_free_text_head` / `latest_free_text_head_echo_is_deferral`) is what keeps a quoted head queued.
- Finalize (`strike_answered_free_text_queue_heads`) releases the claim of each struck claimed head in the same
  closeout via new `queue_claim::release_closed_head` (no live-head resolution, no controller RPC).
- Preflight `#qheadresidue` catch-up strikes claimed heads too and releases their claims.
- Controller projection + commit-io + main.rs drop the claimed_heads plumbing.
- Pre-write evidence gate (`run_entry.rs`) still owes no answer for a claimed head (unchanged).
- Docs: SKILL.md (+5 harness copies), runbooks/respond.md (+5 copies), specs/07 `#queueclaim`, SPEC.md `#ftstrike`.
  VERSIONS.md left for the release commit.

## Tests

- `agent-doc-queue-io` `claimstrike_*` (3): claimed+echo struck and claim released, residue decision empty;
  claimed+`**Deferred:**` stays and stays claimed; claimed `do [#id]` completes by `--done`.
  The first fails on pre-fix code (`left: 0 right: 1`).
- `agent-doc-preflight-io` `run_queue_maintenance_strikes_claimed_answered_head_and_keeps_deferred_one`.
- Controller test renamed to `claimed_free_text_head_answered_by_the_response_projects_a_strike`.
- `agent-doc-queue` deferstrike tests reworked (no claim param).

## Status

See git log for the latest milestone; `make check` result recorded below.
