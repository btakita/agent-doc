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
