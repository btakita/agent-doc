# Plan: `--pending-add` must match the documented prepend semantics

## Symptom

A finalize cycle that passed four `--pending-add` operations to capture a wave-by-wave breakdown of `#adcr` produced backlog order **opposite** to the agent's intent.

Add order on the CLI (in agent's strategy: add tail-first so head-of-batch ends up on top):

```
--pending-add "#adcr-wave-4 ..."
--pending-add "#adcr-wave-3 ..."
--pending-add "#adcr-wave-2 ..."
--pending-add "#adcr-wave-1 ..."
```

Expected backlog (per `SKILL.md` "Pending capture rule": *Put new items at the beginning of `agent:backlog`*):

```
1. wave-1 (added last → ends up on top)
2. wave-2
3. wave-3
4. wave-4 (added first → ends up just above existing #adcr)
5. adcr (pre-existing)
```

Actual backlog committed in `f94e7851c`:

```
1. wave-4 (#mzzd) ← top
2. wave-3 (#yxvq)
3. wave-2 (#yhsr)
4. wave-1 (#pjb4)
5. adcr
```

Each `--pending-add` actually **appended** the new item below the previously-added one inside the same CLI invocation. The result is pure CLI-arg order, not "newest-on-top." User then flagged the backlog as not priority-ordered (`tasks/agent-doc/open-code.md` exchange tail).

## Root cause hypothesis

Two candidate code paths:

1. **CLI processing loop** — `finalize` consumes `--pending-add` args in their argv order and calls `pending::op_add` (or equivalent) once per arg. If `op_add` itself prepends, then back-to-back prepends should still produce a stack — first arg becomes deepest. The observed behavior is the opposite, so either `op_add` actually appends, or the loop reverses the args, or the "adjacent to predecessor" branch fires for every consecutive add.
2. **"Ordered batch" heuristic** — `SKILL.md` says: *"if you are extending an ordered batch already in pending, insert the new item adjacent to its predecessor."* This may be detecting the previous CLI-arg add (just inserted) as the predecessor and inserting the next one **below** it to preserve adjacency. That would explain the pure append behavior: each subsequent add finds its predecessor at the top and inserts immediately below.

Both branches need inspection. The expected files:

- `src/agent-doc/src/pending.rs` — `op_add` and `op_insert_item_first` definitions. The skill rule names insertion at the beginning, and the public function `op_insert_item_first` exists (see `pending.rs` public list captured in `tasks/agent-doc/plan-agent-doc-core-extraction.md` §"Surface"). If `op_add` is wired to a position-aware insertion that picks "first" by default but switches to "after predecessor" for same-batch, the loop interaction is the bug.
- `src/agent-doc/src/write.rs` / `pending_cmd.rs` — the CLI consumer that converts `Vec<String>` of `--pending-add` args into a sequence of `op_add` calls. Look for any "ordered batch detection" or any reversal of args.

The user-visible spec (`SKILL.md`) and the binary behavior must match.

## Fix options

**Option A — make `--pending-add` strictly prepend regardless of batch context.** Each add inserts at the top (`op_insert_item_first`). Multiple `--pending-add` args in one CLI invocation produce the natural stack (first arg = deepest). The "ordered batch adjacency" rule only applies when an explicit predecessor is named via `--pending-add-after <id>` or similar — never inferred from prior CLI args. Simplest, matches the documented behavior, predictable. **Recommended.**

**Option B — process CLI args in reverse before passing to the prepend loop.** Each individual add still uses the adjacency heuristic, but the agent's natural "type wave-1 first" ordering produces the right result. Smaller diff but couples user-visible CLI semantics to argv parsing direction; doesn't fix any non-CLI consumer (`agent-doc-cli` from a script, FFI, etc.).

**Option C — document the actual behavior as the spec.** Update `SKILL.md` to say "`--pending-add` appends; add items in reverse priority order to get priority-on-top." Worst of the three: every agent now needs the inverted-order convention; the "Pending capture rule" gets harder to teach; this exact bug will recur across agents.

**Recommended:** Option A. Update `SKILL.md` only to clarify that adjacency is opt-in via a future flag (or remove the second clause if no flag exists).

## Acceptance

1. Repro: `agent-doc finalize <FILE> --stream --pending-add "#new-a ..." --pending-add "#new-b ..."` against an empty backlog → backlog ends as `[#new-b, #new-a]` (b on top, because it was added last → newest-on-top).
2. Repro with existing backlog `[#old]`: `--pending-add "#new-a" --pending-add "#new-b"` → backlog ends as `[#new-b, #new-a, #old]`.
3. Unit test in `pending.rs` covers (a) single add prepends; (b) two adds in one cycle stack newest-on-top; (c) add into non-empty backlog prepends.
4. `make check` green.
5. `SKILL.md` "Pending capture rule" body matches binary behavior — either keeps the "beginning of backlog" wording (Option A) or explicitly documents adjacency-as-default (Option C, not recommended).

## Out of scope

- Whether the agent's "tail-first add order" strategy is the right teaching for the skill — that's a separate doc improvement. This bug is specifically that the binary does not match the documented spec.

## Files likely touched

- `src/agent-doc/src/pending.rs` — `op_add` / `op_insert_item_first` (Option A: ensure `op_add` always uses `op_insert_item_first` semantics).
- `src/agent-doc/src/write.rs` — `--pending-add` consumer loop.
- `src/agent-doc/SKILL.md` — wording cleanup (Option A: confirm "Put new items at the beginning" matches).
- `src/agent-doc/SPEC.md` — pending operations contract.

## Related

- `tasks/agent-doc/agent-doc-bugs2.md` — backlog item references this plan.
- `tasks/agent-doc/open-code.md` — original repro session; user prompt: "The backlog is not ordered by priority. #agent-doc-bug. Please order by priority."
- `tasks/agent-doc/plan-queue-strike-on-halt-response.md` — earlier bug in the same class (binary heuristic firing where SKILL.md describes simpler explicit behavior).
