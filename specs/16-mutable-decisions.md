# Mutable salient-decision registry

## Authority

A project may track `decisions.md` at its Git root. It is ordinary Markdown
and is the sole current authority for mutable salient decisions.
`decisions-archive.md` is a tracked append-only provenance ledger. Git is the
durable history database after commit. SQLite and archive indexes are optional,
rebuildable operational projections.

Each `agent:decision` block has a stable `DEC-...` ID and stores exactly one
current state (`accepted`, `superseded`, `reversed`, or `deprecated`),
the current decision and rationale, canonical response evidence IDs, an updated
timestamp, and optional superseded decision IDs. Updating a decision rewrites
that block rather than creating a second current record. IDs present in archive
history cannot be reused after deletion.

## Mutation protocol

`agent-doc decision plan` reads both tracked Markdown authorities and emits a
JSON compare-and-swap plan with their content hashes. `decision apply` takes
an exclusive project mutation lock, verifies both hashes again, appends an
immutable archive revision, and atomically replaces the current registry.
Concurrent/stale plans fail closed. `--commit` commits only the two decision
files.

Every affected decision is emitted once, sorted, as
`Agent-Doc-Decision: <id>`. Create, current-record mutation, state change,
supersede/reverse, delete, and `decision:DEC-...` reference addition/removal
all count. Evidence reference additions/removals also emit sorted, deduplicated
`Agent-Doc-Response: <id>` trailers. The ordinary session-document commit path
derives these trailers from its HEAD/current Markdown diff, so queue, TODO,
notes, and prompt references use the same commit contract.

## References and archive retention

Session documents carry compact ordinary-Markdown references:
`[DEC-storage](decision:DEC-storage)`. The link form is valid in queue, TODO,
notes, prompts, or exchange text. No hidden database or generated q-ID is
required.

Evidence IDs use the existing response lookup form
`[#r-<hex>]`. `pinned_response_ids(project_root)` is the public archive/GC
boundary: consumers read the tracked registry (or a cache proven to derive from
it) and must retain every returned response. `decision validate` scans
agent-doc Markdown and compact archives using the existing response marker
parser, and reports dangling response evidence, missing supersedes targets, and
dangling decision links.

`decision read` and direct ID lookup read the tracked registry; Git
trailer/history search is fallback, not the hot path. A rebuildable cache may
accelerate the same lookup. Scale coverage exercises 5,000 records and affected
ID/trailer extraction.
