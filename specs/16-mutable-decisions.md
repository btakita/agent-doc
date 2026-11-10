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
`evidence` (`r-<hex>` response ids, `commit:<sha>`, GitHub artifacts
`github:<owner>/<repo>/issues/<n>` or `github:<owner>/<repo>/pull/<n>`).
`principle`, `fact` and `derivation` nodes hold first-principles
derivations (below).

**Edges** read `from <kind> to`: `answers` (revision → decision),
`restates` / `amends` / `supersedes` (revision → previous revision),
`depends_on` (B assumes A), `conflicts_with`, `applies_to` (from concerns
to), `evidenced_by` (revision or decision → evidence), `copy_of`, `same_as`,
`derived_from` (derivation → premise) and `concludes` (derivation →
decision, its clause the concluded answer), and `upstream_of` (a cited
GitHub issue or pull request → the decision that exists because that
upstream is open, clause `open`); `subsumes` and `exception_to` are
reserved for `#decisionprune`. Each edge has a source: `derived`
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
confirms with `change:` or `amend:`. The operator's word outlives its row
(`#decisionrevisionsource`): each `answer_ledger_revisions` row records its
`relation_source`, and a load restores an `operator` relation onto a revision
whose row the document no longer shows (or whose row lost its lead-in), so
the cycle contract keeps `change_source: operator` instead of falling back to
a proposal. A store never replaces an `operator` relation with a `proposed` or
`derived` one; only another operator word replaces it, and a history rebuild
that recovers the operator's word fills a revision that lacked it. A database
that predates the column gains it in place, and its rows adopt the operator's
word from the graph cache (a revision node whose label is the answer and whose
revision-relation edge is `operator`). The append-only archive
(`answer_ledger_archive_revisions`, `#decisionprune`) keeps the same
`relation_source` (`#decisionrevisionsource`, archive): the write-through
backfill and every archived row copy it, an operator word for an archived row
is never lost to an earlier derived or proposed row with the same key, and a
dormant decision the document names again revives each answer with the
latest operator relation any of its archived rows recorded.
`depends on [#id]`, `assumes [#id]`,
`builds on [#id]`, `relies on [#id]` record `depends_on`; `conflicts with
[#id]`, `contradicts [#id]` record `conflicts_with`. Both ends are lifted onto
their decisions. A row that holds nothing but such phrases is a link, not an
answer. Commit SHAs and `[#r-…]` ids in an answer, or on a closed copy,
become `evidenced_by` edges.

**Closed upstream.** No second causality graph. A decision cites a GitHub
issue or pull request when its question text (choices included) contains
`github.com/<owner>/<repo>/issues/<n>` or `.../pull/<n>`, or `owner/repo#<n>`
for a known upstream with that owner, repo, and number. A bare `issue <n>`,
`issues/<n>`, `PR <n>`, or `pull request <n>` cites an upstream only when
that text does not name a different repo for the number and exactly one
explicit URL in the document, or else exactly one upstream the caller
reports closed, has that kind and number. `DecisionGraph::build` records
`upstream_of` for explicit citations. `DecisionGraph::moot_by_closed_upstream`
binds the remaining bare citations and reports every still-open decision
that cites a closed upstream as moot. A decision that does not cite it
stays. Work items are not decisions and are not mooted.

**Moot reported closeout** (`#mootdecision`). A
`subagent-operator-request-reportedcloseout-` decision is the operator
question on a `Reported` attempt. It stays in the document while that
attempt's closeout still `requires_operator`. Once no stored `Reported`
claim still asks the operator for that id, preflight and strict closeout
remove the open todo, the queue-row request annotation, the `**Request**`
row, and a generated waiting item that holds no operator text, and strike
the packet opener so a later closeout cannot record the question again. A
decision that still needs an answer stays. Any other operator request, a
local answer (`[#id]: …`), a checked todo, operator text under a waiting
item, and the underlying work item stay. An unreadable claim ledger, or an
unreadable lineage ledger while an attempt is still `Reported`, removes
nothing.

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
[--json]`, and `agent-doc decision search [FILE] <query>... [--project]
[--limit N] [--json]` (`#decisionsearchmenu`: answered decisions ranked by
exact id, decision stamp, normalized question text, then wording and optional
tsift recall across session documents; with no FILE, `#decisionsearchnofile`
searches the project's session documents and `decisions.md`; see SPEC.md). `<id>` may be a decision id, a revision id, a question copy id,
(for `why`, and for `impact` when unambiguous) an item a decision applies to,
or (for `why`) a decision only a derivation concludes.
tsift graph-db may index the cached rows as an optional traversal layer.

### First-principles derivations (`#firstprinciplesstore`)

A derivation is one inference step from premises to an answer for one
decision (the `#firstprinciples` model, `agent_doc_principles::derivation`).
It is stored in the same per-document `state.db` decision-graph tables, not
a parallel store (`agent_doc_sqlite::answer_ledger_derivations`):

- a `derivation` node, id `drv-<hex>`: `label` is the stated inference;
  `status` is `agent_derived_unreviewed`, `operator_confirmed` or `rejected`,
  with `status_at` and `status_note` (the operator's reason). These three
  columns are added to `answer_ledger_nodes` in place by their own migration;
- premise nodes `principle:<id>` and `fact:<id>` whose label is the
  statement, and decision premises as the decision's own node;
- one `derived_from` edge to each premise and one `concludes` edge to the
  decision whose `clause` is the concluded answer. Their source is `proposed`
  until the operator confirms the step, then `operator`.

**Authoring.** `agent-doc decision derive FILE --premise principle:<id>
--premise fact:<id>[=<statement>] --premise decision:<id> --inference <one
sentence> --decision <id> --answer <answer> [--id drv-…] [--json]`. A
principle premise must be in scope for FILE (global file, ancestor
`.agent-doc/project.md`, the document; nearest wins) and takes its statement;
a fact needs a statement the first time; a decision premise and `--decision`
resolve like `decision why` (decision, revision, question copy, or an item
exactly one decision applies to), and a step may not be its own premise. The
default id is a hash of the decision, the normalized answer and inference,
and the sorted premises, so authoring the same step again reports it on
record (`created: false`) with its review status kept; a different step
under an explicit id already in use is refused. The command stores the step
as `agent_derived_unreviewed`, reports live derivations of the same decision
that reach another answer (`same_premises` / `different_premises`), and
prints the operator forms.

**Operator review.** Only the operator's words change a step's status:
`confirm [#drv-…]` / `reject [#drv-…]` (optionally `: <why>`), `[#drv-…]:
confirm`, `re [#drv-…]: reject — <why>`, or a bare `confirm` / `reject:
<why>` row under a waiting item whose own id is the derivation's.
`confirmed` / `rejected` are accepted too. Such a row (queue or waiting
packet) is never an answer, question or link. When several rows review one
step, a live row outranks a struck one, which outranks an archived one, and
the later row wins among equals. Every preflight ledger pass records the
deciding review in `state.db`, so the status survives after the row is
pruned; reads overlay the reviews the document currently shows. A review
naming a step this document never stored is reported and skipped. Rejecting
a step never rejects its premises; a rejected step does not propagate
impact or count toward contradictions.

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
