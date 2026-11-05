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

## Session decision graph (`#decisiongraph`)

The per-document answer ledger (`#answerbyrequestid`, crate
`agent-doc-answer-ledger`) is projected onto a typed graph
(`agent_doc_answer_ledger::graph`). It is not a parallel store: the session
document and its done archives stay the authority, and the graph is rebuilt
on every preflight and strict closeout ledger pass, then cached in the same
`state.db` index (`answer_ledger_nodes`; `answer_ledger_edges` with `source`
and `clause` columns, added in place to older databases). Cached rows the
document no longer produces are merged back as history (`in_document:
false`); for a revision pair the current build's relation wins over a cached
one of another kind.

**Nodes.** `question` (a todo, waiting or request copy that asks the
operator, `copy_of` its decision), `decision`, `revision` (one per distinct
operator answer, id `<decision>@<hash>`), `item` (work the decisions apply
to; a work todo nobody asked about is an item, never a decision), and
`evidence` (`r-<hex>` response ids, `commit:<sha>`). `principle`, `fact` and
`derivation` are reserved for `#firstprinciples`.

**Edges** read `from <kind> to`: `answers` (revision → decision),
`restates` / `amends` / `supersedes` (revision → previous revision),
`depends_on` (B assumes A), `conflicts_with`, `applies_to` (from concerns
to), `evidenced_by` (revision or decision → evidence), `copy_of`, `same_as`;
`derived_from`, `subsumes` and `exception_to` are reserved for
`#firstprinciples` and `#decisionprune`. Each edge has a source: `derived`
(read off document structure), `operator` (the operator's explicit words), or
`proposed` (the binary's classification, standing until the operator confirms
or corrects it). An `amends` edge carries the amended clause.

**Operator forms.** A reply under any copy of the question (or a queue row
`[#id]: …`) that starts with `change:` (also `changed:`, `reverse:`,
`supersede:`) records a superseding revision; `amend:` records an amending
one whose clause is the reply text. The lead-in is stripped before the answer
is hashed, so adding it to an earlier reply confirms that same revision.
Without a lead-in the binary classifies the revision (`answer_relation`) as
`proposed`, and the cycle-contract guidance tells the agent the operator
confirms with `change:` or `amend:`. `depends on [#id]`, `assumes [#id]`,
`builds on [#id]`, `relies on [#id]` record `depends_on`; `conflicts with
[#id]`, `contradicts [#id]` record `conflicts_with`. Both ends are lifted onto
their decisions. A row that holds nothing but such phrases is a link, not an
answer. Commit SHAs and `[#r-…]` ids in an answer, or on a closed copy,
become `evidenced_by` edges.

**Impact walk** (`DecisionGraph::impact`). Dependents of a decision are its
copies, items tied to it or its copies by `applies_to` in either direction,
and decisions or items that `depends_on` it, transitively (each row names the
decisions walked). For a `supersedes` revision every open dependent is
flagged (`flag`), every closed one is to be reopened (`reopen`), and closed
copies plus the evidence of the replaced revisions are listed as
`actions_taken` for the operator to keep or undo. For an `amends` revision
only open dependents whose text touches the clause (a shared id, a shared
subject anchor, or two shared content tokens) are flagged
(`review_clause`); the rest are counted as unaffected and nothing is
reopened. `restates` and `initial` disturb nothing. Each `answered_decisions`
contract entry whose decision changed or was amended carries
`change_source`, `amends` + `amended_clauses`, and `change_impact` lines; the
binary reports the walk and never reopens anything itself.

**Why** (`DecisionGraph::why`): every revision oldest first with the
operator's verbatim words, location, first-seen time, relation and its
source, clause and evidence; the transitive `depends_on` premises with their
current answers; conflicts, copies, applied items and `same_as` merges.

**CLI** (debugging and operator lookup; enforcement never depends on it):
`agent-doc decision graph FILE [--json]`, `agent-doc decision impact FILE
<id> [--revision <rev>] [--json]`, `agent-doc decision why FILE <id>
[--json]`. `<id>` may be a decision id, a revision id, a question copy id, or
(for `why`, and for `impact` when unambiguous) an item a decision applies to.
tsift graph-db may index the cached rows as an optional traversal layer.

### Project registry in the graph (`#decisionregistrygraph`)

`decision graph|impact|why` also load the `decisions.md` registry at the
document's project root (`agent_doc_answer_ledger::graph::registry`, loaded
by `agent_doc_preflight_io::decision_registry_graph`). Each record is a
`decision` node with its verbatim `DEC-…` id and one `revision`
`DEC-…@<hash>` holding its current text (location `decisions.md (<state>,
updated <timestamp>)`), `evidenced_by` its response evidence (the stored
`[#r-<hex>]` form unbracketed, so it meets the evidence node a ledger answer
citing the same response produces). A record's `Supersedes:` list becomes
decision-level `supersedes` edges `DEC-new → DEC-old` (source `derived`); a
dangling target adds nothing (`decision validate` reports it). A session
item whose text cites a record (`[DEC-x](decision:DEC-x)`, read with the
same parser as `decision validate`) `applies_to` it, and a bracket id
`[#DEC-x]` (read by the ledger as the lowercased placeholder `dec-x`, e.g. in
`depends on [#DEC-x]`) is folded onto the record node.

The impact of a record that supersedes others is a `supersedes` change from
the first replaced record's text (`superseded_decisions` lists all of them):
the dependents of the record and of every record it replaces are flagged or
reopened as for any supersede (a replaced record's dependents name it first
in `via`), and the replaced records' evidence is listed in `actions_taken`.
The replaced record's own impact stays `initial`. `why` adds `supersedes`
(each replaced record with its text) and `superseded_by`.

The overlay is applied on read only: preflight and closeout neither load nor
cache it, so the registry stays the authority and a removed record never
lingers as cached graph history. A missing registry is a no-op; an
unparsable one is reported on stderr and skipped.
