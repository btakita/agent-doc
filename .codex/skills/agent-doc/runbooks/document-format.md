# Document Format

Session documents are markdown files with YAML frontmatter. Two modes: **inline** (legacy) and **template** (current default for new docs).

## Frontmatter

```yaml
---
agent_doc_session: <uuid or null>
agent: <name or null>
model: <model or null>
branch: <branch or null>
agent_doc_format: <inline | template>   # default: inline
agent_doc_write: <append | crdt>         # crdt required for stream mode
agent_doc_mode: <append | template>      # legacy alias for agent_doc_format
agent_doc_model_tier: <low | med | high> # optional; feeds model tier gate
---
```

`agent_doc_format: inline` is the canonical name for the old "append" format. `append` is accepted as a backward-compat alias. Similarly, `agent_doc_mode: template` is a legacy alias for `agent_doc_format: template`.

## Inline mode

The body alternates `## User` and `## Assistant` blocks. Inline annotations within any block — blockquotes, HTML comments, edits to previous responses — are valid prompts and show up in the next diff.

`agent-doc finalize --stream` appends one complete
`## Assistant\n\n<response>\n\n## User\n\n` block per committed cycle. Bare
`write --stream` is rejected for session responses.

## Template mode

The body contains named components delimited by HTML comment markers:

```markdown
<!-- agent:status patch=replace -->
<!-- /agent:status -->

<!-- agent:recap patch=replace -->
<!-- /agent:recap -->

<!-- agent:run priority subagents clear -->
<!-- /agent:run -->

<!-- agent:review -->
<!-- /agent:review -->

<!-- agent:todo priority -->
<!-- /agent:todo -->

<!-- agent:backlog priority queue -->
<!-- /agent:backlog -->

<!-- agent:icebox -->
<!-- /agent:icebox -->

<!-- agent:notes -->
<!-- /agent:notes -->

<!-- agent:done -->
<!-- /agent:done -->

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
```

New scaffolds use this queue-centric order:
status, worktrees, recap, active, run, review, todo, backlog,
icebox, notes, done, then exchange last. `active` is a binary-owned
informational projection of started, unclosed work grouped by authoritative
source; it is never an executable work source. Legacy `agent:open-items`
markers are migrated to `agent:active`.
Missing-component repairs use the same relative slots without moving
established components; valid documents in the previous order are not silently reordered.

### Conventional component names

- `<!-- agent:input -->` — user writes prompts here (read-only for the agent).
- `<!-- agent:output -->` — agent responds here when no other target is given.
- `<!-- agent:exchange -->` — shared conversation surface; user and agent both write inline. Default target for unmatched patches and the canonical location for the response boundary marker.
- `<!-- agent:run -->` — runnable prompts consumed in queue order. Legacy alias: `<!-- agent:queue -->`; existing alias markers are preserved, while new scaffolds use `agent:run`. The default scaffold is `<!-- agent:run priority subagents clear -->` and does not carry `go`, because a queue with no control already runs in go-mode.
- `<!-- agent:backlog -->` — task tracker managed through granular backlog ops (see `pending-ops.md`). Legacy alias: `<!-- agent:pending -->`.
- `<!-- agent:review -->` — code-complete tracked work awaiting human review. `--backlog-gate` moves backlog items here as `[/]`; `--backlog-ungate` moves them back.
- `<!-- agent:icebox -->` — project icebox for items parked outside the active backlog. Sits after `agent:backlog` in the default template.
- `<!-- agent:signals -->` — realtime signal definitions/readings. Built-in tests, queue, supervisor, and IPC signals are expected first; custom signal plugins are deferred.
- `<!-- agent:status -->`, `<!-- agent:log -->`, `<!-- agent:architecture -->`, ... — agent-managed via targeted patch blocks.
- `<!-- agent:recap -->` — this turn's recap. While the turn is in progress the previous turn's recap stays beside it, and until this turn's recap is written the current section shows the turn's status. Committing the turn removes the previous one; a status-only current section restores the previous recap. See [recap.md](recap.md).


#### Numbered steps under a row (`#orderedsteps`)

A numbered list under a run row is an ordered sequence. Steps run one at a
time, in order; they inherit the row's lineage and presets like bulleted
children:

```markdown
- ### https://github.com/org/repo/pull/389
  - steps
    1. #rebase
    2. #address
    3. #adversarial-pr
```

