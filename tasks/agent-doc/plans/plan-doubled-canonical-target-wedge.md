# Plan — retained intent doubles the whole document and wedges every recovery path

**Tag:** `#doubledtargetwedge`
**Found:** 2026-07-30, `tasks/professional/equityfundingsource.md`, agent-doc 0.35.104
**Status:** diagnosed, NOT implemented (filed per the `#agent-doc-bug` preset)

## Symptom

Every command that constructs a canonical write target fails closed with:

```
serialized_atomic_write: refusing structurally invalid canonical target for <FILE>
(duplicate_singleton_component:backlog=2,done=2,exchange=2,icebox=2,queue=2,review=2,status=2);
current Lazily/editor authority is unchanged and pending intents remain retained
```

and `session-check` fails earlier still, at the integrity gate:

```
[integrity-gate] INTERRUPTED: 2 agent boundary markers in <FILE>;
the exchange must contain at most one standalone boundary marker
```

## What is actually wrong

The retained-intent authority holds the document **concatenated with itself**, exactly 2×.

| source | lines | boundary markers | components |
|---|---|---|---|
| disk | 343 | 1 | 1 of each |
| git HEAD | 343 | 1 | 1 of each |
| `.agent-doc/snapshots/<hash>.md` | 343 | 1 | 1 of each |
| `agent-doc read` (authority) | **686** | **2** | **2 of each** |

`diff <(agent-doc read FILE | sed -n '344,686p') FILE` is **empty** — the second half is
a verbatim copy, with its own `---` frontmatter starting at line 344. No unique operator
text exists in the duplicate, so nothing is lost by discarding it; there is simply no
supported command that will.

The exchange component itself is healthy: `agent-doc exchange list` succeeds (it does not
go through the structural validator) and returns 15 nodes with **no duplicate ids or
labels**. So the doubling is introduced when the *document-level* target is rebuilt from a
retained intent, not by a duplicated exchange node.

`state.db` holds 4 `document_write_deferred` records for this document, produced across
several turns in which `respond` reported
`editor_replica_reregister=delivery_failed_to_all:1` and retained the intent. The likely
mechanism: a retained intent whose payload is the **whole document** is replayed against a
base that already contains the whole document, and the replay concatenates instead of
rebasing. Every subsequent command reconciles that intent first, rebuilds the doubled
target, and the structural validator correctly refuses it.

## Why nothing self-heals

Every documented recovery was tried on 0.35.104, after `make install` and an IDE restart,
with **no controller or supervisor process running**:

| command | result |
|---|---|
| `preflight` (auto-repair for `late-IPC committed-response over-application`) | refuses: structurally invalid target |
| `reset --from-current --preserve-session` | sends `save_document` (receipt `applied`), then waits forever for convergence |
| `reset --from-current --force-disk` | refuses: same structural error |
| `reset --from-current` (no `--preserve-session`) | refuses: same structural error |
| `repair --apply-recovery` | withheld — classifies as `escaped_template_patch`, wants a response body |
| `cancel` | `no open cycle to reclaim` |
| `admin recycle` | "marked to recycle at next idle boundary" — no process to recycle |
| `autofix --apply` | its own whitelisted `write --commit` exits 1 |
| `admin repair-projection` | rebuilds *from* the bad SQLite state |
| move `.agent-doc/crdt/<hash>.yrs` aside | no effect (that file is stale, 13 days old) |

The gate runs before recovery in both `preflight` and `session-check`, so the binary's own
repair paths are blocked by the very condition they exist to clear. `autofix` correctly
reports `editor_convergence` as unsatisfiable: *"rebased canonical target passes the shared
structural validator"* is exactly the proof that cannot be produced.

Note also the misclassification: `repair` reports `escaped_template_patch` and asks for a
rewrite with real `<!-- patch:exchange -->` blocks. There are no escaped markers on disk —
`grep` finds exactly one well-formed marker per component. The classifier is reading the
doubled target and blaming response formatting.

## What is needed

1. **A supported way to drop a retained intent.** There is currently no command that
   abandons `document_write_deferred` records without first rebuilding the target they
   corrupt. `cancel` only reclaims an *open cycle*; this state has none. Something like
   `agent-doc repair --drop-retained-intents <FILE>` (document-scoped, refusing to run if
   any retained payload contains text absent from disk) would close this class.
2. **Validate the target BEFORE retaining the intent, not only before writing it.** A
   payload that fails `duplicate_singleton_component` can never become writable, so
   retaining it only guarantees a permanent wedge. Reject at capture.
3. **Make replay rebase rather than concatenate.** A whole-document payload replayed onto a
   base that already contains it should converge to one copy. Worth an idempotency check on
   the replay path: replaying a payload whose content is already a prefix of the base must
   be a no-op.
4. **Let `reset --force-disk` mean it.** The flag documents itself as "bypass editor
   convergence and write directly to disk … intended for explicit recovery", but it still
   routes through the structural validator on a target built from the bad authority. For a
   recovery flag, disk should be the *source*: rebuild state from the visible file and
   discard conflicting retained intents.
5. **Fix the `escaped_template_patch` misclassification** so the operator is not sent to
   rewrite a response that is not the problem.

## Reproduction sketch

1. Drive several `respond` cycles while the editor replica is absent, so each returns
   `editor_replica_reregister=delivery_failed_to_all` and retains its intent.
2. Let a later cycle reach `write_applied` and commit the body by hand.
3. Re-invoke on a fresh binary. The replayed intent concatenates, and every command above
   fails closed.

A SimWorld case should assert that after N retained intents against an absent replica, the
reconstructed target contains exactly one of each singleton component.

## Operator-gated workaround

Not applied — it edits shared state. `.agent-doc/state.db` is ~80 MB and backs **every**
document in the project, so clearing this document's rows by hand risks the failure mode in
`reference_agent_doc_state_db_header_clobbered_by_tracing_on_controller_recycle`. Disk and
git HEAD are correct and complete, so nothing is at risk from waiting; only agent-doc's
bookkeeping for this one document is stuck.
