# Mutable decisions

Use the project-level `decisions.md` registry for a salient decision whose
current answer may change over the project lifetime. Session documents keep only
a compact link such as `[DEC-storage](decision:DEC-storage)`; queue, TODO,
notes, and prompt text may use the same link.

The registry is current authority. `decisions-archive.md` is append-only
revision evidence. Git commits and the sorted `Agent-Doc-Decision` /
`Agent-Doc-Response` trailers accelerate history lookup; a database or index
may be rebuilt from Markdown and Git and is never authoritative.

Use the compare-and-swap command flow:

```sh
agent-doc decision read --project-root . DEC-storage
agent-doc decision plan --project-root . --id DEC-storage \
  --state accepted --decision "Keep Markdown authoritative." \
  --rationale "It is reviewable and portable." \
  --evidence '[#r-0123456789ab]' --output /tmp/decision-plan.json
agent-doc decision apply /tmp/decision-plan.json --commit
agent-doc decision validate --project-root .
agent-doc decision history --project-root . DEC-storage
```

An update changes the existing record in place and appends one immutable archive
revision. Use `--state superseded|reversed|deprecated` and `--supersedes
DEC-...` when appropriate. Do not mint an implicit ID, reuse an archived ID, or
backfill queue IDs into decision IDs. A stale plan fails if either Markdown file
changed after planning.

Evidence uses canonical response IDs such as `[#r-0123456789ab]`. These IDs
pin their responses for archive GC through
`agent_doc_decision_io::pinned_response_ids`. Validation reports response IDs
that are no longer materialized and decision links whose records do not exist.

## Operator answers in a session document

You do not look operator answers up. The binary does it for you on every
preflight and every closeout (`#answerbyrequestid`):

- Each operator answer in the document is recorded under every copy of its
  question: the todo, its Waiting-on-you item, and requests raised for that
  item. Answers in the done archive count too.
- The cycle contract lists `answered_decisions`. Each entry has the operator's
  latest answer word for word, where it was given, and the open copies it
  covers. Act on that answer. Do not ask it again.
- `possibly_answered` entries are only likely matches. If one is the same
  question, ask the operator once whether it is the same. They confirm by
  replying `same as [#id]` under the item. Never merge two questions yourself.
- The binary closes duplicate request copies once their answer has been
  delivered. A closeout that asks an answered question again is refused, with
  the recorded answer. If the answer truly leaves a new question open, cite
  the decision id, or write `already answered: <answer>`, in the request and
  say what is still open.
- Only operator words change a decision. A newer operator answer becomes the
  latest revision. Earlier answers are kept, and the cycle contract shows a
  changed decision once as `decision changed: <old> -> <new>`.
- The binary writes each answered decision's id onto every open todo copy of
  the question as `⟦decision:dec-…⟧` (`#decisionidstamp`). It is a binary
  token, not prose: leave it in place, never type or copy one yourself, and
  cite the decision id in words when a request truly needs to.
- Answers that were pruned from the document before the ledger existed are
  recovered once from the document's git history. `preflight --probe` shows the
  same `answered_decisions` without writing anything.

`agent-doc decision answered <FILE> [--json]` prints the ledger for debugging.
`agent-doc decision validate --document <FILE>` also lists near-duplicate
question clusters.

## Changing, amending and linking decisions (`#decisiongraph`)

The operator changes a decision by replying under the item or any copy of
it. `change: <new answer>` replaces the old answer. `amend: <clause>` keeps
the decision and changes or adds one clause. `depends on [#id]` says this
decision assumes another; `conflicts with [#id]` says they cannot both hold.
Never write these forms yourself: they are the operator's words.

When a reply has no `change:` or `amend:`, the binary proposes the kind and
the contract says so. If it matters, ask the operator once to confirm by
starting the reply with `change:` or `amend:`.

A changed or amended entry in `answered_decisions` carries `change_impact`:
items to reopen or re-evaluate, rows that touch an amended clause, and work
already done on the old answer (closed copies, commits, responses). Act on
it: re-evaluate or reopen what it names, and ask the operator once whether
to keep or undo the work done on the old answer. An amend leaves everything
else alone.

`agent-doc decision impact <FILE> <id> [--json]` shows the same walk.
`agent-doc decision why <FILE> <id> [--json]` shows why a decision holds:
the operator's words for every revision, the evidence, and what it depends
on. `agent-doc decision graph <FILE> [--json]` prints the whole graph.