- Only the first unfinished step is drained, offered to the subagent pool, or
  claimable. Later steps rest `⛔ ⟦blocked by step-N⟧` and `queue tree` shows
  `(waits on step N)`; `queue claim` refuses them.
- A step is finished when it is struck and no live agent follow-up remains
  under it. Claimed, running and reported (`☑️`) steps are not finished.
  Operator-owned rows under a step (`[operator-verify]`, `🙋`) do not hold it.
- `N.` and `N)` both work. Numbers are kept exactly; glyphs, claim labels and
  strikes go after the number (`1. ~~#rebase~~`).
- A step's follow-ups and response tree nest under it at its content column
  (7 spaces under `    1. `). Close a step by its nest vector, for example
  `queue consume --ack-text 'https://github.com/org/repo/pull/389: steps: #address'`.
- Bulleted siblings (`- note`) beside numbered rows are not steps, and a
  numbered list at column 0 is not a sequence.

#### Edge operators: ` |` pipe and ` >` fold (`#pipeprompt`, `#foldprompt`)

A run row's last token sets how its children relate to it. With no
operator, the row is context and each child is its own leaf (paths join with
`: `). Two operators make the row and its children ONE task with one queue
head:

```markdown
- Collect the open agent-doc GitHub issues |
  - group them by component |
    - draft a triage summary |
      - under 200 words
      - no internal ids
- Summarize the release thread >
  - focus on blockers
  - skip chit-chat
```

- ` |` pipes: each ` |` row passes its result to the rows nested under it.
  Numbered children (`1.`) chain in order, each taking the previous step's
  result; bulleted children (`- `) fan out, each taking the parent's result
  (above: `… draft … | under 200 words` and `… draft … | no internal ids`).
  The grammar is strict: inside a statement, a row with children must end in
  ` |` (or ` |+` / ` >`). Otherwise nothing folds, the rows stay separate
  leaves, and the lint names the row.
- ` |+` deep-pipes: every descendant pipes too, so the whole subtree
  flattens in document order into one chain (`collect | group | draft |
  under 200 words`), whatever the list type. Inner rows need no ` |`.
- ` >` folds: the row and its whole nested list are one task; the list is
  context. Inner rows need no operator.
- Inside a ` |`, ` |+` or ` >` block, numbered rows (`1.`, `2.`) stay in the
  one task: they keep their numbers and never become separate
  `#orderedsteps` steps or get a `⛔` hold. Under ` >` they are a procedure
  to do in order; under a pipe they are the chain order.
- The top row is the only head: it alone gets the `[#q-…]`, claim, strike and
  response tree. Inner rows get no ids and are never offered, claimed or
  struck on their own. Commands pipe too: `- #review |` over `- #address`.
- The operator is the last authored token, before any `[#q-…]` or other
  binary suffix, and never inside a code span. A ` | ` in the middle of a row
  is prose, unless every segment starts with a preset (an inline pipe, below).
  Adding or removing the operator does not change the row's claim
  identity. While the head is claimed an edit cannot remove it, and while a
  row inside is claimed adding it does not fold the block.
- Stage indicator: the worker reports the stage it starts with
  `agent-doc queue claim <FILE> --item <ITEM> --owner <OWNER> --refresh
  --generation <N> --stage <n>` (stage 1 is the top row). That row shows
  `▶️` and earlier rows `✅`; the head keeps its own lifecycle glyph. The
  glyphs are binary bookkeeping and leave when the claim does.
- The lint reports a ` |`, ` |+` or ` >` row with no children, a missing
  inner ` |`, a code span ending in ` |`, a ` |>` row with no stage or inside
  a ` |` / ` |+` / ` >` block, and the reserved suffix ` ||`, plus ` |*` (did
  you mean ` |+`?).

#### Hand-off pipe: ` |>` (`#pipeprompt`)

A row ending in ` |>` is stage 1 of a hand-off. Each row nested under it is a
later stage that runs as its own queue row, in a NEW worker, with the
previous stage's hand-back as its input:

```markdown
- #review |>
  1. #address
  2. summarize the changes |>
     - post to the tracker
     - post to the channel
```

- Nothing folds: the ` |>` row has one `[#q-…]`, and every stage keeps its
  own claim, generation and response, nested in order.
- Stage 1 (the ` |>` row) runs first. Every stage under it waits
  `⛔ ⟦blocked by stage-1⟧` and `queue claim` refuses it.
