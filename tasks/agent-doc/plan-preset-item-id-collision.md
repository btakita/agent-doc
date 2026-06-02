# Plan: prevent preset and task item id collisions

## Problem

Live repro from `src/boost-client/tasks/monsterrodholders.md` on 2026-05-30:

1. Frontmatter defines a prompt preset named `#next-steps`.
2. `agent:backlog` also contains an active item with id `#next-steps`.
3. User prompts such as `do #next-steps` and response text such as "Top backlog item:
   `#next-steps`" become ambiguous: the same token can mean "expand/run a preset" or
   "execute/consume/gate/done this backlog item".

Expected behavior: all live identities in a session document should be unique across
prompt presets and task items, so every `#id` has one meaning inside that document.

## Required Invariant

For each session document, normalize ids by stripping the leading `#` and comparing
case-sensitively against one identity registry. The following identities must be
unique:

- frontmatter `prompt_presets` keys
- `agent:backlog` item ids
- `agent:review` item ids, because gated items are still active backlog work
- `agent:icebox` item ids

`agent:done` / archived completed ids may be reported as warnings when useful, but
they should not block reuse unless agent-doc already treats them as active lookup
targets.

## Implementation Outline

1. Add a document identity registry helper:
   - parse frontmatter with the existing YAML/frontmatter parser;
   - collect `prompt_presets` keys;
   - collect active pending ids from backlog, review, and icebox using the existing
     pending/task item parser instead of regex-only scanning;
   - return precise collision diagnostics with both locations and component names.

2. Enforce the invariant at mutation boundaries:
   - `agent-doc write --pending-add id=<id>` rejects ids that collide with prompt
     presets or existing active items;
   - pending edit/reorder/gate/done paths validate that a command's target id maps
     to exactly one active identity;
   - queue generation from backlog refuses to generate `do [#id]` for an ambiguous id.

3. Enforce the invariant before dispatch:
   - preflight/session-check fail closed when an existing document has active collisions;
   - diagnostics should name the repair, for example "rename backlog item `#next-steps`
     or preset `#next-steps`; do not dispatch until the identity is unique";
   - route/JB should surface this as a document repair error, not as a generic agent
     failure.

4. Add an optional repair helper later:
   - do not auto-rename by default;
   - a future explicit command such as `agent-doc backlog rename-id old new` can update
     backlog/review/icebox references and matching queue lines in one transaction.

## Tests

- Fixture matching `monsterrodholders.md`: `prompt_presets.#next-steps` plus
  `agent:backlog` item `#next-steps` causes preflight/session-check to fail closed.
- `write --pending-add 'id=next-steps ...'` refuses the add when the document already
  has a `#next-steps` preset.
- Backlog/review/icebox duplicate active ids produce one diagnostic with every source
  location.
- Queue generation or `do [#next-steps]` dispatch does not choose between preset and
  item silently; it reports the collision.
- A completed archived `#id` plus a new active item is either allowed or downgraded to
  warning according to the documented active-id policy.

## Likely Files

- `src/agent-doc/agent-doc-orchestration/src/preflight.rs`
- `src/agent-doc/agent-doc-orchestration/src/session_check.rs`
- `src/agent-doc/src/pending.rs`
- `src/agent-doc/src/write.rs`
- `src/agent-doc/src/notify.rs`
- `src/agent-doc/agent-doc-orchestration/src/queue.rs`
- `src/agent-doc/tests/test_cli.rs`
- `src/agent-doc/tests/run_integration.rs`
- `specs/07-orchestration-commands.md`
- agent-doc skill/runbook docs for prompt preset and backlog id rules

## Acceptance

- A document cannot acquire a new active backlog/review/icebox id that collides with a
  prompt preset.
- An existing collision blocks dispatch with a specific diagnostic and repair hint.
- The live `monsterrodholders.md` collision is caught by a regression fixture.
- The documented identity rule says one `#id` has one active meaning per document.
- `make check` passes after implementation.
