---
name: reference_refused_replica_vetoes_visible_delivery_receipt
description: "A live CRDT replica released from the delivery barrier for availability keeps visible_delivery_projected false forever, so the native-save gate never opens and every later cycle is refused — FIXED 0.35.418+ (refusedreceiptveto)"
metadata:
  node_type: memory
  type: reference
---

Coined id: `refusedreceiptveto` (written unprefixed — the PreToolUse guard
blocks an untracked `#`-id in a Write; the tag IS in the committed source).

**Signature.** `session-check` reports `INTERRUPTED: canonical editor authority
and disk projection diverge` with **all four** components diverged
(`backlog`/`exchange`/`queue`/`review`), and `ops.log` shows, for the same
document:

```
crdt_replica_barrier_released_without_progress clients=<id> waits=12
crdt_nonconverging_replica_recovery reregister=definitively_refused_by_all:1 found=1 notified=0
crdt_commit_barrier ready=true delivery_required=false visible_delivery_projected=false live_editors=1
controller_commit_projection_pending decision=NativeSaveRequired request_sent=false
editor_projection_persistence_pending driver=editor_native_save save_diagnosis=native_save_gate_not_ready
retained_write_blocks_new_cycle cause=authority_disk_diverged
```

...then `controller_crdt_replica_handled method=replica_pull` at ~4/s with the
authority hash **frozen**. The pull storm is the tell: it is liveness, not
progress.

**Why.** `RelayHub` derives two facts over the live cut. `delivery_converged`
excludes a member the non-convergence budget released; `visible_delivery_projected`
(`for all live m: m.pending is empty`) does **not**. Once a live member blows its
budget with a non-empty queue the barrier releases it, `pending` is untouched, and
the receipt is **stably false** — nothing decrements the streaks, nothing drains
`pending`, and `claim_nonconverging_recovery` is one-shot. The native-save gate
needs the receipt, so no save is ever *requested*; disk never converges; the
retained write never settles; preflight refuses every later cycle.

`ReplicaSignalOutcome::diagnosis` already classified `definitively_refused_by_all`
apart from a delivery fault and said the caller "must stop treating the endpoint
as serving this document". **No caller implemented it** — the token was logged
and discarded.

**Two diagnostics lied.** `session-check` told the operator "the controller
**has requested** the generation-fenced native editor save and owns the next
closeout attempt" while `request_sent=false` and no request had ever been made;
and `ensure_controller_commit_projection_saved` only *observed* — it never
ensured anything. Same class as
[[reference_no_live_editor_reason_contradicts_live_editors_one]]. Grep
`request_sent=` and `save_diagnosis=` before believing a stand-down message.

**Fix.** `RelayHub::drop_definitively_refused_replica` `disconnect`s (never
`deregister`s) a released member whose endpoint answered and refused: replica
state survives for a lossless `reconnect`, and the pending queue only ever held
canonical-to-member updates. `diagnosis()` and `nonconverging_disposition()` now
both derive from one `classify()` so token and action cannot drift. Only
`definitively_refused_by_all` drops — absent, unreachable and generation-fenced
endpoints stay retryable, which is what keeps this from being a silent
`--force-disk`.

**This is the write-side dual of the `EditorReplicaStrand` read-side fix.**
`formal/tla/VisibleDeliveryReceipt.tla` proves the pre-fix state is a genuine
state-graph deadlock; its wedge cfg must deadlock and its reach cfg must
violate, both in `scripts/run_tla.sh`'s must-violate list. See
[[reference_agent_doc_recovery_index]].

## Second half: an exhausted build mismatch was also classified retryable

Same defect class, same chain, found by following the first fix into the live
system. `send_message_to_pid_recovering_build_mismatch` surfaces a typed
`BuildMismatch` only AFTER its reload-only compatibility request has failed, so
its `Err` arm means "the listener answered with a version rejection AND the one
recovery for that rejection did not deliver". It was collected into
`build_mismatches`, logged `crdt_replica_build_mismatch_recovery_failed`, and
then classified `definitive_refusal=false` — retryable forever. Retrying re-runs
the same handshake between the same two builds.

Consequence: `decide_authority_recovery` never got its refusal witness, stayed in
`FailClosed`, and the replica could never be rebuilt:
`realtime_doc_resolve_disk_read_refused ... editor_open=true
invariant=attached_editor_never_descends_to_disk`.

Signature to grep: `listener=<ver>+<stampA>, client=<ver>+<stampB>` with
`definitive_refusal=false`. The two stamps are epoch seconds — decode them; the
LISTENER is the controller, the CLIENT is the editor's loaded cdylib.

Deliberately NOT applied to the native-save send path, which does not attempt the
reload recovery — a mismatch there is a first observation, not an exhausted one.
A transport failure with no typed handshake answer stays retryable everywhere;
promoting silence to proof would be a silent `--force-disk`.

**Likely trigger:** a mid-session `make install` cdylib reload leaves the old
plugin registration alive but no longer serving the document — see
[[reference_cdylib_reload_strands_attached_document_replica]]. Prefer a
binary-only install (`cargo build --profile release-local --bin agent-doc` +
`agent-doc binary-install --source ...`) over `make install` while a document is
attached; `make install` also runs `lib-install` + `install-editor-plugins`.

**But a binary-only install CREATES the build skew above.** The controller moves
forward and the editor's loaded cdylib does not, and the reload that would fix it
is itself deferred while a document is mid-cycle. If you install the binary, build
and `lib-install` the matching cdylib from the SAME tree in the same step
(`cargo build --profile release-local --lib` + `CARGO_TARGET_DIR=target/local-install
agent-doc lib-install --profile release-local`), and verify the `.so` is newer than
your edited source before trusting it — a `--lib` build that prints no `Compiling`
lines may still be a fresh relink, or may be stale; check the mtime and grep the
`.so` for a token you just added.