- When stage 1 closes (`--ack-text`, `--ack-id`, or the automatic reported
  closeout), the row is not struck: it shows `✅` and keeps its response
  tree. Its stages are released. Numbered stages then run one at a time
  (`#orderedsteps`, `⛔ ⟦blocked by step-N⟧`), and bulleted stages run in
  parallel.
- A stage's brief carries `Hand-off input`: the hand-back of the stage before
  it, quoted from the document. A bulleted stage or the first numbered stage
  takes stage 1's hand-back; a later numbered stage takes the numbered stage
  before it. The hand-back is the `agent:exchange` entry that carries the
  stage's `[#r-…]` id, else its response tree or completion cell. It is read
  when the brief is built, so editing a future stage changes what runs.
- The ` |>` row is struck in the same write as its last open stage. A nested
  ` |>` stage works the same way and closes the row above it when it is the
  last one.
- Each stage's presets are its own (`#presetorder`): `- #address |>` over
  `- #adversarial-pr` is `#address then #adversarial-pr` as two workers. A
  `NO-WRITTEN-FIX` stage 1 settles only stage 1; the review stage still runs.
- `✅` on a ` |>` row is binary-owned and never part of its identity. Do not
  type it; to skip stage 1, strike the row and its stages instead.
- The state is all in the document: after a crash nothing reruns, a lost
  stage worker retries only that stage, and self-repair finishes a strike
  an older writer left without its `✅`.

#### Inline pipes: `- #review | #address` (`#pipeprompt`)

A row whose segments are all presets can pipe on one line:

```markdown
- #review | #address
- #gh-fix https://github.com/org/repo/issues/7 | #pr
- #rebase |> #address |> #adversarial-pr
```

- A row is an inline pipe only when splitting it on ` | ` or ` |> ` (outside
  code spans) gives two or more segments and every segment starts with a
  known preset. Arguments may follow the preset: `#gh-fix <url> | #pr`,
  `#address (see <url>) |> #adversarial-pr`.
- It is the same statement as the nested form: `- #review | #address` is
  `- #review |` over `- #address`. With only ` | ` it is one task whose
  stages are its segments, and its submission ends with `Inline pipe
  (#pipeprompt) …` and the pipeline.
- A trailing ` |` (or ` |>`, ` >`) still applies to the row's children:
  `- #review | #address |` over `- under 200 words` pipes the last segment
  into them.
- ` |> ` between segments is a hand-off. Each stage needs its own row, so
  agent-doc rewrites the row into the nested ` |>` form once, before any
  worker holds it: `- #rebase |>` over `- #address |>` over
  `- #adversarial-pr`. The row keeps its `[#q-…]` on stage 1, a ` | ` inside
  one stage stays inline (`#a | #b |> #c` becomes `#a | #b |>` over `#c`),
  and the row's children move under the last stage. A row a worker already
  holds is left as written and runs its stages in order in one task.
- Each segment's leading preset is a directive (`#directiveposition`), and
  the stages feed `#presetorder`: `- #address | #adversarial-pr` is
  `#address then #adversarial-pr`.
- A ` | ` row with a segment that does not start with a preset stays prose
  (`pending | completed`). When its first segment is a preset
  (`#review | then summarize`), the lint says to nest the rest under it with
  ` |`. Shell pipes inside code spans are never operators.

Built-in element semantics live in the `agent-doc-element-*` crate family and
are composed by `agent-doc-element-registry`. An unregistered `agent:*`
component uses the `agent-doc-element-unknown` fallback: preserve
operator-visible content and do not apply semantic mutations until a plugin or
built-in descriptor defines that element.

### Inline component attributes

```markdown
<!-- agent:output patch=append max_lines=50 -->
```

- `patch=<mode>` — one of `append`, `prepend`, `replace`. `mode=` is accepted as a backward-compat alias; `patch=` wins if both are present.
- `max_lines=<N>` — trim the component to the last N lines after patching. `0` or absent = unlimited.

Mode precedence (highest → lowest): stream overrides > inline attribute > `.agent-doc/config.toml` `[components.<name>]` > built-in default (`exchange`/`findings` default to `append`; everything else defaults to `replace`).

## Snapshot storage

- Location: `.agent-doc/snapshots/` relative to the project root.
- Filename: `sha256(canonical_path) + ".md"`.
- Contains the full document content after the last submit.
- The skill never reads snapshots directly — use `agent-doc read <FILE>` to fetch HEAD content.
