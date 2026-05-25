# Plan: CRDT merge leaks `❯ ` user-prompt prefix into the response body line

**Bug class:** write / IPC / CRDT merge
**Related:** [#ipcdupfinalize] (IPC corruption + duplicate during typing), [#crdtlivpfxdup] (CRDT live prompt prefix duplicate)
**Severity:** high — corrupts the response body so `session-check` classifies the assistant response as a prompt-only closeout tail, blocking closeout

## Problem

When `agent-doc finalize` falls through to the CRDT merge path (socket IPC timeout + file IPC timeout + CRDT merge), the merge sometimes prefixes the **first non-empty line of the response body** with the harness prompt marker `❯ `. The `### Re: ...` heading is preserved unprefixed, but the first paragraph line ends up looking like a user prompt.

Direct evidence from this session, finalize cycle ending around 2026-05-24 21:43 for `tasks/agent-doc/agent-doc-bugs2.md`:

```
[write] IPC timeout — response saved as patch, awaiting plugin
[write] IPC timeout path: file modified, CRDT merging...
[crdt] merge: base_len=9880 ours_len=12850 theirs_len=9880
[write] CRDT merge successful — no conflicts possible.
[write] WARNING: visible write deferred before exit(75): visible editor buffer ... differs from the expected disk state
```

Resulting document (line numbers from after the CRDT merge landed):

```
### Re: do [#jbrunstart] Phase 1 — opus-4-7


❯ Landed Phase 1 only this cycle. Item stays open (`--pending-gate`d) — …
```

The leading `❯ ` on the first response paragraph is the harness's user-prompt marker and was never in the response stdin. `session-check` then refuses to close:

```
[session-check] INTERRUPTED: live exchange ends with unresolved prompt-only
closeout tail and no assistant response: Landed Phase 1 only this cycle…
```

Manual repair: delete the leading `❯ ` and re-run `agent-doc write --commit --force-disk <FILE>`.

## Root cause hypotheses

1. **CRDT merge replays the user's last typed prompt prefix onto the first response line** when the live document (theirs) has a recent `❯ ` user input at the same column position. The merge inserts the agent-content line *into* the existing prompt run instead of *after* it.
2. **The pre-patch boundary insertion races the CRDT delta**: `template::pre-patch boundary inserted at end of exchange` is followed by the response patch, but the live document's pending prompt prefix has not yet been moved out of the exchange when the CRDT merge runs.
3. **The response patchback path has no post-merge sanity check** to detect "first non-empty line of the response body starts with the harness prompt marker" and fail closed before disk write.

## Plan

### Phase 1: Deterministic SimWorld repro

**File:** `src/sim_world.rs` (or sibling)

Construct a scenario:
1. Live exchange ends with `❯ do [#whatever]` (user-typed prompt awaiting response).
2. Agent submits a response patchback whose first body line is plain text.
3. Force socket-IPC timeout + file-IPC timeout to drive CRDT merge fallback.
4. Assert the merged result does NOT prefix the first response body line with `❯ ` (or any user-prompt marker).

Until the fix lands, this scenario reproduces the corruption (failing baseline).

### Phase 2: CRDT merge post-condition guard [done — option (a) chosen]

**File:** `src/write.rs` (landed)

`normalize_final_template_content` now calls a new `strip_prompt_prefix_from_response_body_first_lines(content)` helper. The helper scans every `### Re: ...` block inside `agent:exchange`; if the first non-empty body line starts with `❯ ` (or the bare marker character), it strips the prefix and the surrounding code logs `flow=document_mutation stage=crdt_post_merge_guard reason=response_body_prompt_prefix_leak`. Lines after the first body line are untouched (so a quoted `❯ ` inside the response body remains intact), and user prompts outside the response block are preserved.

Chose option (a) over (b) because the corruption signature is unambiguous (real responses never start with `❯ ` — that's the harness prompt marker, not Markdown), and a fail-closed path here would reproduce the wedge that motivated the bug.

**Tests (`write::submodule_patch_routing_tests`):**
- `strip_prompt_prefix_from_response_body_first_lines_strips_leaked_marker` — happy path: leaked prefix stripped, response body intact, neighbouring user prompts preserved.
- `strip_prompt_prefix_from_response_body_first_lines_skips_when_clean` — clean document returns `None`, no allocation/rewrite.
- `strip_prompt_prefix_from_response_body_first_lines_preserves_inner_prompt_like_lines` — quoted `❯ ` after the first body line is preserved.
- `strip_prompt_prefix_from_response_body_first_lines_handles_multiple_re_blocks` — both response blocks in a multi-Re exchange get repaired independently.

### Phase 3: Repair path for the corruption signature

**File:** `src/repair.rs`, `src/dedupe.rs`

`agent-doc repair` (and `agent-doc write --commit` empty-stdin recovery) should recognize the prompt-prefix-on-response-body signature and either strip it deterministically or report the corruption with the exact line for the operator. Currently the only way out is manual editing.

### Phase 4: Regression coverage

1. CRDT merge unit tests covering "live prompt prefix immediately before injected response line" — assert no prefix leak.
2. SimWorld scenario from Phase 1.
3. Repair-path unit test covering automatic recovery from a prefixed response body.

## Testing

- Manual repro: type `❯ do [#foo]` at the end of the exchange, race a CRDT-merge finalize cycle, assert response body lands unprefixed.
- `make check` clean (clippy + full test suite).
- The SimWorld scenario must pass.
