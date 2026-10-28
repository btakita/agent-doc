# Focused document extract

`agent-doc extract SOURCE TARGET` extracts one concern from an overloaded session
without making an agent manually copy managed document regions.

## Architecture contract

- **Invariant:** every selected live logical node is present in exactly one
  document after a successful apply, while every unselected byte remains in its
  source component and both documents have current baselines. Compacted exchange
  archives are immutable history: selecting an archived response copies that
  bounded context into the destination with its canonical archive locator and
  leaves the archive unchanged.
- **Policy owner:** the focused-extract planner owns selector resolution,
  prompt/response adjacency, collision refusal, provenance, and idempotency.
- **Transition table:** unmatched selector -> refuse; target identity collision
  -> refuse; preview (attached or detached) -> report only; detached apply ->
  write target then
  source and checkpoint both; second write/checkpoint failure -> restore both;
  target-first crash marker -> resume by completing source removal without a
  second target append, but only after proving the marked target still holds
  every selected node; marked target that lost a selected node -> refuse with
  no documents changed (see "Marked-target holding check"); already applied ->
  no-op, but only after the target is verified against the earlier
  operation's recovery copy (see "Stripped-source re-run check"); stripped
  source whose target lost a recorded node -> refuse with no documents
  changed; either document attached to a
  live editor without coordination -> emit the deterministic preview, then
  refuse before mutation with the exact attached path(s) and explicit
  confirmation that the target was not created (or was left unchanged); live
  JetBrains editor plus `--coordinate-live-editors` -> persist the exact visible
  revisions, freeze the tabs read-only, wait for the controller-owned live CRDT
  replica counts (not durable open-tab registrations) to reach zero, reject any
  unexpected registration or replica at the mutation boundary, compare-and-swap
  each frozen controller canonical under a zero-live-replica fence, materialize
  disk from that canonical, reload the result, restore editability, and
  re-register. Disk-only force writes are forbidden for this transition: a
  nominal disk apply that leaves controller canonical unchanged would let resume
  bootstrap the pre-extraction text and report `projection_changed` on the next
  save. If disk materialization fails after a canonical advance, the binary
  restores that document's exact pre-write canonical before running the existing
  two-document rollback. Selected exchange nodes never carry their source
  document's trailing boundary: the source retains that boundary and the moved
  nodes are inserted before the target's existing boundary, preserving one
  document-local boundary on each side. A lost freeze receipt still schedules
  compensating resume; duplicate resume after success is an idempotent
  acknowledgement, while a resume carrying another operation ID is refused.
- **Evidence inputs:** parsed agent components, tracked item IDs (including
  `agent:done`), stable live exchange node IDs, compact archive locators,
  source-content hash, destination operation marker, frontmatter
  collaboration/security metadata, pane ownership, durable editor registrations,
  exact frozen endpoint identities, and controller CRDT relay membership counts.
- **Reactive topology:** editor registrations remain in the existing
  document/process `Source -> Computed liveness -> write Effect -> receipt`
  graph, while replica register/deregister events drive the controller relay
  membership used by `admin inspect`. Focused extraction is a deliberately
  one-shot CLI boundary. Because the controller exposes no replica-membership
  subscription, it uses a bounded RPC poll of that typed projection during the
  five-second freeze handoff; it does not create a parallel authority owner.
- **Imperative extraction audit:** selector resolution and all resulting content
  are computed once before I/O. The old poll of durable open-tab liveness is not
  a detach signal and is removed. Remaining direct calls are the bounded
  cross-process membership observation, zero-member-fenced canonical CAS plus
  its exact disk projection, rollback, and snapshot checkpoints; they have no
  durable scheduling relationship and therefore no new reactive edge. Replaying
  an already-applied canonical target is an idempotent no-op, so the operation
  marker and source removal still commit exactly once.
