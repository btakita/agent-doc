# Focused document extract

`agent-doc extract SOURCE TARGET` extracts one concern from an overloaded session
without making an agent manually copy managed document regions.

## Architecture contract

- **Invariant:** every selected logical node is present in exactly one document
  after a successful apply, while every unselected byte remains in its source
  component and both documents have current baselines.
- **Policy owner:** the focused-extract planner owns selector resolution,
  prompt/response adjacency, collision refusal, provenance, and idempotency.
- **Transition table:** unmatched selector -> refuse; target identity collision
  -> refuse; preview (attached or detached) -> report only; detached apply ->
  write target then
  source and checkpoint both; second write/checkpoint failure -> restore both;
  target-first crash marker -> resume by completing source removal without a
  second target append; already applied -> no-op; either document attached to a
  live editor -> refuse before mutation.
- **Evidence inputs:** parsed agent components, tracked item IDs, stable exchange
  node IDs, source-content hash, destination operation marker, frontmatter
  collaboration/security metadata, pane ownership, and durable editor liveness.
- **Reactive topology:** editor registrations remain in the existing
  document/process `Source -> Computed liveness -> write Effect -> receipt`
  graph. Focused extraction is a deliberately one-shot boundary and consumes
  the liveness projection; it does not create a parallel actor or
  poll/recompute readiness.
- **Imperative extraction audit:** selector resolution and all resulting content
  are computed once before I/O. The remaining direct calls are one-shot file
  writes, rollback, and snapshot checkpoints; they have no durable scheduling
  relationship and therefore no new reactive edge.
- **Allowed edit surfaces:** the focused-extract module, CLI routing, target
  scaffold shared with transfer, this command spec, its runbook, and regression
  tests.
- **Verification:** pure planner tests cover multi-surface ID selection,
  exchange adjacency, complete destination initialization, collision refusal,
  and operation-marker resume; CLI help and the full project check cover wiring.
- **Out of scope:** semantic-topic inference inside the mutation binary,
  cross-filesystem transactions, and live multi-document CRDT transactions.

## Interface

Preview is the default:

```bash
agent-doc extract SOURCE TARGET --ids id1,id2
agent-doc extract SOURCE TARGET --ids one-item-id
agent-doc extract SOURCE TARGET --exchange-node r:abc123
agent-doc extract SOURCE TARGET --components notes,status
agent-doc extract SOURCE TARGET --selection legal-selection.json
```

The JSON preview names the deterministic operation ID, whether the destination
would be created, every component/item/node to move, and
`"source_action": "remove_on_apply"`. Re-run with `--apply` to execute that
exact selection: the target receives the selected material first, then that
material is removed from the source. Preview never mutates either document.
Selectors can be combined.

An operator does not need to discover IDs or author a manifest. They may write
an imperative request naturally in the source document, for example, “Extract
the legal concern into `tasks/sdk/sdk-legal.md`.” That request authorizes the
agent to inspect the commingled surfaces, resolve a coherent stable selection,
run the preview, and apply it when the preview is clean and unambiguously
matches the request; no second confirmation is required. Exploratory or
question wording (“Could this be extracted?”) authorizes preview/advice only.
The agent asks a question instead of applying when the concern, destination,
or preview is ambiguous. Because an in-document request normally leaves the
source attached to its editor, the agent previews immediately but must not
claim the extraction completed: it presents the exact already-authorized
`--apply` retry command, asks the operator to close the source and target tabs,
and retries after detachment. That safety handoff is not a second semantic
confirmation.

For a commingled session, an agent first inspects the source (`outline`,
`exchange list`, and the managed work components), decides which pieces form a
coherent concern, and writes a selection manifest:

```json
{
  "focus": "SDK legal and compliance",
  "rationale": "Keep counsel decisions with their originating turns and tracked work.",
  "ids": ["tdminorscompliance", "prelinkid2"],
  "exchange_node_ids": ["r:abc123"],
  "components": []
}
```

`--selection` cannot be combined with direct selector flags. The semantic
judgment therefore stays agent-assisted and reviewable, while the mutation is
still a deterministic resolution of stable selectors. `focus` and `rationale`
are included in the preview and its operation ID, so the operator can review
both why and what before `--apply`.

This selector-bearing form extends, rather than replaces, the existing command:
`agent-doc extract SOURCE TARGET [--component NAME]` still immediately moves the
last exchange entry exactly as before. The plural `--components` flag selects
whole components for the focused refactoring mode and avoids changing the
legacy singular flag. A separate top-level `split` command is intentionally not
added: `extract` matches extract-method refactoring terminology, while “split”
already names tmux/layout concepts such as `layout --split`.

The legacy no-selector form requires `TARGET` to exist and already be an agent
document, proven by `agent_doc_session` / `agent_doc_format` /
`agent_doc_write` frontmatter or at least one managed `agent:*` component. It
refuses a plain Markdown target before mutating `SOURCE`; this prevents an
unrelated plan with a matching basename from being mistaken for the extracted
session. Use `agent-doc init TARGET` first, or use focused selectors when the
command should initialize a new target.

Tracked IDs are resolved across `agent:queue`, `agent:todo`, `agent:backlog`
(including legacy `agent:pending`), `agent:review`, and `agent:icebox`. Exchange
nodes that mention a selected ID move with it. Selecting one exchange prompt or
response also carries its adjacent response or prompt so the extracted history
does not become an orphaned half-turn. Whole-component selectors support
`status`, `exchange`, `queue`, `backlog`, `todo`, `review`, `icebox`, and `notes`.
Selected tracked item blocks retain their stable ID, checkbox/gate state, text,
and indented continuation bytes exactly.

An absent destination is initialized as a complete template document with a new
session ID and the source collaboration/security-review, model,
linked-resource, tool-search, and prompt-preset context (but no resume/routing
identity). Relative linked-resource paths are rebased for the destination
directory. The target records `agent-doc-extract` provenance; the source
records an `agent-doc-extract-destination` pointer. These markers make recovery
discoverable and prevent a target-first crash from duplicating content on retry.

Apply refuses while either document has live editor authority. Cross-document
atomic compare-and-swap does not exist across independent CRDT actors, so a live
focused extraction must never degrade to direct disk writes. Close both tabs
(or otherwise detach them), inspect the preview again, and apply.
`--bypass-claim` bypasses only the tmux pane-ownership check, never the
editor-authority gate.
