# Transfer & Extract

Move content between agent-doc session documents.

## Focused extract (one concern across surfaces)

Use focused `extract` when a large session mixes concerns and one focused document needs
the matching exchange history and tracked work together. Preview is the default:

```bash
agent-doc extract tasks/sdk.md tasks/sdk/sdk-legal.md --ids tdminorscompliance,prelinkid2
agent-doc extract tasks/sdk.md tasks/sdk/sdk-legal.md --ids tdminorscompliance
agent-doc extract tasks/sdk.md tasks/sdk/sdk-legal.md --exchange-node r:abc123 --components notes
agent-doc extract tasks/sdk.md tasks/sdk/sdk-legal.md --archive-locator archive:.agent-doc/archives/hash.md#2
agent-doc extract tasks/sdk.md tasks/sdk/sdk-legal.md --selection legal-selection.json
```

Inspect the JSON plan, then add `--apply`. The preview's
`"source_action": "remove_on_apply"` makes the move semantics explicit:
preview changes nothing; apply writes the target first and then removes the
listed live material from the source. IDs select matching queue, todo, backlog,
review, icebox, and inline done item blocks plus exchange turns that cite those
IDs. Each moved item keeps its stable ID, checkbox/gate state, text, and
indented continuations exactly; dated done entries also keep their archive date.
Use `agent-doc exchange list SOURCE` to obtain stable exchange node IDs. A whole
component can be selected with repeatable/comma-separated `--components`.
If that node has since been compacted, extraction resolves its content-stable ID
through the source archive index and records the resulting `archive:...#turn`
locator in the preview. `--archive-locator` selects such archived context
directly. Archived context is copied with provenance because compact archives
are immutable canonical history; selected live content is still moved.

The operator may ask naturally inside the source document, for example,
“Extract the legal concern into `tasks/sdk/sdk-legal.md`.” They do not need to
supply IDs or a manifest. An imperative extraction request authorizes the agent
to resolve the coherent selection, inspect the deterministic preview, and run
`--apply` when that preview cleanly and unambiguously matches the request; do
not ask for a second confirmation. Exploratory/question wording authorizes
preview or advice only. If the concern, destination, or preview is ambiguous,
ask a focused question before applying. An in-document request usually leaves
the source editor-attached, so preview immediately but do not claim completion:
present the exact already-authorized `--apply` retry command, ask the operator
to close the source and target tabs, and retry after detachment. This safety
handoff is not a second semantic confirmation.

For a commingled session, let the agent make the semantic selection without
letting the mutation step become nondeterministic: inspect `agent-doc outline`,
`agent-doc exchange list`, and the managed components, then author a JSON
selection manifest with `focus`, optional `rationale`, and the explicit `ids`,
`exchange_node_ids`, `archive_locators`, and `components` arrays. Archive
locators are the stable values printed by `agent-doc response-toc`. Preview it with `--selection`,
refine until it contains one coherent concern, then add `--apply`. The focus and
rationale appear in the preview and operation identity. Do not combine a
manifest with direct selector flags.

The no-selector form remains backward compatible: `agent-doc extract SOURCE
TARGET [--component NAME]` immediately moves the last exchange entry. Focused
selectors switch to preview-by-default mode. There is no top-level `split`
command because tmux/layout already uses “split” terminology.

The destination is initialized when absent. The operation records provenance in
both documents, rejects collisions and unmatched selectors, and can resume a
target-first interrupted apply without duplicating target content. Relative
linked-resource paths are rebased when the destination directory differs.
Preview's terminal notice names the exact destination and explicitly says that
it was not created (or was left unchanged). If `--apply` is blocked by a live
editor, the command prints the deterministic preview before failing and names
the attached path(s), confirms that no documents changed, and says whether the
destination was not created or left unchanged. A same-named Markdown file at a
different path is not the extraction result.
Apply is allowed only while both documents are detached from live editors; close their
tabs before applying. This is required because independent live CRDT authorities
cannot provide one atomic cross-document compare-and-swap.

## Transfer (move entire component)

Moves all content from a component in the source document to the same component in the target document. Source component is cleared.

```bash
agent-doc transfer <SOURCE> <TARGET> <COMPONENT> [--bypass-claim] [--items ID1,ID2]
```

**Example:** Move exchange content from one session to another:
```bash
agent-doc transfer tasks/briantakita.me.md tasks/software/corky.md exchange --bypass-claim
```

