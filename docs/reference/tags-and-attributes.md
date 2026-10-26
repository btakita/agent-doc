# Tags and opening-marker attributes

This page is the user-facing reference for agent-doc's Markdown comment
protocol. It covers document components, response patch blocks, queue and
status attributes, and the markers written by agent-doc itself.

## Component syntax

A component is a paired HTML comment whose body remains ordinary Markdown:

```markdown
<!-- agent:status -->
Ready.
<!-- /agent:status -->
```

The opening marker is `<!-- agent:NAME [ATTRIBUTES] -->`; the closing marker
is `<!-- /agent:NAME -->`. A name must match
`[a-zA-Z0-9][a-zA-Z0-9-]*`. The close name must match the open name.
Components may nest, but the markers must be balanced and properly nested.
An opening marker may follow its matching ATX heading on the same line, such as
`## Run <!-- agent:run priority subagents -->`; the heading text must match the
tag name. Closing markers remain standalone.

Opening attributes are separated by whitespace. A value without spaces may be
unquoted; use matching single or double quotes for a value containing spaces:

```markdown
<!-- agent:run id=release preset="#build #publish" -->
<!-- /agent:run -->
```

Marker-shaped text in inline code or a fenced code block is prose, not
protocol. Quote examples in backticks or a code fence.

## Built-in components

These names have built-in semantic behavior. New repeating work should use
`loop`; `queue loop` (and `run loop`) remain compatibility spellings. `pending`
is the legacy alias for `backlog`.

| Component | Ownership and purpose |
|---|---|
| `agent:exchange` | Shared conversation history and the normal response destination. Changes can start a turn. Template responses append by default. |
| `agent:run` | Binary-managed runnable prompt queue. Queue item syntax, claims, dependencies, scheduling, and consumption apply. |
| `agent:loop` | Repeating runnable prompt queue. It implies an unbounded loop; add `loop=N` for a finite number of passes. Legacy `agent:queue loop` and `agent:run loop` markers behave identically. |
| `agent:backlog` | Granular tracked-work source. Use backlog operations instead of replacing its whole body. |
| `agent:pending` | Legacy alias for `agent:backlog`. |
| `agent:review` | Granular tracked work waiting at an external or operator review gate. |
| `agent:icebox` | Granular tracked work parked outside the runnable backlog. |
| `agent:done` | Completed-work archive, inline or redirected with `archive`. |
| `agent:status` | Current status projection. It may contain ordinary status prose and protected binary-owned operational lines. |
| `agent:signals` | Realtime signal definitions/readings reconciled by the signal subsystem. |
| `agent:todo` | Operator-owned informational checklist. It is not runnable work and is never promoted to the queue. |
| `agent:notes` | Operator-owned informational context. Content-only edits do not start a turn. |

`agent:model` is a supported preflight control component rather than a
registry-owned work component. Its trimmed body may be a tier
(`auto`, `low`, `med`, or `high`) or a model name known to the active harness.

Names such as `input`, `output`, `findings`, `log`, and `architecture` are
conventional generic patch slots. Any syntactically valid unregistered
`agent:NAME` is parsed as a component, remains a turn trigger by default, and
uses the unknown-element fallback: preserve its operator-visible content and
do not apply built-in semantic item mutations. `agent-doc patch` and a
matching response patch can still address it. A plugin may register richer
semantics later.

## Common component attributes

| Attribute | Where | Values and behavior |
|---|---|---|
| `patch` | Patchable components | `replace`, `append`, or `prepend`. For template response application, the inline value overrides project component configuration. `exchange` and `findings` default to `append`; other names default to `replace`. |
| `mode` | Patchable components | Compatibility alias for `patch`. If both appear, `patch` wins. |
| `max_lines` | Patchable components | Non-negative integer. After template patching, retain only the last `N` lines; `0` or absence means unlimited. |
| `priority` | `backlog`, `pending`, `icebox`, `review`, `todo`, `queue` | Bare flag enabling component-level priority ordering. Item-level `priority=1` through `priority=9` is separate item syntax. |
| `archive` | `done` | Bare `archive` writes to `<document-stem>.done.md` beside the document. `archive=PATH` selects a project-contained path ending in `.done.md`. |

