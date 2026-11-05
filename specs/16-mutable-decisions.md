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

**First-principles chain** (`#firstprincipleswhy`,
`agent_doc_answer_ledger::graph::why_chain`). `decision why` joins that
answer with the `#firstprinciples` derivation chain
(`agent_doc_principles::derivation::Graph::why`) into one record: the ledger
fields above, flattened, plus `derived_from` (the derivation steps that
conclude the decision, each with `status`, `inference`, `answer` and
`premises`; a premise that is itself a derived decision carries its own
`because` chain, and a premise already on the chain is marked `cycle`),
`derivation_supported` (some step is not `rejected`; absent when no step
concludes it), `principles` (every in-scope principle the chain or
`depends_on` reaches, with statement, scope and source) and
`missing_principles` (principle premises no longer in scope). Principles are
the cycle contract's composed set for the document (global, project,
document). The binary states a derivation for every answered decision whose
holding revisions (the latest back through amends and restatements to the
one that superseded or began the answer) cite an in-scope principle as
`[#id]`, or that has a `depends on [#id]` link to a principle or another
decision: id `op:<latest revision id>`, the holding revisions verbatim as
the inference, the latest as the answer, status `operator_confirmed`. A
superseded answer's citations no longer count. A `depends_on` premise that
names a principle reads as kind `principle` with its statement. Stored
derivations join the same graph with their decision references resolved
onto ledger decision ids, and win over an operator-stated derivation with
the same id; an invalid one (no premises, no inference, its own conclusion
as a premise, a duplicate id) is skipped with a stderr note. A decision only
a derivation concludes still answers, with empty `question` and the live
step's answer as `current_answer`.

**CLI** (debugging and operator lookup; enforcement never depends on it):
`agent-doc decision graph FILE [--json]`, `agent-doc decision impact FILE
<id> [--revision <rev>] [--json]`, `agent-doc decision why FILE <id>
[--json]`. `<id>` may be a decision id, a revision id, a question copy id,
(for `why`, and for `impact` when unambiguous) an item a decision applies to,
or (for `why`) a decision only a derivation concludes.
tsift graph-db may index the cached rows as an optional traversal layer.
