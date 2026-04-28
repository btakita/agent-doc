# agent:queue Implementation Plan

## Overview

Add an `agent:queue` document component that lets users batch multiple prompts in one block. The queue is consumed sequentially — each prompt triggers a full preflight→respond→persist cycle.

## Design Decisions

- **Hybrid syntax (Option A+B):** single-line prompts use bare `- ` list items (Option A). Multi-line prompts use `~~~prompt` fences (Option B). The parser distinguishes them by whether the line starts with `- ` or `~~~prompt`.
- **`---` shorthand:** accept `---` as an alias for `~~~prompt` / `~~~` inside `<!-- agent:queue -->` boundaries only. Outside queue boundaries, `---` retains its standard markdown meaning (thematic break / frontmatter delimiter).
- **Exchange-triggered execution (default):** the queue does not self-trigger by default. The user writes `do queue` or `run queue` in the exchange to start consumption. A persisted `queue_active: true` flag tells preflight to keep consuming on subsequent cycles without needing another exchange prompt. The flag clears when the queue drains to empty.
- **Self-triggering (opt-in):** two mechanisms to enable auto-start without an exchange prompt:
  1. **Element attribute:** `<!-- agent:queue auto -->` — the `auto` attribute tells preflight to begin consuming immediately when the queue is non-empty. No `do queue` needed.
  2. **Inline start fence:** a `~~~start` or `--- start` fence inside the queue that acts as an activation signal. When preflight sees a start fence, it activates the queue and removes the start fence, then begins consuming the remaining items. This lets users compose a one-shot "start" signal directly in the queue block.
- **Trigger precedence:** `auto` attribute > inline start fence > exchange `do queue`. If `auto` is set, the queue always starts on non-empty. If not, an inline start fence activates once (consumed on first cycle). Otherwise, exchange `do queue` is required.
- **Scheduled start (`--- start at <datetime>`):** the start fence can carry a time argument. Preflight parses the datetime, compares against the current time. If the time hasn't arrived, it emits `queue_deferred: true, queue_start_at: "<iso8601>"` and skips activation. Once the time passes, it activates normally and consumes the fence. Requires a polling mechanism (harness loop or cron trigger) to check periodically.
- **`auto` is one-shot:** when the queue drains to empty, `auto` is stripped from the opening comment (`<!-- agent:queue auto -->` → `<!-- agent:queue -->`). To re-run, the user re-adds `auto` or uses `do queue`.
- **Per-item removal:** each completed item is removed from the `agent:queue` block immediately after its cycle commits. The queue shrinks visibly as it runs.
- **Live editing:** the user can add, remove, or reorder items in the `agent:queue` block between cycles. Preflight re-reads the queue from the document on each cycle, so edits between cycles are picked up naturally.
- **Stop-on-change (not debounce):** if the next item to consume has changed since the previous cycle's snapshot, the queue halts immediately — no waiting, no debounce. Running an incomplete prompt is worse than stopping. The user explicitly restarts the queue (`do queue` or re-add `auto`) after finishing their edit. This avoids the unreliable "is the user still typing?" detection problem entirely.
- **Inline stop fence (`--- stop`):** a `--- stop` or `~~~stop` fence inside the queue acts as a breakpoint. When the consumer reaches it, the queue halts (strips `auto`, clears `queue_active`). The stop fence is consumed (removed) so it doesn't block on restart. Items after the stop fence remain queued for the next activation. This lets users batch items into groups with review gates between them.
- **Per-item time gates:** a `--- start <time>` fence placed between queue items acts as a time gate. Items before it run immediately; the queue pauses at the fence until the specified time. Multiple time gates create a scheduled pipeline. Semantically equivalent to `--- stop` + `--- start <time>` combined. Both `--- start <time>` and `--- start at <time>` are accepted. Items inserted before a time gate run before it — sequential position determines execution order.
- **Per-item orchestration:** each queue item can carry its own preset or orchestration metadata (e.g., `#spec-test-build-install-commit-push`), so per-item orchestration flows through the normal cycle for free.

## Syntax

### Exchange-triggered (default)

```markdown
<!-- agent:queue -->
- /agent-doc tasks/plan.md
- do #fix1. preset #spec-test-build-install-commit-push
---
Review the changes in src/agent-doc/src/frontmatter.rs
and check for edge cases with empty body content.
Then run cargo test.
---
<!-- /agent:queue -->
```

User writes `do queue` or `run queue` in the exchange to start.

### Self-triggering via attribute

```markdown
<!-- agent:queue auto -->
- do #fix1
- do #fix2
- run tests
<!-- /agent:queue -->
```

Preflight detects `auto`, activates immediately when queue is non-empty.

### Self-triggering via inline start fence

```markdown
<!-- agent:queue -->
--- start
Start processing these items now.
---
- do #fix1
- do #fix2
<!-- /agent:queue -->
```

The `--- start` / `~~~start` fence is consumed on first cycle, activating the queue. Remaining items process normally.

### Scheduled start

```markdown
<!-- agent:queue -->
--- start at 17:00 ET
- run nightly test suite
- generate coverage report
<!-- /agent:queue -->
```

Preflight checks `now >= 17:00 ET`. If not yet, emits `queue_deferred: true` and skips. Once the time arrives, activates and consumes the fence. Pairs with `/loop /agent-doc <file>` or a cron trigger to poll periodically.

