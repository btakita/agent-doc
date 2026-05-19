# Plan: Block CRDT Duplicate Scaffold Corruption

**Backlog:** `#scaf2`
**Status:** Plan captured; implementation appears present in the local agent-doc tree and needs install/release verification against the observed 0.33.16 corruption.

## Problem

While the user was typing in `agent:exchange`, a CRDT closeout produced a malformed template document:

- a duplicate `<!-- /agent:exchange -->` close marker appeared
- queue/backlog/done scaffold content was copied into the exchange tail
- live user text was split around the duplicated scaffold

That shape must not be committed as a successful response closeout.

Latest observed shape: `tasks/software/agent-loop.md` was corrupted after a CRDT closeout by inserting a duplicate close marker plus copied pending/done scaffold between live prompt fragments `agent-skill` and `agent-skills is`.

## Desired Behavior

- If the duplicated content is a pure repeated template scaffold, closeout may repair it by dropping the duplicate scaffold.
- If the duplicated scaffold is mixed with live user text, closeout must fail closed instead of deleting that text or committing a corrupted document.
- The normal CRDT/template closeout normalizer must run the scaffold repair path, not only the escaped-response duplicate-close repair path.

## Implementation

- Wire `repair_duplicate_exchange_close_scaffold` into `normalize_template_structure_or_fail`.
- Tighten duplicate scaffold detection so it only accepts structural headings, non-agent comments, and known template scaffold components.
- Reject mixed duplicate scaffold content containing user text such as the observed `corky.md The arrow` tail and the `agent-skill` / `agent-skills is` split.
- Add regressions for both the repairable pure scaffold case and the mixed-content fail-closed case.

## Verification

- [x] `cargo test scaffold --lib`
- [x] `cargo test --all-targets normalize_template_structure_ -- --test-threads=2`
- [x] `make check`
- [x] `make tmux-ci`

## Files Changed

- `src/agent-doc/src/template.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/VERSIONS.md`