**Example:** Move specific backlog items by ID:
```bash
agent-doc transfer tasks/software/tsift.md tasks/software/tagpath.md pending --bypass-claim --items "#ast4,#0m97"
```

**Example:** Move specific icebox items by ID:
```bash
agent-doc transfer tasks/software/tsift.md tasks/software/tagpath.md icebox --bypass-claim --items "#cold1,#cold2"
```

**`--bypass-claim`:** Required when the target document is owned by a different tmux pane. Without it, transfer checks pane ownership and refuses to write to another pane's document. Always use `--bypass-claim` for cross-session transfers — it signals deliberate intent.

**`--items`:** Selective backlog or icebox transfer. Moves the tracked parent item whose flush-left `- ...` or `1. ...` line contains `[#id]`, along with any indented nested continuation lines under that item. Remaining items stay in the source. Valid with `component=pending`, `component=backlog`, or `component=icebox`. IDs can include or omit the `#` prefix.

**What happens (full transfer):**
1. Checks pane ownership of target (unless `--bypass-claim`)
2. Reads the component content from the source
3. Clears the source component
4. Appends to the target component with a `*Transfer from <source>*` annotation
5. If the transferred component is not backlog/pending or icebox, also merges both backlog and icebox items from source to target
6. Commits the target to prevent `(HEAD)` marking on next cycle
7. Saves snapshots for both files

**What happens (--items selective):**
1. Checks pane ownership (unless `--bypass-claim`)
2. Scans the source backlog or icebox for tracked parent items matching `[#id]`
3. Moves each matched item block, including indented nested lines, to the same target component; leaves unmatched items in source
4. Commits target; saves both snapshots
5. Warns about any IDs that didn't match

If the source document's active prompt contract explicitly says to "add to the backlog of <TARGET>", this target-side backlog change is the proof that later `finalize` / `session-check` expects. A response that names the preset but leaves the referenced target backlog unchanged should fail closed. If the response body lists concrete new tracked items such as `- [ ] [#id] ...`, every newly listed id must be present in the live target backlog before closeout succeeds; changing the target backlog in some other way is not enough. For `#agent-doc-bug`, a generic "transferred" note is no longer sufficient when the prompt-bearing changes described multiple bugs: the response must enumerate the transferred backlog items explicitly so closeout can prove the full set landed.

## Referral (pointer instead of copy)

Inserts a structured referral tag in the target, leaving content in the source. The target session can read the source for context without duplicating history.

```bash
agent-doc transfer <SOURCE> <TARGET> <COMPONENT> --referral [--bypass-claim]
```

**Example:** Reference tsift's exchange from tagpath without copying:
```bash
agent-doc transfer tasks/software/tsift.md tasks/software/tagpath.md exchange --referral --bypass-claim
```

**What gets inserted in target:**
```html
<!-- agent:referral src="../tsift.md" component="exchange" created="2026-04-20T..." -->
*Context from [../tsift.md](../tsift.md) — read source exchange for full history.*
<!-- /agent:referral -->
```

**What happens:**
1. Checks pane ownership (unless `--bypass-claim`)
2. Computes relative path from target to source
3. Inserts referral block into target's matching component
4. Source content is NOT modified
5. Commits target; saves snapshot

**When preflight sees referrals:** Future enhancement — preflight can resolve `<!-- agent:referral -->` tags and inject referenced content as context for the responding agent.

## Extract (move last exchange entry)

Extracts the last `### Re:` entry from the source exchange to the target document.

```bash
agent-doc extract <SOURCE> <TARGET> [--component <NAME>]
```

**Example:** Extract the last response to a new session:
```bash
agent-doc extract tasks/software/agent-doc.md tasks/software/new-feature.md
```

**What happens:**
1. Splits the last `### Re:` block from the source exchange
2. Removes it from the source
3. Appends it to the target's exchange component with a `*Extract from <source>*` annotation
4. Saves snapshots for both files

## When to use

- **Transfer:** When moving an entire topic/task to a different session document (e.g., a task outgrew its original session)
- **Extract:** When splitting off the last discussion point into its own session

## Important

- Both commands write directly to the target document -- no manual copy-paste needed
- Both update snapshots for both files, so the next `/agent-doc` cycle sees a clean baseline
- The source session's skill does NOT need to write to the target -- the binary handles cross-file writes
- **Cross-pane transfers:** Use `--bypass-claim` when the target is owned by a different pane. Transfer is a deliberate user action, not a concurrent write, so pane ownership should not block it.
