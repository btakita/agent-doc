#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# v1.8.0 is the upstream rolling prerelease: its tag and release asset are
# replaced by CI, so a pinned checksum eventually rejects a different binary at
# the same URL. Use the immutable stable release for reproducible model checks.
tools_version="1.7.4"
tools_sha256="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
tools_url="https://github.com/tlaplus/tlaplus/releases/download/v${tools_version}/tla2tools.jar"
tools_jar="${TLA_TOOLS_JAR:-${repo_root}/target/tla/tla2tools-${tools_version}.jar}"

if [[ -f "${tools_jar}" ]] && ! printf '%s  %s\n' "${tools_sha256}" "${tools_jar}" | sha256sum --check --status; then
  echo "[tla] cached tools checksum changed; refreshing ${tools_jar}" >&2
  rm -f "${tools_jar}"
fi

if [[ ! -f "${tools_jar}" ]]; then
    command -v curl >/dev/null 2>&1 || {
        echo "[tla] curl is required to download the pinned TLA+ tools" >&2
        exit 1
    }
    mkdir -p "$(dirname "${tools_jar}")"
    partial="${tools_jar}.partial"
    curl --fail --location --silent --show-error "${tools_url}" --output "${partial}"
    printf '%s  %s\n' "${tools_sha256}" "${partial}" | sha256sum --check --status
    mv "${partial}" "${tools_jar}"
fi

