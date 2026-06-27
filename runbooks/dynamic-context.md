# Dynamic Context

Use the installed agent-doc SKILL/AGENTS surface as a hot-path router. Keep only
cycle-critical triggers and invariants inline; put branch detail in bundled
runbooks or binary-generated preflight/plan output.

## Instruction Surface Pattern

- `SKILL.md` owns trigger wording, mandatory closeout boundaries, and the ordered
  cycle spine.
- `runbooks/*.md` own branch-specific procedure detail.
- `okf/*.md` owns durable concept definitions and vocabulary that should remain
  stable across prompt sessions.
- `agent-doc preflight`, `agent-doc plan`, `tsift` envelopes, and session-memory
  commands own generated context for the current document and repo.
- Managed generated files (`.claude/skills/agent-doc/SKILL.md`, `.codex/AGENTS.md`,
  `.opencode/skills/agent-doc/SKILL.md`, root `AGENTS.md`) are mirrors. Change the
  bundled source and reinstall instead of manually editing each mirror.

## Entry Rule

Every new AGENTS/SKILL entry should name its dynamic source:

- Procedure: `Follow runbooks/<name>.md when <condition>.`
- Durable vocabulary: `Use okf/<name>.md when <term or concept> needs a stable
  definition.`
- Dynamic state: `Use agent-doc <command> and trust its emitted fields.`
- Code context: `Use tsift <envelope> instead of raw recursive reads.`
- Historical/session context: `Use agent-doc memory/search or session-review packs
  instead of replaying the full document or transcript.`

## Service / DB Shape

Keep dynamic context local-first and deterministic. SQLite is enough for the
agent-doc family:

- `session_memory`: document id, component, item id, status, source commit, summary.
- `context_pack_cache`: query, budget, input hashes, generated pack, expiry.
- `runtime_state`: controller/session facts already owned by the binary.
- `source_index`: file/rule/runbook hash, summary, and expansion command.
- `okf_index`: concept path, type, tags, content hash, and expansion command.

Expose this through small commands that return bounded packs with source handles,
for example `agent-doc memory search`, `tsift --envelope context-pack`, or a future
`agent-doc context pack --budget normal`. Never make the database the source of
policy; committed files own policy, and generated rows must carry source hashes so
stale context can be invalidated.

## Invalidation

A pack is stale when any referenced file hash, git revision, binary version,
frontmatter/config fingerprint, or session-memory source id changes. Emit those
inputs with each pack so agents can decide whether to reuse, refresh, or expand.