### Stop fence (review gate)

```markdown
<!-- agent:queue auto -->
- do #fix1
- do #fix2
--- stop
- do #fix3
- run full test suite
<!-- /agent:queue -->
```

Queue processes `#fix1` and `#fix2`, then hits `--- stop` and halts. The stop fence is removed. To continue with `#fix3` and the test suite, re-add `auto` or `do queue`.

### Per-item time gates (scheduled pipeline)

```markdown
<!-- agent:queue auto -->
- do #fix1
- do #fix2
--- start 17:00 ET
- run nightly tests
--- start 18:00 ET
- generate coverage report
<!-- /agent:queue -->
```

`#fix1` and `#fix2` run immediately. Queue pauses at the 17:00 fence. At 17:00, resumes with nightly tests. Pauses again at 18:00. At 18:00, runs the coverage report. Each time fence is consumed when its time arrives.

Items added before a time fence run before the pause:

```markdown
<!-- agent:queue auto -->
- do #fix1
- do #fix2
- do #fix3          ← newly inserted, runs before 17:00 gate
--- start 17:00 ET
- run nightly tests
<!-- /agent:queue -->
```

**Parsing rules:**
1. Lines starting with `^- ` at column 0 → single-line prompt
2. `^~~~prompt$` or `^---$` opens a multi-line prompt fence
3. The matching closer (`~~~` or `---`) ends that prompt
4. `^--- start(\s+(at\s+)?.+)?$` / `^~~~start$` → start fence (consumed on activation); optional `<datetime>` or `at <datetime>` defers until scheduled time; can appear at head (initial activation) or mid-queue (time gate)
5. `^--- stop$` / `^~~~stop$` → stop fence (consumed when reached, halts queue)
6. Blank lines between prompts are ignored
7. Content outside list items, fences, or control fences is a parse error

## Implementation Phases

### Phase 1: Parser + data model

- Add `QueueComponent` to the document model (parallel to `PendingComponent`)
- Implement parser for the hybrid syntax: `- ` list items + `~~~prompt`/`---` fences
- Add `<!-- agent:queue -->...<!-- /agent:queue -->` to the default template scaffold (in `claim.rs` empty-file scaffold, `claim.rs` component scaffold, and `sync.rs` scaffold) — placed between `agent:exchange` and `agent:pending`
- Unit tests for: single-line, multi-line with `~~~prompt`, multi-line with `---`, mixed, empty queue, parse errors, `--- start`/`--- start <time>`, `--- stop`, multiple time gates

### Phase 2: Preflight integration (trigger resolution)

- Detect `agent:queue` in preflight
- Resolve activation mode (priority order):
  1. Check for `auto` attribute on `<!-- agent:queue auto -->` → immediate activation
  2. Check for `--- start` / `~~~start` fence in queue content → if no `at` argument, consume and activate; if `at <datetime>`, defer until scheduled time
  3. Check for `do queue` / `run queue` in the exchange → activate
- On activation, set `queue_active: true` in the document or session state
- When `queue_active` and queue is non-empty, emit `queue_prompts: [...]` in preflight JSON
- First prompt becomes the effective user edit for the cycle
- Remaining prompts stay queued
- When queue is empty, clear `queue_active` flag
- When queue drains: strip `auto` from the opening comment, clear `queue_active` flag

### Phase 3: Consumption + dispatch (looping)

- After successful commit, remove the consumed prompt from the `agent:queue` block in the document
- Snapshot the remaining queue items after each removal (for change detection on next cycle)
- Harness loops back automatically while `queue_active: true` — no per-item exchange prompt needed
- Next preflight cycle re-reads the queue from the document. Before consuming, check for control fences at the head:
  - **`--- stop`:** halt the queue (strip `auto`, clear `queue_active`), consume the fence, emit `queue_halted: "stop_fence"`. User must restart explicitly.
  - **`--- start <time>` (time gate):** if `now < time`, emit `queue_deferred: true, queue_start_at: "<iso8601>"` and skip this cycle — queue stays active but paused. If `now >= time`, consume the fence and proceed to the next item.
  - **Prompt item unchanged:** consume normally
  - **Prompt item changed:** halt the queue (strip `auto`, clear `queue_active`), emit `queue_halted: "item_modified"`. User must restart explicitly.
  - **New items appended:** not a halt — only the next-to-consume item triggers change detection
- Track consumption state so crashes don't re-run completed prompts
- Queue drains until empty → strip `auto` attribute if present → clear `queue_active` → looping stops

### Phase 4: CLI scaffolding

- `agent-doc queue add <FILE>` — append a prompt (wraps in fence if multi-line)
- `agent-doc queue list <FILE>` — show queued prompts
- `agent-doc queue clear <FILE>` — empty the queue

## Open Questions

- Error handling: if prompt N fails, should N+1 still run?
- Where to persist `queue_active` — frontmatter field, session state file, or inline comment?
- Should the inline start fence content (for bare `--- start` without `at`) be injected as context for the first queue item, or purely discarded?
- Scheduled start datetime formats: support natural language (`17:00 ET`, `tomorrow 9am`, `in 2 hours`) or require ISO 8601 only?
- Should `--- start at <past-time>` activate immediately (time already passed) or error?