printf '%s  %s\n' "${tools_sha256}" "${tools_jar}" | sha256sum --check --status || {
    echo "[tla] checksum mismatch for ${tools_jar}" >&2
    exit 1
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/agent-doc-tla.XXXXXX")"
trap 'rm -rf "${work_dir}"' EXIT

modules=(AgentDocCloseout PassiveTmuxSync JetBrainsFileCache CloseoutChurn CrdtLineageFence ResponseCheckpoint PaneExecutionAuthority SupervisorGenerationTransition ReactiveTopology EditorReplicaStrand TransientRefusalLatch VisibleDeliveryReceipt PlanClosureContract IpcBuildIdentity StopHookContinuation StopHookFailClosed RefusedSaveOperatorAction RecycleSettleDispatch RetainedProjectionHold RetainedTransitionFixedPoint RealtimeSteeringStop EditorAuthorityLadder ConflictReconciliation AdmissionSplitMerge)

# Non-vacuity obligations. Each entry is `Module:Config` that MUST be reported as
# a violation. A safety or liveness property that cannot fail is not evidence,
# and this harness had been certifying exactly that: `JetBrainsFileCache` proved
# `EventuallyConverged` while production wedged for months, because its
# reregister step was an unconditional assignment that could not be rejected.
#
# There are two kinds of obligation here, and a module that models a recovery
# path wants both:
#
#   * a WEDGE config disables the recovery edge and must violate the liveness
#     property - proof that the edge is load-bearing rather than decorative;
#   * a REACH config asserts the negation of the desired end state and must be
#     violated too - proof that the happy path is still reachable, so a
#     conditional property cannot pass because its antecedent never holds.
#
# Together they mean the positive run is checking something. Either one alone
# leaves a way to go vacuous.
must_violate=(
    EditorReplicaStrand:EditorReplicaStrandWedge
    # `#acceptedneverserved` — one wedge PER EDGE, not one for the module. This
    # config leaves the ORIGINAL rejection edge enabled and disables only the
    # acceptance edge: if the rejection edge covered an endpoint that accepts
    # every request and never serves, it would pass. It must violate, which is
    # what proves the first fix never reached the state a cdylib reload actually
    # produces. A single module-level wedge would have stayed green on the
    # shipped edge while the production wedge stayed open — exactly how this one
    # survived the first fix.
    EditorReplicaStrand:EditorReplicaStrandAcceptedWedge
    # The write-side dual of `EditorReplicaStrand`: a definitively refusing
    # endpoint that keeps vetoing the visible-delivery receipt. Wedge proves
    # the drop edge is load-bearing; reach proves the editor-native save was
    # not abandoned in favour of the detached-write path.
    VisibleDeliveryReceipt:VisibleDeliveryReceiptWedge
    VisibleDeliveryReceipt:VisibleDeliveryReceiptBuildMismatchWedge
    VisibleDeliveryReceipt:VisibleDeliveryReceiptReach
    # The safety counterpart: a plan that pre-fills `--done` for work the
    # turn did not execute. Wedge proves the dispatch gate is load-bearing;
    # reach proves the plan still closes a genuinely dispatched head.
    PlanClosureContract:PlanClosureContractWedge
    PlanClosureContract:PlanClosureContractReach
    JetBrainsFileCache:JetBrainsFileCacheWedge
    JetBrainsFileCache:JetBrainsFileCacheReach
    # `TransientRefusalLatch` has one wedge PER KNOB rather than one for the
    # module. The three knobs are independent fixes for the same class, so a
    # single wedge would let two of them go vacuous the moment the third landed.
    TransientRefusalLatch:TransientRefusalLatchUnclassified
    TransientRefusalLatch:TransientRefusalLatchBlindRetry
    TransientRefusalLatch:TransientRefusalLatchCrashAsVerdict
    TransientRefusalLatch:TransientRefusalLatchReach
    # The identity the two modules above both consume. A build id taken from a
    # clock is wrong in both directions, so it gets one wedge PER DIRECTION: a
    # single wedge would let the surviving half go vacuous, which is exactly how
    # the shipped stamp hid its own false matches behind its false mismatches.
    IpcBuildIdentity:IpcBuildIdentityFalseMismatchWedge
    IpcBuildIdentity:IpcBuildIdentityFalseMatchWedge
    IpcBuildIdentity:IpcBuildIdentityReach
    # The Stop hook's continuation bound. One wedge per way the shipped bound
    # could be missing (nothing armed it, or a reconcile disarmed it), and two
    # reach configs because this fix ADDS a reason to allow the final answer --
    # "it stopped looping" and "it stopped working" are indistinguishable from
    # the outside, so both the block and the second block must stay reachable.
    StopHookContinuation:StopHookContinuationMarkerHostedWedge
    StopHookContinuation:StopHookContinuationUnrememberedWedge
    StopHookContinuation:StopHookContinuationReach
    StopHookContinuation:StopHookContinuationNoLatchReach
    # The hook's other refusal path, kept separate because its bound is a
    # different fact: within-stop (`stop_hook_active`) rather than cross-turn.
    # Modelling them together would let one bound stand in for the other.
    # `#refusedsaveopaque`: an endpoint that ANSWERED and refused every route
    # reported `operator_action=none`, so a retained write that could never
    # converge was indistinguishable from one in flight. Wedge proves the
    # terminal classification is what carries the invariant; Reach proves the
    # fix did not satisfy it by telling the operator to inspect the endpoint on
    # every ordinary retry.
    RefusedSaveOperatorAction:RefusedSaveOperatorActionWedge
    RefusedSaveOperatorAction:RefusedSaveOperatorActionReach
    # `#ambiguousholdforever2`: single-splice containment wedges on a marker-only
    # difference (fpe.md, 2026-09-29); the containment edge must stay reachable.
    RetainedProjectionHold:RetainedProjectionHoldWedge
    RetainedProjectionHold:RetainedProjectionHoldReach
    # `#replayafterack`: a retained delta already in the editor cut is its fixed
    # point; rebasing it again duplicated fpe.md's response (2026-09-29).
    RetainedTransitionFixedPoint:RetainedTransitionFixedPointWedge
    RetainedTransitionFixedPoint:RetainedTransitionFixedPointReach
    # `#queuetypingsteer`: a content_edit after a commit is steering too; the
    # prompt_target-only Stop hook left api.md's queue typing unanswered.
    RealtimeSteeringStop:RealtimeSteeringStopWedge
    RealtimeSteeringStop:RealtimeSteeringStopReach
    # `#admissionmergedup`: a line-based merge rung kept both of two edits of one
    # queue item (infra.md, 2026-09-30). One wedge for the pre-fix ladder, one
    # proving stop-on-equals is no substitute (the count guard carries the
    # invariant); reach proves disk-only work still lands.
    AdmissionSplitMerge:AdmissionSplitMergeWedge
    AdmissionSplitMerge:AdmissionSplitMergeGuardOffWedge
    AdmissionSplitMerge:AdmissionSplitMergeReach
    # `#editorauthority`: adopting the CRDT over an open editor rolls it back
    # (api.md, 2026-09-29); forward merging still delivers agent work.
    EditorAuthorityLadder:EditorAuthorityLadderWedge
    EditorAuthorityLadder:EditorAuthorityLadderReach
    # `#editorauth1`: the conflict-reconciliation merge. One wedge per rule it
    # could silently break (operator-first ordering, last-writer-wins on an
    # overlap); reach proves a same-span conflict is actually surfaced.
    ConflictReconciliation:ConflictReconciliationOperatorFirstWedge
    ConflictReconciliation:ConflictReconciliationDropWedge
    ConflictReconciliation:ConflictReconciliationReach
    StopHookFailClosed:StopHookFailClosedWedge
    StopHookFailClosed:StopHookFailClosedReach
    # `#recyclesettlewaitshort` — the dispatch gate across an upgrade re-exec.
    # The wedge restores the shipped one-timeout-is-a-verdict rule and must
    # violate, which is what proves the re-arm edge carries the invariant. Two
    # reach configs because this fix REMOVES a refusal: "it stopped refusing
    # wrongly" and "it stopped refusing at all" are indistinguishable from the
    # outside, so delivery across a live recycle AND the surviving unstamped
    # fail-closed refusal must both stay reachable.
    RecycleSettleDispatch:RecycleSettleDispatchWedge
    RecycleSettleDispatch:RecycleSettleDispatchReach
    RecycleSettleDispatch:RecycleSettleDispatchUnstampedReach
)

for module in "${modules[@]}"; do
    cp "${repo_root}/formal/tla/${module}.tla" "${work_dir}/"
    cp "${repo_root}/formal/tla/${module}.cfg" "${work_dir}/"
done
for entry in "${must_violate[@]}"; do
    cp "${repo_root}/formal/tla/${entry#*:}.cfg" "${work_dir}/"
done

(
    cd "${work_dir}"
for module in "${modules[@]}"; do
    if grep -Eq '\(\* --(fair )?algorithm' "${module}.tla"; then
      java -XX:+UseParallelGC -cp "${tools_jar}" pcal.trans "${module}.tla"
    fi
# `-metadir` is explicit because TLC otherwise derives it from the current time
# to the SECOND, and two checks that start inside the same second collide with
# "that directory already exists". With the same module now checked under several
# configs that is not a rare race, it is the common case.
java -XX:+UseParallelGC -cp "${tools_jar}" tlc2.TLC -workers auto \
    -metadir "${work_dir}/states-${module}" "${module}.tla"
done

for entry in "${must_violate[@]}"; do
    module="${entry%%:*}"
    config="${entry#*:}"
    log="${module}-${config}.log"
    # `set -e` is active, so guard the intentionally-failing run.
    if java -XX:+UseParallelGC -cp "${tools_jar}" tlc2.TLC -workers auto \
        -metadir "${work_dir}/states-${module}-${config}" \
        -config "${config}.cfg" "${module}.tla" >"${log}" 2>&1; then
        echo "[tla] NON-VACUITY FAILURE: ${module} with ${config}.cfg was expected to" >&2
        echo "[tla] report a violation and instead passed. The recovery edge that" >&2
        echo "[tla] config disables is no longer load-bearing, or the property no" >&2
        echo "[tla] longer constrains it - the positive run is now vacuous." >&2
        tail -40 "${log}" >&2
        exit 1
    fi
    if ! grep -Eq 'Error: (Deadlock reached|Temporal properties were violated|Invariant .* is violated)' "${log}"; then
        echo "[tla] NON-VACUITY FAILURE: ${module} with ${config}.cfg failed, but not" >&2
        echo "[tla] with a deadlock / property violation - so the wedge it is meant to" >&2
        echo "[tla] exhibit was not what TLC actually reported." >&2
        tail -40 "${log}" >&2
        exit 1
    fi
    echo "[tla] non-vacuity confirmed: ${module} with ${config}.cfg violates as required"
done
)
