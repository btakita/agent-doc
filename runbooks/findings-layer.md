# Findings Layer (authored knowledge)

agent-loop uses tsift's **Findings Graph Layer** (`#trt1`) — an authored-knowledge
layer that anchors design decisions, invariants, and gotchas to code so the "why"
is retrieved on the agent hot path instead of rotting in prose. Findings live in a
dedicated `.tsift/findings.db` (independent of the rebuildable code projection, so
they survive `graph-db refresh` and reindex). Full spec:
[`src/tsift/specs/graph.md`](../src/tsift/specs/graph.md) § Findings Graph Layer.

## Configuration

`.tsift/config.toml` (local, gitignored) opts agent-loop into passive harvest:

```toml
[findings]
passive_harvest = true
```

Injection (below) is **automatic** and needs no config. Only the passive
auto-capture path is gated — drafts never reach the hot path until promoted.

## Authoring

Anchor a finding to a symbol or file (defaults to `trusted`, idempotent by
content):

```bash
tsift finding add --kind decision --title "<T>" --body "<why>" --about <symbol_or_file>
tsift finding list [--about <s>] [--status draft|trusted] [--include-stale] [--json]
```

`--kind` ∈ `finding | decision | note`. Anchoring is by stable handle, never line
number; a finding goes **stale** (hidden by default) once its anchor's content
advances past the captured watermark.

## Passive harvest + promotion

```bash
tsift finding harvest [<path>] [--json]   # config-gated; extracts DRAFT candidates from .agent-doc/archives
tsift finding promote <id>                # flip a draft → trusted
```

Harvest scans agent-doc session archives for lines pairing a decision/insight
signal with a code token that resolves to an indexed symbol/file, storing each as
`draft`. Drafts are **excluded from injection** until `promote` confirms them —
the promotion step is the precision gate, not the extractor. Harvest is idempotent
and never overwrites an existing finding.

> Run harvest from the repo whose index resolves the anchors. agent-loop's root
> index covers root-level files; for findings about `src/tsift/...` symbols, run
> from the tsift submodule so its index resolves the symbol anchors.

## Retrieval (automatic)

Trusted, fresh findings ride every navigation surface for nodes already in the
result set — no extra command:

- `tsift --envelope context-pack <doc>` — `findings` section for touched files/symbols.
- `tsift --envelope search <query>` — findings for matched symbols/files.
- `tsift --envelope explain <symbol>` — findings for the symbol, callers/callees, community.
- `tsift graph-db map [--format md|html]` — communities/hubs/focus annotated with attached findings.

Each preview carries an `expand` command (`tsift finding list --about <anchor> --json`)
back to the full anchored set.
