> Extracted from [07-commands.md](07-commands.md)

# Closeout Commands

This file covers binary-owned response persistence: commit boundaries, patch/write semantics, recovery, and post-write session validation.

## commit

`agent-doc commit <FILE>`

- Commits the saved snapshot, not arbitrary working-tree drift.
- Narrow self-heal is allowed only for agent-owned drift that preserves the redacted component structure, or for exchange-only already-committed historical response growth proven by `HEAD`.
- Plain user prompts must remain uncommitted.
- If `snapshot == HEAD` but the working tree still differs, `commit` must classify that state as later local drift rather than a missed patchback.
- Direct assistant patchback in the working tree without a newer binary-owned cycle must fail closed.
- Historical self-heal must fail closed if typed components such as `status`, backlog, or pending changed, or if the repaired tail would still contain a bare prompt target.
- The command serializes the staging/commit critical section with a git-dir scoped advisory lock.
- Successful post-commit cleanup must leave the committed blob, snapshot, and user-facing file in the same clean shape except for genuine later user edits.

## compact

`agent-doc compact <FILE> [--component NAME] [--message TEXT] [--tag NAME] [--commit]`

- Rewrites exchange or another component into a compacted summary/archive-pointer state.
- After writing the archive markdown, compact best-effort upserts that archive into `.agent-doc/archive-index.db`; indexing failure warns but must not roll back archive creation.
- For exchange compaction without an explicit message, the default summary must preserve live backlog/queue/icebox context.
- `--commit` proves only that the compacted document state itself crossed the commit boundary; it does not also persist any later human explanation.

## patch

`agent-doc patch <FILE> <COMPONENT> [CONTENT] [--mode replace|append|prepend]`

- Deterministic named-component patcher for direct component mutation.
- Resolved component config may supply timestamps, size trimming, and pre/post hooks, but explicit CLI mode still controls append/prepend behavior.

## write

`agent-doc write <FILE> [--baseline-file PATH] [--stream] [--ipc] [--force-disk] [--origin ORIGIN]`

- Parses `patch:*` blocks from stdin, applies them against the baseline, merges with current disk content when needed, and saves snapshot/CRDT state.
- IPC-first write remains the default when the editor patch directory exists; `--force-disk` bypasses IPC.
- Final parsed responses must be durably captured before any document mutation once they survive strict pre-write guards.
- Compatibility normalization may rewrite one legacy list-shaped backlog/pending patch through the granular pending primitives before capture, but unsupported shapes still fail closed before `response_captured`.
- Destructive `patch:todo` replacements are blocked when they would shrink an existing checklist surface.
- Session-document `write --commit` shares `finalize`'s strict closeout contract.
- Bare session-document `write` is not a terminal success path: if it preserves a response and leaves the cycle open at `response_captured` / `write_applied`, the command must fail closed with recovery intact instead of returning success and waiting for a later `agent-doc commit`.

### Write-path invariants

- Template/CRDT retries must adopt the already-visible response instead of appending a duplicate block.
- Prompt-prefix normalization for append-mode exchange comes from the shared prompt-bearing classifier, not ad hoc line-shape guesses.
- IPC sidecar verification and post-commit working-tree prompt-prefix repair must preserve duplicate prompt-target occurrences by count; one earlier prefixed `spec-test-...` line must not mask a later bare duplicate.
- Carried-forward formatting directives from historical `❯ ...` prompt blocks must be re-injected into new agent prompts when they still read as active document-level requirements.
- Template writes fail closed if live conversation content would end up outside `agent:exchange`, including the inter-component gap between the exchange close marker and later components such as backlog, except for the narrow duplicate-close / duplicate-open repair shapes the binary knows how to normalize safely.
- Boundary markers are binary-owned. The write path removes stale boundaries, inserts a fresh end-of-exchange boundary, applies the response, and re-inserts a fresh boundary at the new exchange end.
- Exchange append replies must bind to the oldest compatible unresolved prompt block rather than skipping earlier prompts still ahead of the old boundary.