- **Routing boundary:** extraction never claims or starts the destination. A
  source document normally already owns the invoking pane, and silently
  rebinding that pane to the new destination would orphan the source session.
  To work the extracted document immediately, provision its routing explicitly
  with `agent-doc claim TARGET --new-pane` (or start it through the normal
  routing workflow) after apply succeeds.
- **Target harness inheritance:** a newly scaffolded destination records the
  source document's effective harness. An explicit source `agent:` value is
  preserved; when the source predates `agent:`, the canonical
  `Frontmatter::active_resume_harness()` compatibility rule selects Claude.
  Extraction never substitutes the harness running the command or a separate
  hardcoded default.
- **Target layout (`#extractlayout`):** a newly scaffolded destination, and any
  component section extraction later appends to a document (including the
  source's `agent:notes` pointer section), heads each component with a
  level-two heading on the same line as its opening marker, in the default
  template order (`## Status <!-- agent:status patch=replace -->`,
  `## Exchange <!-- agent:exchange patch=append -->`,
  `## Run <!-- agent:run priority subagents clear -->`,
  `## Todo <!-- agent:todo priority -->`,
  `## Backlog <!-- agent:backlog priority queue -->`, `## Review`, `## Icebox`,
  `## Done`, `## Notes`). The runnable-work component is `agent:run` headed
  `## Run`, never `## Queue`; a runnable section extraction adds is always the
  canonical `agent:run`, and an existing `agent:run` satisfies a request for
  the legacy `queue` component. When the source already heads a component on
  its opener line (`## Run: Main <!-- agent:run ... -->`,
  `## Parking Lot <!-- agent:icebox -->`), the generated section reuses that
  heading text verbatim (the legacy `agent:queue` keys as `agent:run` and
  `agent:pending` as `agent:backlog`); a heading on its own line above a
  source opener is prose and never renames anything. Extraction never rewrites
  a heading that already exists in either document. Through 0.35.558 the
  transfer/extract scaffold kept its own copy of the template with every
  heading on its own line above the opener and `agent:run` headed `## Queue`,
  after the claim/sync scaffold had moved to same-line `## Run` headings.
- **Allowed edit surfaces:** the focused-extract module, CLI routing, target
  scaffold shared with transfer, this command spec, its runbook, and regression
  tests.
- **Marked-target holding check (GH #324):** the operation ID hashes the
  source bytes, so a re-run on an unchanged source re-derives the same
  destination marker. The marker therefore proves only that this operation
  once wrote the target, not that the target still holds what it moved. When
  the target is already marked, the planner removes a selected node from the
  source only if the target still holds it: an exchange turn by its stable
  node ID in the target `agent:exchange`; a tracked item by its ID in the same
  target component or in target `agent:done` (a completed item); an
  ID-less whole component by its moved bytes; an archived-context copy by its
  provenance marker. Sibling items that share the selected ID never vouch for
  one another. Any missing node fails the whole plan with the typed
  `FocusedExtractTargetLostNodes` refusal, whose message names the operation,
  the target, and each missing `component:id`, and states that no documents
  were changed. The CLI records the condition in the destination's ops log as
  `focused_extract_target_lost_nodes operation=<id> target=<path>
  node_ids=<component:id,...>`.
- **Recovery copy:** before an apply mutates either document, it writes every
  byte the operation removes from the source, labelled per transfer
  (component, mode, item IDs, node IDs), to
  `.agent-doc/archives/extract/<operation>.md` under the source's project root.
  If that write fails, apply refuses before any document changes. The
  subdirectory keeps these copies out of the compact-archive index, which reads
  only the top level of `.agent-doc/archives`. An archived-context copy removes
  nothing from the source and adds nothing to the recovery copy.
- **Stripped-source re-run check (GH #324 follow-up):** once an apply has
  stripped the source, a re-run hashes different source bytes, derives a new
  operation ID, finds no marker for it in the target, and moves nothing.
  Selector coverage alone (each selected ID mentioned in some target list or
  exchange turn) cannot answer "already applied": a surviving `[#id]` list
  item would vouch for lost exchange turns that share the ID. Before reporting
  `already_applied`, the planner therefore collects every earlier operation ID
  of this source/target pair — from the source's
  `agent-doc-extract-destination` pointers (which survive the strip) and the
  target's `agent-doc-extract` markers — and loads each one's recovery copy
  from `.agent-doc/archives/extract/<operation>.md`. Every recorded section the
  current selectors select (a recorded item ID or node ID is selected, or the
  removed bytes mention a selected ID) must still be held by the target under
  the marked-target holding rules, using the section's recorded node IDs and
  item IDs. The first operation, in recorded order, with a missing node fails
  the plan with the same typed `FocusedExtractTargetLostNodes` refusal naming
  that earlier operation, each missing `component:id`, and the recovery copy to
  restore from; the ops-log line gains `recovery_copy=<path>`. Sections the
  current selectors do not select never block the re-run. An extract that
  predates recovery copies (no copy on disk) keeps the selector-coverage
  answer.
- **Verification:** pure coordination tests cover replica-count wait/timeout and
  mutation-boundary rows. Relay tests prove the detached CAS rejects a raced
  live replica, advances a zero-member canonical, and treats an exact replay as
  a no-op; realtime tests prove disk and the resume bootstrap expose that same
  canonical without creating force-disk conflict lineage. Editor tests cover
  idempotent same-operation resume and stale-operation refusal. Planner tests
  cover multi-surface and completed-item ID selection, exchange adjacency,
  source/target boundary identity preservation, archive-locator resolution after
  compaction, complete destination initialization, collision refusal,
  operation-marker resume, refusal when a marked target lost exchange turns or
  an item (while a target-completed item still resumes), a recovery copy
  that holds exactly the removed bytes and whose sections round-trip each
  transfer's labels, and the stripped-source re-run: refusal when the target
  lost recorded turns or a recorded item (also when the target lost its
  operation marker), `already_applied` for a holding or target-completed
  target, losses under an unselected operation ignored, and the legacy
  selector-coverage answer without a recovery copy. A CLI test reproduces the
  GH #324 state (marked target without the moved turns, source unchanged) and
  proves the re-run leaves both documents byte-identical and logs the loss; a
  second CLI test strips the source by applying, drops the turns from the
  target while its `[#id]` items survive, and proves the re-run refuses instead
  of reporting `already_applied`, names the recovery copy, leaves both
  documents byte-identical, and logs the loss; CLI help and the full project
  check cover wiring.
- **Out of scope:** semantic-topic inference inside the mutation binary,
  cross-filesystem transactions, and simultaneous multi-document CRDT commits.

## Interface

Preview is the default:

```bash
agent-doc extract SOURCE TARGET --ids id1,id2
agent-doc extract SOURCE TARGET --ids one-item-id
agent-doc extract SOURCE TARGET --exchange-node r:abc123
agent-doc extract SOURCE TARGET --archive-locator archive:.agent-doc/archives/hash.md#2
agent-doc extract SOURCE TARGET --components notes,status
agent-doc extract SOURCE TARGET --selection legal-selection.json
agent-doc extract SOURCE TARGET --selection legal-selection.json --apply --coordinate-live-editors
```

The JSON preview names the deterministic operation ID, whether the destination
would be created, every component/item/node to move, and
`"source_action": "remove_on_apply"`. Re-run with `--apply` to execute that
exact selection: the target receives the selected material first, then that
material is removed from the source. Preview never mutates either document.
Its terminal notice names the exact target and says whether it was not created
or was left unchanged, so another same-named Markdown file cannot be mistaken
for the extraction result.
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
source attached to its editor, the agent previews immediately and then uses
`--apply --coordinate-live-editors` when every attached endpoint advertises the
coordination capability. The plugin saves the exact visible revisions, makes
the affected tabs temporarily read-only, and fences automatic re-registration
while the CLI owns detached disk authority. No tab close or GUI automation is
required. An older or non-JetBrains endpoint fails closed and names the required
capability; closing the tabs remains the compatibility fallback.

For a commingled session, an agent first inspects the source (`outline`,
`exchange list`, and the managed work components), decides which pieces form a
coherent concern, and writes a selection manifest:

```json
{
  "focus": "SDK legal and compliance",
  "rationale": "Keep counsel decisions with their originating turns and tracked work.",
  "ids": ["tdminorscompliance", "prelinkid2"],
  "exchange_node_ids": ["r:abc123"],
  "archive_locators": ["archive:.agent-doc/archives/hash.md#2"],
  "components": []
}
```

`--selection` cannot be combined with direct selector flags. `archive_locators`
accepts the stable `archive:...#turn` values printed by `response-toc`; the
alias `exchange_archive_locators` is also accepted. The semantic
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

Tracked IDs are resolved across `agent:run`, `agent:todo`, `agent:backlog`
(including legacy `agent:pending`), `agent:review`, `agent:icebox`, and inline
`agent:done`. Dated done entries retain their archive date, text, and indented
continuations. Exchange nodes that mention a selected ID move with it. Selecting
one exchange prompt or response also carries its adjacent response or prompt so
the extracted history does not become an orphaned half-turn. Whole-component
selectors support `status`, `exchange`, `queue`, `backlog`, `todo`, `review`,
`icebox`, `done`, and `notes`.

When compaction removes a live exchange node after a selection was saved, the
planner searches the source document's derived compact-archive index for the
same content-stable node ID. The preview records the resolved archive locator
and uses transfer mode `archived_context_copy`; saved prompt-node and
response-node selectors both resolve through the assistant response's locator,
and retain their adjacent prompt/response pair. An explicit `archive_locators`
selector resolves the same pair. Apply embeds the selected archived pair in the
target exchange between provenance markers, while the canonical compact archive
remains immutable. A locator owned by another source document, a non-assistant
turn, or a missing turn is rejected before either document is written.
Selected tracked item blocks retain their stable ID, checkbox/gate state, text,
and indented continuation bytes exactly.

An absent destination is initialized as a complete template document with a new
session ID and the source collaboration/security-review, model,
linked-resource, tool-search, and prompt-preset context (but no resume/routing
identity). That context is the source document's OWN frontmatter
(`#extractpresetcopy`), never its effective frontmatter: project defaults from
every visible `.agent-doc/project.md` already apply to the destination at read
time, so copying the merged view would materialize every project preset (and
project `model`/link defaults) as per-document overrides. Presets declared
only in the source's frontmatter are real overrides and are kept; a destination
under the same project roots therefore usually carries no `prompt_presets` at
all. The same rule holds for `transfer`'s auto-created target. Relative linked-resource paths are rebased for the destination
directory. The target records `agent-doc-extract` provenance; the source
records an `agent-doc-extract-destination` pointer. These markers make recovery
discoverable and prevent a target-first crash from duplicating content on retry.

Apply refuses while either document has live editor authority unless
`--coordinate-live-editors` is supplied. Cross-document atomic compare-and-swap
does not exist across independent CRDT actors, so coordination first persists
and freezes each capable editor, waits for detached authority, verifies that the
previewed generations reached disk, and only then performs the existing
target-first transaction. IPC freeze/resume requests use the exact document path
advertised by each editor registration (after verifying that it resolves to the
requested source or target), so relative CLI paths and editor-absolute paths
cannot miss one another at the plugin boundary. Every exit path attempts resume;
a failed resume leaves the tab read-only and reports explicit plugin
reload/restart recovery instructions rather than risking an editor overwrite.
An attached-editor `--apply` attempt still prints the deterministic preview,
then fails nonzero with the exact attached source/target path(s), states that no
documents changed, and states whether the intended target was not created or
left unchanged.
`--bypass-claim` bypasses only the tmux pane-ownership check, never the
editor-authority gate or the coordinated freeze capability check.
The ownership check is read-only: neither preview nor apply auto-claims or
starts the destination. This remains true when apply initializes a new target.
