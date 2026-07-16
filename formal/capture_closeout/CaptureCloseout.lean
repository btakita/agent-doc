/-
  Lean model of the agent-doc captured-response closeout policy.

The Rust sources of truth are
`agent-doc-workflow/src/capture.rs::decide_capture_closeout_materialization` and
`agent-doc-document-realtime/src/write_policy.rs`.
  An open capture with an available commit surface may close only when its exact
  response is inline or in a compact archive referenced by that surface.
-/

namespace CaptureCloseout

inductive Basis where
  | noActiveCapture
  | terminalCapture
  | commitSurfaceUnavailable
  | inlineCommitSurface
  | referencedCompactArchive
  deriving DecidableEq, Repr

inductive Decision where
  | allow (basis : Basis)
  | blockMissingResponse
  deriving DecidableEq, Repr

structure Evidence where
  activeCapture : Bool
  captureTerminal : Bool
  commitSurfaceAvailable : Bool
  responseInCommitSurface : Bool
  responseInReferencedCompactArchive : Bool
  deriving DecidableEq, Repr

def decide (e : Evidence) : Decision :=
  if !e.activeCapture then
    .allow .noActiveCapture
  else if e.captureTerminal then
    .allow .terminalCapture
  else if !e.commitSurfaceAvailable then
    .allow .commitSurfaceUnavailable
  else if e.responseInCommitSurface then
    .allow .inlineCommitSurface
  else if e.responseInReferencedCompactArchive then
    .allow .referencedCompactArchive
  else
    .blockMissingResponse

def openEvidence (inline archive : Bool) : Evidence where
  activeCapture := true
  captureTerminal := false
  commitSurfaceAvailable := true
  responseInCommitSurface := inline
  responseInReferencedCompactArchive := archive

/-- Safety: an open capture cannot be allowed unless the exact response has a
    materialization proof on the commit surface or its referenced archive. -/
theorem open_available_allow_iff_materialized (inline archive : Bool) :
    (∃ basis, decide (openEvidence inline archive) = .allow basis) ↔
      inline = true ∨ archive = true := by
  cases inline <;> cases archive <;> simp [decide, openEvidence]

/-- Completeness for Compact Exchange: archive materialization is sufficient
    even when compaction intentionally removed the response from the document. -/
theorem exact_archive_allows :
    decide (openEvidence false true) = .allow .referencedCompactArchive := by
  rfl

/-- An unrelated or absent archive cannot close the captured cycle. -/
theorem missing_response_blocks :
    decide (openEvidence false false) = .blockMissingResponse := by
  rfl

/-- Inline materialization remains the preferred proof when both are present. -/
theorem inline_precedes_archive :
    decide (openEvidence true true) = .allow .inlineCommitSurface := by
  rfl

/- A bounded foreground delivery wait is not the durability boundary. Once the
   exact canonical target is retained and asynchronous recovery is active, the
   command may complete without claiming that the editor has acknowledged it. -/

inductive WriteCompletion where
  | visibleAndAcknowledged
  | retainedForAsyncDelivery
  | blockMissingRetention
  deriving DecidableEq, Repr

structure DeliveryEvidence where
  exactTargetRetained : Bool
  asyncDeliveryRecoveryActive : Bool
  deliveryConverged : Bool
  deriving DecidableEq, Repr

def decideDelivery (e : DeliveryEvidence) : WriteCompletion :=
  if e.deliveryConverged then
    .visibleAndAcknowledged
  else if e.exactTargetRetained && e.asyncDeliveryRecoveryActive then
    .retainedForAsyncDelivery
  else
    .blockMissingRetention

/-- A delayed ACK can complete only from the conjunction of exact retention and
    an active asynchronous recovery path. -/
theorem delayed_success_iff_retained_recovery (retained recovery : Bool) :
    decideDelivery {
      exactTargetRetained := retained
      asyncDeliveryRecoveryActive := recovery
      deliveryConverged := false
    } = .retainedForAsyncDelivery ↔ retained = true ∧ recovery = true := by
  cases retained <;> cases recovery <;> simp [decideDelivery]

/-- Missing exact canonical retention remains fail-closed even when a recovery
    signal exists. -/
theorem recovery_without_retention_blocks (recovery : Bool) :
    decideDelivery {
      exactTargetRetained := false
      asyncDeliveryRecoveryActive := recovery
      deliveryConverged := false
    } = .blockMissingRetention := by
  cases recovery <;> rfl

/-- Delivery convergence is stronger than deferred retention and therefore has
    precedence when all evidence is present. -/
theorem visible_ack_precedes_deferred :
    decideDelivery {
      exactTargetRetained := true
      asyncDeliveryRecoveryActive := true
      deliveryConverged := true
    } = .visibleAndAcknowledged := by
  rfl

inductive RetryAdmission where
  | startDrain
  | retainUntilBackoffExpires
  deriving DecidableEq, Repr

def decideRetry (backoffScheduled : Bool) : RetryAdmission :=
  if backoffScheduled then .retainUntilBackoffExpires else .startDrain

/-- External editor/file events cannot bypass an ACK retry backoff frontier. -/
theorem backoff_blocks_external_drain :
    decideRetry true = .retainUntilBackoffExpires := by
  rfl

inductive CommitTransport where
  | delegateOnAcquiredStream
  | fallbackLocal
  | blockLostStream
  deriving DecidableEq, Repr

def decideCommitTransport
    (streamAcquired sameStreamConsumed : Bool) : CommitTransport :=
  if !streamAcquired then
    .fallbackLocal
  else if sameStreamConsumed then
    .delegateOnAcquiredStream
  else
    .blockLostStream

/-- A controller liveness probe is delegation authority only when the request
    consumes that same stream; reconnecting after the probe is not allowed. -/
theorem delegation_iff_same_acquired_stream (acquired consumed : Bool) :
    decideCommitTransport acquired consumed = .delegateOnAcquiredStream ↔
      acquired = true ∧ consumed = true := by
  cases acquired <;> cases consumed <;> simp [decideCommitTransport]

end CaptureCloseout
