# Restart recovery journal

## Invariant and policy owner

Queue claims, queue lineage, and supervisor session lineage are the live
transition authorities. The restart journal is a cold-hydration projection and
durable sink only. It must not claim work, replay an effect, reconstruct an
agent response, or supersede an authoritative receipt.

`agent_doc_queue::restart_recovery` owns the pure projection, compaction, and
recovery policy. `agent_doc_queue_io::restart_journal` owns SQLite persistence,
boot-boundary evidence, and joins to queue lineage. Claim mutations update the
claim ledger and journal in one transaction. A startup reader may derive an
action, but the existing generation-fenced queue and supervisor commands remain
the only effect edges.

## Stored state

The `restart_journal_v1` row stores compact typed checkpoints, never prompts,
responses, transcript text, tool output, diffs, or artifact bodies:

- main harness, owner identity, resume UUID, boot ID, phase, and update time;
- per-worker queue-identity hash, owner, generation, worker identity, boot ID,
  phase, and update time;
- large-artifact references as a locator hash plus optional Git commit; and
- a bounded sequence of phase transitions for recent diagnostics.

Worktree, repository, branch, base, source commit, push, integration, and
release facts remain in queue lineage. The journal joins that authority during
reconciliation instead of copying its payload.

## Transition and recovery policy

| Previous checkpoint at startup | Evidence | Projected action |
| --- | --- | --- |
| main, nonterminal | same boot | reattach to the existing session |
| main, nonterminal | new boot and resume UUID | resume the exact main thread |
| main, nonterminal | new boot without resume UUID | needs operator |
| worker, dispatching/lost | new boot | generation-fenced redispatch |
| worker, running | same boot | reattach to the existing worker |
| worker, running | new boot plus durable artifact reference | integrate artifact through existing claim/lineage fences |
| worker, running | new boot and no durable artifact | needs operator; never guess whether effects completed |
| worker, reported/completed | durable receipt | preserve receipt; never replay work |
| worker, quota blocked | any | needs operator |

The initial production slice automatically resumes the Codex main thread
through the existing supervisor lineage. It durably discovers and classifies
worker recovery, exposes the result through startup context and `agent-doc
queue recovery`, and deliberately leaves worker reattachment/redispatch to the
existing generation-fenced coordinator edges. Native harness adapters may later
consume those typed actions; they must not turn `needs_operator` into a retry.

## Disk bounds and retention

Defaults are intentionally small and configurable at process start:

| Setting | Default | Environment variable |
| --- | ---: | --- |
| retained transition records | 64 | `AGENT_DOC_RESTART_JOURNAL_MAX_TRANSITIONS` |
| retained generations per queue identity | 3 | `AGENT_DOC_RESTART_JOURNAL_RECENT_GENERATIONS` |
| compaction cadence | 16 transitions | `AGENT_DOC_RESTART_JOURNAL_COMPACT_EVERY` |
| terminal worker recovery window | 6 hours | `AGENT_DOC_RESTART_JOURNAL_TERMINAL_RETENTION_SECS` |
| per-document serialized cap | 64 KiB | `AGENT_DOC_RESTART_JOURNAL_DOCUMENT_BYTE_CAP` |
| project-global serialized cap | 8 MiB | `AGENT_DOC_RESTART_JOURNAL_GLOBAL_BYTE_CAP` |

Compaction drops old transitions first, then expired terminal generations.
Per-document pressure may drop retained terminal snapshots after transition
history, but never live resumable state. Global pressure collects the oldest
fully terminal document rows only. If the live set itself exceeds either cap,
the writer fails closed instead of truncating recovery state.

## Evidence inputs and reactive topology

The process-scoped sources are supervisor session lineage, queue-claim
transitions, queue-lineage receipts, and the host boot ID. Claim mutations feed
the compact journal sink transactionally. Startup is the one actorless
cold-hydration boundary: it joins those durable sources into a computed recovery
projection, publishes a status value, then delegates effects to the existing
supervisor/queue state machines. There is no polling worker and no second live
state machine.

Linux `/proc/sys/kernel/random/boot_id` proves an OS boundary. Other hosts use a
stable unknown value and therefore fail closed to same-boot reattachment until
a platform adapter provides equally strong evidence.

## Allowed edit surfaces and verification

The implementation surface is limited to the pure queue recovery model,
queue-I/O journal adapter, claim transaction hook, startup reconciliation,
SessionStart context, and the diagnostic CLI. Tests cover recovery action
selection, recent-generation retention, transition compaction, terminal GC,
per-document overflow, live-state fail-closed behavior, global oldest-terminal
GC, and main-thread resume after a boot change.

## Explicit follow-ups

- Add harness adapters that can prove a native child worker is reattachable.
- Apply `Redispatch` automatically only through a compare-and-swap of the exact
  queue identity and generation.
- Record the finer main phases (`waiting_input`, `closeout_pending`) from the
  cycle state machine; startup already records `starting`, `running`, and
  owner-fenced `completed`.
- Add non-Linux boot-session evidence where the platform exposes it.