`patch=replace` controls how a response patch changes a component. It does not
grant ownership of status text, enable status synchronization, or make a
component binary-managed.

The parser also recognizes `timestamp`, `transfer-source`, and `broken` as
compatibility/reserved component keys. They have no public opening-marker
behavior in the current component patch pipeline. Timestamping is configured
with `timestamp = true` under `[components.NAME]` in
`.agent-doc/config.toml`; do not rely on an opening-marker `timestamp` key.

## Status synchronization

`agent:status` can carry a standing instruction that asks the active agent to
keep a fact or summary current:

```markdown
<!-- agent:status sync="keep the last published release and publication time current" interval=5m -->
Current release: v0.35.523, published 2026-10-19 07:59:51 UTC.
<!-- /agent:status -->
```

`sync` is evaluated at preflight and added to the active turn's prompt. On
Claude Code and Codex, the owner-only `Stop` hook re-surfaces it once after the
turn's queue and persistence obligations settle, so a fact that changed during
the turn is checked against its current authoritative source before handoff.
That final check is bounded by the harness recursion guard: an unchanged answer
makes no write, while a changed answer uses `status set` plus `commit`; the next
Stop passes through. The owner-only `PostToolUse` hook can also re-surface the
instruction no sooner than the configured `interval` after preflight and at
most once per following interval. This is a throttled prompt checkpoint, not a
timer,
background job, queue item, direct document write, or promise that external
facts were verified.
It cannot interrupt one continuously running tool call; once overdue, the next
completed owner tool call surfaces the reminder immediately.
The reminder is additive to normal mid-turn steering, and concurrent hooks
publish tool-boundary observations to the controller-owned Lazily graph. Its
cycle-and-cadence-keyed Computed derives the due edge, and the controller
atomically records the receipt that consumes that edge. SQLite is not a live
timing or arbitration authority. The instruction should explicitly request
verification when it depends on an external source.

Periodic mid-turn checkpoints default to off. With `sync` but no `interval`,
the instruction is still checked at preflight and final Stop, but the
`PostToolUse` hook publishes an explicit disarm fact and injects no repeated
checkpoint. A valued `interval` opts in. It accepts a unitless number of seconds
or a compound duration using
`s`, `m`, `h`, and `d`, such as `5m`, `1h30m`, or `1d`. It is valid on
`agent:status` only when `sync` is present. Bare, blank, duplicate, malformed,
overflowing, and sub-minute values fail closed. The one-minute floor prevents a
configuration intended to save tokens from instead amplifying prompt traffic.
(`agent:run interval=...` retains its independent schedule semantics.) Editing
the interval during an open cycle recalculates the generation-keyed due edge
from that cycle's start.

During an open turn, the reminder names the safe focused command explicitly:
`agent-doc status set <FILE> --text "<current result>"`. It updates only status,
attributes that component delta to the agent baseline, and does not capture,
seal, append a response, mutate the queue, or commit the active cycle.
At the final Stop checkpoint the response transaction is already closed, so a
changed result is followed by `agent-doc commit <FILE>`; an unchanged result is
silent and leaves the document untouched.

`sync` requires a non-empty instruction. Synchronization is additive by
default (`sync-mode="additive"` is the explicit spelling). The agent upserts
exactly one neutral managed block, delimited by `<!-- status:sync -->` and
`<!-- /status:sync -->`, while preserving every byte outside that block.
Repeated turns update the block rather than accumulating duplicate copies.
Pass only the result as `--text`; if the text carries its own
`<!-- status:sync -->` block, agent-doc parses it as the whole status body and
replaces the managed block instead of nesting it. A duplicate or malformed
managed block fails closed at preflight, which names the binary-owned repair
`agent-doc status repair <FILE>`; any status write also folds such a structure
back to one block.

For explicit ordinary-prose replacement, opt into the destructive mode:

```markdown
<!-- agent:status sync="show only the current published release" sync-mode="replace" -->
<!-- /agent:status -->
```

