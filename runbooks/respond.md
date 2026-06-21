# Respond / pending detail

Detail for SKILL.md Workflow **step 1 (Respond)** and **step 1b (Update
pending)**. The spine in SKILL.md keeps the reconcile-oldest-first rule, the
`### Re:` header + model-attribution format, and the pending granular-flags-only
rule. This runbook carries the rest.

## Step 1 — Respond

- Address the user's changes naturally in the console; that response is the
  document response.
- Reconcile the changed exchange tail oldest-first. Do not stop at the newest
  question; answer or group each unresolved prompt in that tail and each
  unresolved `prompt_target`; treat `content_edit` items as user corrections.
- If session-accretion supplies bounded context, use the included `### Re:`
  blocks as prompt-position anchors, not proof that older turns are absent.
- Execute from the planning record. If `execution_scope=plan_backlog_only`, stay
  in plan/backlog capture mode. Otherwise complete the requested repo work before
  persistence or stop on a blocker. Do not keep appending "starting/continuing"
  status prose while the requested work remains undone.

**Draining a free-text queue head — quote it (`#qdeferstrike`).** When you answer a
free-text queue prompt (a queue head with **no** `#id`), quote it verbatim as a
`> **Queue prompt:**` blockquote at the top of your response. The position-
independent strike (`#ftstrike` / `strike_answered_free_text_queue_heads`) matches
the head's prose against your response's **blockquote** region — a `### Re:` heading
alone is not enough, so a heading-only answer leaves the head queued and the loop
churns re-answering it forever. This matters most when the free-text head sits
*behind* a deferred `[operator-verify]`/`[focused-cycle]` id-head: the leading
consume stops at that deferred id-head, so the **only** path that can strike the
free-text item is the blockquote-matched `#ftstrike`. `do [#id]` heads are
different — they strike by id via `--done <id>` regardless of position and need no
quote.

The `#ftstrike` pass is **conservative about in-flight edits (`#qstrikeexplain`)**: it
strikes a free-text head only when that head was present in the stable pre-turn
baseline (the preflight baseline). A head that first appeared in the live buffer
*this* turn — a line the operator is still typing — is **never** same-cycle struck,
even if it happens to fuzzy-match a quoted prompt; it defers to the cycle that
actually answers it (editor-wins, consistent with `#queue-user-edit-overwrite`). So
quoting a queue head you did not intend to drain cannot strike a line the operator
is mid-authoring.

**Response header format (template mode):** use `### Re: topic` markdown headers —
**not** bold (`**Re:**`). The `(HEAD)` boundary marker requires real headings. Use
h4–h6 for sub-sections within a response.

**Model attribution:** always append the resolved model short name with a spaced
em dash: `### Re: topic — gpt-5` or `### Re: topic — opus-4-6`. Use
`preflight.agent_model` if non-null (from frontmatter); otherwise use your own
model identity. Never use the harness label (`codex`, `claude`) as the suffix, and
never omit it.

**Streaming checkpoints:** for long responses, flush partial content at natural
breakpoints; see [streaming-checkpoints.md](streaming-checkpoints.md). Prefer
`<!-- patch:exchange -->`.

**`#agent-doc-bug` plan proof:** if the prompt contract requires a plan, create
the plan file before closeout and cite every plan path. If
`execution_scope=plan_backlog_only`, create plan/backlog items and explain the
deferred implementation boundary instead of editing code.

## Step 1b — Update pending (template mode)

Mutate `<!-- agent:backlog -->` (or legacy `agent:pending`) only through granular
`agent-doc write` flags: `--pending-add`, `--done <id>`, `--pending-edit
"id=text"`, `--pending-reorder`, `--pending-gate`, `--pending-ungate`,
`--review-add`, `--review-edit`. Full-replace via `<!-- patch:backlog -->` /
`<!-- patch:review -->` is rejected; see [pending-ops.md](pending-ops.md). For
`<!-- agent:icebox -->`, use `<!-- replace:icebox -->`.

Completed/reaped items live under canonical `<!-- agent:done -->`; legacy
`agent:backlog-done` and `agent:pending-done` tags require `agent-doc migrate`.

**Pending capture rule:** if the response creates concrete follow-up work, add it
to `agent:backlog` in the same cycle. Put new items at the beginning of
`agent:backlog`; if you are extending an ordered batch already in pending, insert
the new item adjacent to its predecessor. If the item is only a recommendation,
include `[recommended]`.

**Cross-document pending rule:** if a prompt preset or user instruction names
another backlog file, add the item to that target with `--pending-add-to
<target-file> "<item>"` on the final `agent-doc finalize` command. Do not satisfy
an explicit target by running `--pending-add` against the current session
document. If the target is missing or lacks a backlog component, stop on the
binary error and report the blocker.

**Plan-backed pending items:** create the plan file first and include that exact
plan file path in the pending text. For multi-phase implementation work, prefer
one backlog ID per actionable phase (for example `#crdtrespfx1`, `#crdtrespfx2`)
instead of one parent ID that gets repeatedly `--pending-gate`d after partial
progress; keep the parent plan file as context, but queue and close out concrete
phase IDs.

**`do #id` closeout rule:** when the user directs `do #id ...`, record the pending
outcome before persistence: `--done <id>` if completed, `--pending-gate <id>` if
code-complete but awaiting review/external validation, or explain concretely why
it stays open. `session-check` enforces the `pending_done_guard`; projects may opt
into `review_done_guard` when review must precede done.

**Complete over gate.** Default to `--done` — finish the work this cycle. Gating
to `agent:review` is exceptional, only for work genuinely blocked on something the
turn cannot do (live editor/pane verify, external approval, CI outage), and the
item must name what unblocks it. Unblocked follow-up goes to `agent:backlog` as an
actionable item, not `agent:review`. Keep `agent:review` small (target < 10);
convert stale/satisfied gated items to `--done` or actionable backlog items rather
than letting them accumulate. See [pending-ops.md](pending-ops.md) for the full
review-discipline rule.