## finalize

`agent-doc finalize <FILE> [write flags...]`

- Strict happy-path closeout for session responses.
- Validates git-backed context before mutation, runs the normal write pipeline, forces `git::commit(<FILE>)` even after partial write errors, and fails unless the final cycle state is `committed`.
- Empty normalization-only template payloads are invalid; the response must contain real response-body proof.
- The same pre-write pending-capture, backlog-required, and pending-done gates apply here before the document mutates.
- A bare `compact exchange` request blocks ordinary finalize/write closeout and must route through `agent-doc compact <FILE> --commit` instead.

## repair

`agent-doc repair <FILE>`

- Repairs open `response_captured` / `write_applied` cycles, deduplicates already-applied responses, and can adopt a visible response already present in the live document when the snapshot still lags behind.
- The same repair path also handles stale `preflight_started` cycles when the hashes or safe historical `HEAD` proof make that deterministic, and it may also auto-close an otherwise-empty `preflight_started` cycle after the bounded stale timeout when no capture exists for that cycle.
- Historical capture replay is narrow: it requires either an active capture artifact or a matching orphan prompt target in the live exchange.
- Transcript-shaped or full-document-dump captured payloads must fail closed and be parked under `.agent-doc/repair-blocked/`.
- No-pending repair still runs transcript canonicalization, completed-backlog reap, safe escaped-conversation repair (including the exchange-to-backlog gap case), safe duplicate-close repair, and deterministic stale-boundary repair when applicable.
- Explicit `repair` must fail closed, not print a false clean `No pending response found`, when `session-check` would still block the document after those no-pending repairs. In particular, when a committed historical patchback can be repaired from `HEAD` but later prompt-bearing user drift still remains, `repair` must surface the same interruption instead of downgrading the blocked document to `Noop`.
- For git-backed docs, repair must not stop after updating the document or snapshot. The same command must carry the recovered closeout through commit.

## preflight

`agent-doc preflight <FILE>`

- Runs interrupted-cycle recovery, repair, commit, claims-log drain, linked-doc inspection, diff computation, and HEAD read in one binary-owned step.
- Before diffing, preflight must not auto-compact the exchange. Legacy `auto_compact` frontmatter is ignored for compatibility, and session-accretion heuristics remain advisory only.
- Open cycle states are `preflight_started`, `response_captured`, and `write_applied`.
- Boundary-only / `(HEAD)`-only churn is normalized back to `no_changes`.
- If the file diff is empty but the active harness prompt still contains body text after `agent-doc <FILE>`, preflight synthesizes an in-memory diff from that prompt body.
- Preflight must fail closed before diffing when the snapshot/file pair already looks like an uncommitted assistant closeout with no recoverable cycle left to explain it.
- Explicit backlog targets extracted from prompt presets are resolved relative to the project root. If a relative target begins with the current project directory name, that redundant prefix is stripped before falling back to a non-existent doubled path.

## session-check

`agent-doc session-check <FILE>`

- Verifies that the latest response cycle reached a terminal committed state and that no likely direct assistant patchback bypassed the binary-owned write path.
- Fails on open cycle states, uncommitted visible `### Re:` / `## Assistant` patchbacks, or hidden `snapshot != HEAD` closeout drift.
- May self-heal only narrow exchange-only already-committed historical drift proven by `HEAD`.
- Must fail closed when the repaired tail would still include a bare prompt target or typed-component drift.
- Optional closeout sidecars such as cycle-state, capture, startup-miss, and ops-log files are advisory; if one disappears between discovery and read, session-check treats it as absent state instead of surfacing a transient `ENOENT`.
- Runs the pending-capture, pending-done, backlog-shadow, backlog-replay, completed-item reap, and snapshot-vs-HEAD closeout guards after a committed cycle.
- The Codex/direct-exec harness path is expected to run this immediately after `finalize` or strict `write --commit`, and must fail closed if the check reports an open or bypassed cycle.