`sync-mode="replace"` updates the managed block and removes ordinary status
prose. It preserves the `📤 Queue handoff:` projection (and migrates its legacy `📥` spelling), the
`🔴 (restart/recycle your supervisor)` stale marker, and warning paragraphs
whose prefixes are `[agent-doc sync]`, `[agent-doc merge]`,
`[agent-doc repair]`, `[sync]`, `[merge]`, or `[repair]`. The opening marker
and all of its attributes remain byte-identical through either mode. An empty
or unknown `sync-mode` value fails closed and leaves the status body unchanged.

In a document with an `agent:run`, `agent:loop`, or `agent:queue` section,
every status write keeps one binary-owned `Queue markers: …` legend line as the
last paragraph of the status body (see
[Queue Markers](../guide/queue-markers.md#queue-markers)). `queue-legend=off`
on the status marker hides it; `queue-legend` also accepts `on`, `true`/`false`,
`yes`/`no`, and `1`/`0`.

## Queue attributes

`agent:run` and `agent:loop` accept the full queue vocabulary. Legacy
`agent:queue` does too as a compatibility alias.

| Attribute | Values and behavior |
|---|---|
| `id` | Unique queue ID. It begins with an ASCII letter or digit and continues with letters, digits, `.`, `_`, or `-`. A single unkeyed queue is `default`; multi-queue documents are assigned stable IDs by maintenance. |
| `after` | Comma-separated predecessor queue IDs. All predecessors must drain before this queue becomes ready. Missing IDs, self-dependencies, empty entries, and cycles fail closed. |
| `start`, `go` | Bare, one-shot activation controls. `go` is the normal continuous-drain spelling; `start` is its start-trigger alias. |
| `pause` | Bare operator hold. The legacy marker spelling `stop` is read as `pause`. |
| `auto` | Deprecated bare activation flag retained for compatibility. Prefer the default running state, `go`, or frontmatter `queue: go`. Binary-owned rewrites remove it. |
| `clear` | Bare positive declaration of the default fresh-context policy. Direct queues clear once per completed turn; `subagents`/`fan-out` queues clear at the next safe idle boundary after in-flight claims settle. Omission remains enabled for compatibility. |
| `disable-auto-clear` | Legacy opt-out. Bare or truthy disables the fresh-context reset; `false`, `0`, `no`, or `off` leaves it enabled. When both spellings appear, this opt-out wins. |
| `preset` | One or more whitespace-separated prompt-preset references. Quote a multi-reference value. `preset=""` explicitly selects no component default. |
| `presets` | Compatibility alias for `preset`; a nonempty canonical `preset` wins when both appear. |
| `subagents`, `fan-out` | Bare dispatches every eligible head to subagents; `=N` sets a positive concurrency cap. If both aliases appear, the most restrictive valid cap wins. On `agent:exchange`, only the bare form is valid because one prompt is dispatched at a time. |
| `model` | Optional model override for subagents dispatched from this queue. It takes precedence over structured preset metadata and frontmatter `subagent_model`. |
| `effort` | Optional reasoning-effort override for subagents dispatched from this queue. It takes precedence over structured preset metadata and frontmatter `subagent_effort`. |
| `priority` | Bare priority ordering, as described above. |
| `interval` | Schedule interval: a bare integer in seconds or a compound duration such as `30s`, `5m`, `1h30m`, or `1d`. Values below the scheduler floor are clamped with a warning. |
| `time` | One or more comma-separated local times in `HH:MM` form. It may be combined with `interval`; the earlier next deadline wins. |
| `loop` | Implied by `agent:loop`; retained as a compatibility attribute on `agent:queue` / `agent:run`. Bare repeats passes without limit; `loop=N` runs a positive number of passes. Without `interval` or `time`, the scheduler floor separates passes. |

`start`, `go`, `pause`, `stop`, `auto`, `clear`, `disable-auto-clear`, and bare
`subagents`/`fan-out` are flags, not `key=true` settings. Queue maintenance
canonicalizes boolean-serialized forms when it safely can.

The prompt attributes `preset`, `presets`, bare `subagents`, and bare
`fan-out` also apply to `agent:exchange`. Valued `subagents=N` and `fan-out=N`
are queue-only.

## Backlog-to-queue synchronization

`agent:backlog` and legacy `agent:pending` accept `queue`:

| Form | Behavior |
|---|---|
| bare `queue` | Append active id-backed backlog work to the queue. |
| `queue=append` | Explicit append mode. |
| `queue=prepend` | Add new mirrored work before existing queue entries. |
| `queue=sync` | Reconcile the id-backed mirror, including removal of stale mirrored IDs; preserve operator-authored free-text queue entries. |

The synchronization runs at queue activation boundaries. `queue` on
`agent:icebox` or `agent:review` is ignored with a warning; move work to the
backlog or enqueue it explicitly.

## Response patch tags

An agent response uses a different namespace so response directives cannot be
confused with document structure:

```markdown
<!-- patch:exchange -->
### Re: topic — model · timestamp

Response text.
<!-- /patch:exchange -->
```

`<!-- patch:NAME --> ... <!-- /patch:NAME -->` targets the matching
`agent:NAME` component. `NAME` is not a fixed allowlist: it must identify a
component in the destination document. A missing target is routed to the
exchange/output overflow path rather than silently discarded; `patch:todo` is
the exception and creates a missing `agent:todo` slot. Strict template
closeout requires a closed `patch:exchange` containing a `### Re:` heading.

Tracked-work body replacement uses the deliberately narrower form
`<!-- replace:NAME --> ... <!-- /replace:NAME -->`. It accepts `backlog`, the
legacy `pending` alias, `review`, and `icebox`. `patch:pending` is rejected;
use `replace:pending` while migrating an old document, or canonicalize the
component to `backlog`.

Patch openers currently have no behavior-changing attributes. Attribute text
is parsed for compatibility (including historical `transfer-source` metadata),
but `mode` and `max_lines` on a `patch:*` opener do not override the target
component. Put patch mechanics on the target `agent:*` opener or in project
component configuration.

Open and close prefixes and names must match. Unclosed patch blocks are
malformed closeout input. Patch-looking text in code ranges remains prose.
The retired `agent:patch:*` spelling is accepted only by a narrow retained
response replay repair; never author it.

## Binary-managed and internal markers

Do not author or retain these as configuration:

| Marker | Purpose |
|---|---|
| `<!-- agent:boundary:ID -->` | Standalone append/IPC insertion anchor, normally at the end of `agent:exchange`. It has no close marker. agent-doc removes stale anchors and writes a fresh short hexadecimal ID, optionally followed by a summary slug. |
| `<!-- status:sync --> ... <!-- /status:sync -->` | The one binary-managed synchronized-status block inside `agent:status`. Author `sync` on the component opener; do not add or duplicate this block manually. |
| `agent:salient-response` | Paired, cycle-keyed non-final checkpoint nested in `agent:exchange`; its opener carries `cycle="ID"`. Final response insertion removes it. |
| `agent:referral` | Paired pointer generated by `transfer --referral`; its opener records `src`, `component`, and `created`. |
| `agent:landability-probe`, `agent:scaffold` | In-memory validation wrappers. They must not be persisted as user document structure. |

The binary also writes lifecycle glyphs and item annotations inside managed
component bodies. Those are item syntax, not opening-marker attributes.

## Validation and fail-closed behavior

- Unknown or misplaced opening-marker attributes are preserved and ignored,
  with a component-specific warning. Strict lint mode may elevate warnings.
- Malformed attribute syntax and empty required values block a write. Bare
  flags are valid only where this reference says they are.
- Invalid queue graph identities/dependencies and invalid status `sync-mode` or `interval`
  values fail closed. An invalid schedule holds the affected queue; sibling
  queues remain independent.
- An unknown component name is different from an unknown attribute: a valid
  unregistered component is preserved through the unknown-element fallback,
  while an attribute typo never silently acquires behavior.
- Component, boundary, and response marker scanners ignore code ranges. A raw
  protocol marker in ordinary response prose can make the response unlandable;
  quote it as code.

