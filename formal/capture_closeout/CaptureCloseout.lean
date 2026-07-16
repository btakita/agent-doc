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

/- A plugin package restart and a native cdylib reload are distinct generation
   transitions. Replay/ACK boundaries adopt a newer registered editor-plugin
   generation; a native-only reload cannot manufacture that evidence. -/

inductive GenerationDecision where
  | keepPreflight
  | adoptLive
  deriving DecidableEq, Repr

structure GenerationEvidence where
  preflightGeneration : Nat
  liveGeneration : Nat
  liveRegistrationObserved : Bool
  deriving DecidableEq, Repr

def decideGeneration (e : GenerationEvidence) : GenerationDecision :=
  if e.liveRegistrationObserved && e.preflightGeneration < e.liveGeneration then
    .adoptLive
  else
    .keepPreflight

theorem newer_registered_generation_supersedes_preflight (old live : Nat)
    (h : old < live) :
    decideGeneration {
      preflightGeneration := old
      liveGeneration := live
      liveRegistrationObserved := true
    } = .adoptLive := by
  simp [decideGeneration, h]

theorem unregistered_generation_cannot_supersede (old live : Nat) :
    decideGeneration {
      preflightGeneration := old
      liveGeneration := live
      liveRegistrationObserved := false
    } = .keepPreflight := by
  simp [decideGeneration]

structure RuntimeGenerations where
  pluginGeneration : Nat
  nativeGeneration : Nat
  deriving DecidableEq, Repr

def reloadNativeOnly (s : RuntimeGenerations) (nextNative : Nat) : RuntimeGenerations :=
  { s with nativeGeneration := nextNative }

theorem native_reload_does_not_upgrade_plugin (s : RuntimeGenerations) (next : Nat) :
    (reloadNativeOnly s next).pluginGeneration = s.pluginGeneration := by
  rfl

/- `make install` owns two distinct transitions: it converges every existing
package on disk and refreshes the native generation, but it cannot claim that a
running editor has activated the package before the editor restarts. -/

structure InstallState where
  sourceGeneration : Nat
  installedPackages : List Nat
  liveGeneration : Nat
  nativeGeneration : Nat
deriving DecidableEq, Repr

def makeInstall (s : InstallState) : InstallState :=
  { s with
      installedPackages := s.installedPackages.map (fun _ => s.sourceGeneration)
      nativeGeneration := s.sourceGeneration }

def restartEditorAfterInstall (s : InstallState) : InstallState :=
  { s with liveGeneration := s.sourceGeneration }

theorem make_install_preserves_package_count (s : InstallState) :
    (makeInstall s).installedPackages.length = s.installedPackages.length := by
  simp [makeInstall]

theorem make_install_converges_every_existing_package (s : InstallState) :
    ∀ generation ∈ (makeInstall s).installedPackages,
      generation = s.sourceGeneration := by
  intro generation member
  simp [makeInstall] at member
  exact member.2.symm

theorem make_install_does_not_claim_live_activation (s : InstallState) :
    (makeInstall s).liveGeneration = s.liveGeneration := by
  rfl

theorem restart_after_install_publishes_source_generation (s : InstallState) :
    (restartEditorAfterInstall (makeInstall s)).liveGeneration = s.sourceGeneration := by
  rfl

/- A retained response may adopt a newer authoritative cut only when the cut is
   a monotonic extension of the matching open-cycle baseline. This admits later
   steering while blocking edits, deletions, reorders, and unrelated captures. -/

inductive RebaseDecision where
  | keepBaseline
  | rebaseToAuthoritativeCurrent
  | blockConflict
  deriving DecidableEq, Repr

structure RebaseEvidence where
  captureRepairable : Bool
  baselineDrifted : Bool
  authoritativeCurrent : Bool
  matchingOpenCycle : Bool
  responseMissing : Bool
  responseHeadingAnswered : Bool
  monotonicExtension : Bool
  deriving DecidableEq, Repr

def decideRebase (e : RebaseEvidence) : RebaseDecision :=
  if !e.baselineDrifted then
    .keepBaseline
  else if e.captureRepairable && e.authoritativeCurrent && e.matchingOpenCycle &&
      e.responseMissing && !e.responseHeadingAnswered && e.monotonicExtension then
    .rebaseToAuthoritativeCurrent
  else
    .blockConflict

theorem safe_authoritative_rebase (answered : Bool) :
    decideRebase {
      captureRepairable := true
      baselineDrifted := true
      authoritativeCurrent := true
      matchingOpenCycle := true
      responseMissing := true
      responseHeadingAnswered := answered
      monotonicExtension := true
    } = (if answered then .blockConflict else .rebaseToAuthoritativeCurrent) := by
  cases answered <;> rfl

theorem non_monotonic_cut_blocks_rebase (authoritative : Bool) :
    decideRebase {
      captureRepairable := true
      baselineDrifted := true
      authoritativeCurrent := authoritative
      matchingOpenCycle := true
      responseMissing := true
      responseHeadingAnswered := false
      monotonicExtension := false
    } = .blockConflict := by
  cases authoritative <;> rfl

structure RetainedRecovery where
  responseToken : Nat
  steeringToken : Nat
  baselineGeneration : Nat
  deriving DecidableEq, Repr

def adoptAuthoritativeBaseline (s : RetainedRecovery) (generation : Nat) : RetainedRecovery :=
  { s with baselineGeneration := generation }

theorem baseline_adoption_preserves_response (s : RetainedRecovery) (generation : Nat) :
    (adoptAuthoritativeBaseline s generation).responseToken = s.responseToken := by
  rfl

theorem baseline_adoption_preserves_later_steering (s : RetainedRecovery) (generation : Nat) :
    (adoptAuthoritativeBaseline s generation).steeringToken = s.steeringToken := by
  rfl

end CaptureCloseout
