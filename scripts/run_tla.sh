#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/run_tla.sh             model-check every module under formal/tla
#        scripts/run_tla.sh --self-test check the runner's own isolation (below)

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# v1.8.0 is the upstream rolling prerelease: its tag and release asset are
# replaced by CI, so a pinned checksum eventually rejects a different binary at
# the same URL. Use the immutable stable release for reproducible model checks.
tools_version="1.7.4"
tools_sha256="936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
tools_url="https://github.com/tlaplus/tlaplus/releases/download/v${tools_version}/tla2tools.jar"
tools_jar="${TLA_TOOLS_JAR:-${repo_root}/target/tla/tla2tools-${tools_version}.jar}"

# --- Run isolation -----------------------------------------------------------
#
# TLC does not read the standard modules (Naturals, Sequences, FiniteSets, TLC,
# ...) out of the jar. `util.SimpleFilenameToStream` copies each one to the
# FIXED path `${java.io.tmpdir}/<Module>.tla`, marks it `deleteOnExit`, and only
# then checks that the file exists and parses it. `java.io.tmpdir` is `/tmp` on
# Linux whatever `TMPDIR` says, so every TLC process on the host shares those
# files: one process exiting deletes the `Naturals.tla` another has just written,
# and a concurrent rewrite hands a reader a truncated `TLC.tla`. That is how a
# `make tla` running beside another checkout's `make tla` failed with
#
#   File does not exist: /tmp/Naturals.tla while looking in these directories:
#   jar:file:.../tla2tools-1.7.4.jar!/tla2sany/StandardModules/
#
# while the jar itself was intact. Every java process below therefore gets a
# `java.io.tmpdir` private to this run, and runs from a checksum-verified
# snapshot of the jar inside the run's own work dir, so a concurrent refresh or
# clean of `target/tla` cannot change the classpath under an in-flight run.

die() {
    echo "$*" >&2
    exit 1
}

jar_ok() {
    printf '%s  %s\n' "${tools_sha256}" "$1" | sha256sum --check --status
}

# Make `tools_jar` a verified copy of the pinned release. A stale or corrupt
# cache is replaced by an atomic rename over it - never deleted first - so a
# concurrent run that is snapshotting it sees either the old file or the new
# one, never a missing or half-written jar.
ensure_tools_jar() {
    if [[ -f "${tools_jar}" ]] && jar_ok "${tools_jar}"; then
        return 0
    fi
    if [[ -f "${tools_jar}" ]]; then
        echo "[tla] cached tools checksum changed; refreshing ${tools_jar}" >&2
    fi
    command -v curl >/dev/null 2>&1 ||
        die "[tla] curl is required to download the pinned TLA+ tools"
    mkdir -p "$(dirname "${tools_jar}")"
    local partial
    partial="$(mktemp "${tools_jar}.partial.XXXXXX")"
    if ! curl --fail --location --silent --show-error "${tools_url}" --output "${partial}" ||
        ! jar_ok "${partial}"; then
        rm -f "${partial}"
        die "[tla] download of ${tools_url} failed or did not match the pinned SHA-256"
    fi
    mv -f "${partial}" "${tools_jar}"
}

# Point this run at `$1`: snapshot the verified jar into it and give java a
# private temp dir there. Sets `tools_snapshot` and `java_tmp`.
isolate_run() {
    tools_snapshot="$1/tla2tools.jar"
    java_tmp="$1/java-tmp"
    mkdir -p "${java_tmp}"
    cp "${tools_jar}" "${tools_snapshot}"
    jar_ok "${tools_snapshot}" ||
        die "[tla] ${tools_jar} changed while it was being snapshotted (checksum mismatch on the copy); rerun"
}

tla_java() {
    java -XX:+UseParallelGC "-Djava.io.tmpdir=${java_tmp}" -cp "${tools_snapshot}" "$@"
}

# True when a TLC log failed because the standard modules could not be loaded -
# an environment fault, never a verdict about the model.
standard_module_failure() {
    grep -Eq 'File does not exist: .* while looking in these directories: jar:.*StandardModules|Could not parse module (Naturals|Integers|Reals|Sequences|FiniteSets|Bags|TLC|TLCExt|RealTime|Json|Randomization|Toolbox) from file' "$1"
}

standard_module_diagnostic() {
    echo "[tla] ENVIRONMENT FAILURE, not a model result: TLC could not load a standard" >&2
    echo "[tla] module (Naturals, TLC, ...). TLC extracts them from the tools jar into" >&2
    echo "[tla] java.io.tmpdir, which this run pins to ${java_tmp}. Check that the" >&2
    echo "[tla] directory is writable and was not removed mid-run, and that" >&2
    echo "[tla] JAVA_TOOL_OPTIONS / _JAVA_OPTIONS do not override java.io.tmpdir." >&2
}

# Run one model check that must pass. Output still streams to the console; the
# copy in `$1` lets a failure be classified.
check_passes() {
    local log="$1"
    shift
    if ! tla_java tlc2.TLC "$@" 2>&1 | tee "${log}"; then
        if standard_module_failure "${log}"; then
            standard_module_diagnostic
        fi
        die "[tla] model check failed: tlc2.TLC $*"
    fi
}

# --- Self-test ---------------------------------------------------------------
#
# Deterministic, not a race: a hostile `JAVA_TOOL_OPTIONS` points the JVM's
# default temp dir at a directory that does not exist, which fails the standard
# module lookup with exactly the production error text. The isolated runner must
# pass under it; an unisolated java must fail under it (so the check has teeth);
# and a run must survive its source jar being truncated after the snapshot.
self_test() {
    ensure_tools_jar
    selftest_dir="$(mktemp -d "${TMPDIR:-/tmp}/agent-doc-tla-selftest.XXXXXX")"
    trap 'rm -rf "${selftest_dir}"' EXIT
    local dir="${selftest_dir}"
    cat >"${dir}/Tiny.tla" <<'TLA'
---- MODULE Tiny ----
EXTENDS Naturals, Sequences, FiniteSets, TLC
VARIABLE x
Init == x = 0
Next == x < 2 /\ x' = x + 1
Spec == Init /\ [][Next]_x
====
TLA
    printf 'SPECIFICATION Spec\n' >"${dir}/Tiny.cfg"
    local hostile="-Djava.io.tmpdir=${dir}/no-such-dir"

    # 1. Non-vacuity: without a private tmpdir the hostile default breaks TLC
    #    with the production error, and the classifier recognises it.
    if (cd "${dir}" && JAVA_TOOL_OPTIONS="${hostile}" java -cp "${tools_jar}" tlc2.TLC \
        -deadlock -workers 1 -metadir "${dir}/states-bare" Tiny.tla >"${dir}/bare.log" 2>&1); then
        cat "${dir}/bare.log" >&2
        die "[tla] self-test: an unisolated TLC passed with java.io.tmpdir unusable - the check below would be vacuous"
    fi
    standard_module_failure "${dir}/bare.log" || {
        cat "${dir}/bare.log" >&2
        die "[tla] self-test: unisolated failure was not classified as a standard-module failure"
    }

    # 2. The isolated runner passes under the same hostile default.
    mkdir -p "${dir}/run"
    isolate_run "${dir}/run"
    (cd "${dir}" && JAVA_TOOL_OPTIONS="${hostile}" tla_java tlc2.TLC \
        -deadlock -workers 1 -metadir "${dir}/states-isolated" Tiny.tla >"${dir}/isolated.log" 2>&1) || {
        cat "${dir}/isolated.log" >&2
        die "[tla] self-test: TLC failed although java.io.tmpdir was pinned to the run's work dir"
    }

    # 3. A run keeps working after its source jar is truncated (a concurrent
    #    refresh or clean of target/tla in the same checkout).
    cp "${tools_jar}" "${dir}/source.jar"
    local saved_jar="${tools_jar}"
    tools_jar="${dir}/source.jar"
    mkdir -p "${dir}/run2"
    isolate_run "${dir}/run2"
    : >"${dir}/source.jar"
    (cd "${dir}" && tla_java tlc2.TLC -deadlock -workers 1 \
        -metadir "${dir}/states-snapshot" Tiny.tla >"${dir}/snapshot.log" 2>&1) || {
        cat "${dir}/snapshot.log" >&2
        die "[tla] self-test: TLC failed after the source jar was truncated - the run is not using its snapshot"
    }
    tools_jar="${saved_jar}"

    # 4. A model verdict is not misread as an environment failure.
    if (cd "${dir}" && tla_java tlc2.TLC -workers 1 \
        -metadir "${dir}/states-deadlock" Tiny.tla >"${dir}/deadlock.log" 2>&1); then
        die "[tla] self-test: the deadlocking Tiny model passed"
    fi
    grep -q 'Error: Deadlock reached' "${dir}/deadlock.log" ||
        die "[tla] self-test: expected a deadlock verdict from Tiny"
    if standard_module_failure "${dir}/deadlock.log"; then
        die "[tla] self-test: a deadlock verdict was classified as a standard-module failure"
    fi

    echo "[tla] runner self-test: ok"
}

if [[ "${1:-}" == "--self-test" ]]; then
    self_test
    exit 0
fi
[[ $# -eq 0 ]] || die "usage: $0 [--self-test]"

# --- Content receipt ----------------------------------------
#
# A TLC verdict is a function of the model files, this runner (module lists,
# must-pass / must-violate obligations, pinned tools version), the JVM, and the
# JVM option variables. When all of those are byte-identical to a run that
# passed, rerunning every check (~125 TLC processes) proves nothing new. The
# receipt is content-addressed and lives in the user cache, so it is shared by
# every worktree of this repository (the same rule as sccache: only
# content-addressed results cross worktrees). Any change to an input yields a
# different fingerprint and a full run. AGENT_DOC_TLA_FORCE=1 always runs.
tla_fingerprint() {
    {
        printf 'runner\0'
        sha256sum "${BASH_SOURCE[0]}" | cut -d' ' -f1
        (cd "${repo_root}/formal/tla" && find . -type f -print0 | LC_ALL=C sort -z |
            xargs -0 sha256sum)
        printf 'java\0'
        java -version 2>&1 || echo "java-unavailable"
        printf 'env\0%s\0%s\0%s\0' "${JAVA_TOOL_OPTIONS-}" "${_JAVA_OPTIONS-}" "${TLA_TOOLS_JAR-}"
    } | sha256sum | cut -d' ' -f1
}

receipt_dir="${AGENT_DOC_CHECK_RECEIPT_DIR:-${XDG_CACHE_HOME:-${HOME:-/nonexistent}/.cache}/agent-doc/check-receipts}"
receipt="${receipt_dir}/tla-$(tla_fingerprint)"

record_receipt() {
    mkdir -p "${receipt_dir}" 2>/dev/null || return 0
    printf 'tla passed at %s in %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "${repo_root}" >"${receipt}.tmp.$$" &&
        mv -f "${receipt}.tmp.$$" "${receipt}" || true
}

if [[ "${AGENT_DOC_TLA_FORCE:-0}" != 1 && -f "${receipt}" ]]; then
    echo "[tla] model files, runner, and JVM are identical to a passing run ($(cat "${receipt}"));"
    echo "[tla] skipping. Receipt: ${receipt}. Set AGENT_DOC_TLA_FORCE=1 to rerun every check."
    exit 0
fi

ensure_tools_jar

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/agent-doc-tla.XXXXXX")"
trap 'rm -rf "${work_dir}"' EXIT
isolate_run "${work_dir}"

modules=(AgentDocCloseout PassiveTmuxSync JetBrainsFileCache CloseoutChurn CrdtLineageFence ResponseCheckpoint PaneExecutionAuthority SupervisorGenerationTransition ReactiveTopology EditorReplicaStrand TransientRefusalLatch VisibleDeliveryReceipt PlanClosureContract IpcBuildIdentity StopHookContinuation StopHookFailClosed RefusedSaveOperatorAction RecycleSettleDispatch RetainedProjectionHold RegistrationAcknowledgedBase RetainedTransitionFixedPoint RealtimeSteeringStop EditorAuthorityLadder ConflictReconciliation AdmissionSplitMerge AdmissionCutExclusivity StaleColumnRecycle NetChannelRetransmit VisibleDeliveryReceiptNet EditorReplicaStrandNet RecycleSettleDispatchNet AgentDocCloseoutNet PassiveTmuxSyncNet PassiveTmuxSyncRetireNet LifecycleSequence LayoutWidthBound RemoteLayoutNoGuess EditorTransportHealth ControllerLifecycle)

# Library modules: INSTANCE'd by the models above, never checked on their own
# (no Spec, no .cfg). They only need to sit next to the importing model.
libraries=(NetChannel)

# Extra POSITIVE configs: `Module:Config` that must pass like `Module.cfg`.
# `#netadv3` splits a model's liveness run (small bounds, so strong fairness per
# message stays tractable) from its safety run under the full adversary.
must_pass=(
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetSafety
    PassiveTmuxSyncNet:PassiveTmuxSyncNetSafety
    # `#surfaceretireformal`: the retirement channel. Safety twice (reconnect at
    # the larger bound, duplication at the smaller one: both at once is >10^8
    # states), liveness with duplication, and SelfHeal: the shipped
    # self-retirement edge put BACK still converges once a live endpoint
    # re-registers on the typed retired receipt (recovery by construction).
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetSafety
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetDuplicateSafety
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetDuplicateLive
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetSelfHeal
    LifecycleSequence:LifecycleSequenceSafety
    # Target design for ADC-fence (NOT in code yet): a fenced commit is what
    # makes one-commit-per-cycle hold; the Fence wedge below shows the gap.
    AgentDocCloseoutNet:AgentDocCloseoutNetFencedTarget
    # `#keylogrebase`: a correctly based op ledger closes the disk-only
    # residual of `#queuecutexclusive`.
    AdmissionCutExclusivity:AdmissionCutExclusivityResidual
)

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
    # `#ackedreplicabase`: the shadow-only proof holds beside a disjoint controller
    # write while the operator keeps typing (agent-doc-bugs.md, 2026-10-28); the
    # acknowledged-base merge edge must stay reachable.
    RegistrationAcknowledgedBase:RegistrationAcknowledgedBaseWedge
    RegistrationAcknowledgedBase:RegistrationAcknowledgedBaseReach
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
    # typingforkdup: disk held an intermediate revision of the queue item the
    # operator was typing (sdk.md, 2026-10-11); the count guard admits it as a
    # disk addition. Collapse off MUST duplicate; collapsing each rung's OUTPUT
    # (instead of projecting the input) MUST duplicate too, and so must a NEWER
    # disk revision with the twin-pair guard off.
    AdmissionSplitMerge:AdmissionSplitMergeTwinWedge
    AdmissionSplitMerge:AdmissionSplitMergeTwinPairWedge
    AdmissionSplitMerge:AdmissionSplitMergeTwinOutputWedge
    # `#queuecutexclusive`: the pre-fix ladder with every heuristic guard on
    # still duplicates (stale cut, and the 2026-10-15 in-flight cut); the
    # shipped rule still merges independent disk work (Reach).
    # `#keylogrebase`: the text-only rule cannot close the disk-only residual
    # (ResidualTextOnly), provenance over the pre-fix mislabelled ledger cannot
    # either (ResidualMislabelled), and the remaining boundary -- an uncovered
    # ledger, or typing and independent work in one component -- is real.
    AdmissionCutExclusivity:AdmissionCutExclusivityLadderWedge
    AdmissionCutExclusivity:AdmissionCutExclusivityInflightWedge
    AdmissionCutExclusivity:AdmissionCutExclusivityReach
    AdmissionCutExclusivity:AdmissionCutExclusivityResidualTextOnly
    AdmissionCutExclusivity:AdmissionCutExclusivityResidualMislabelled
    AdmissionCutExclusivity:AdmissionCutExclusivityBoundary
    AdmissionSplitMerge:AdmissionSplitMergeTwinReach
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
    # `#netadv5` R9: an elapsed TTL is not proof the settle was lost; with the
    # supervisor alive, proceeding would inject across a live hot-reload.
    RecycleSettleDispatch:RecycleSettleDispatchTtlWedge
    # GH #136 — a stale stash supervisor pane admitted as a layout column on the
    # strength of a recycle request that was never consumed. One wedge PER FIX
    # (overdue bound, no-widen, first-unconsumed clock, non-lapsing fan-out,
    # durable level-triggered request instead of a droppable notification);
    # reach proves the focus exception and consumption both stay reachable.
    StaleColumnRecycle:StaleColumnRecycleOverdueWedge
    StaleColumnRecycle:StaleColumnRecycleWidenWedge
    StaleColumnRecycle:StaleColumnRecycleRefreshWedge
    StaleColumnRecycle:StaleColumnRecycleLapseWedge
    StaleColumnRecycle:StaleColumnRecycleDropWedge
    StaleColumnRecycle:StaleColumnRecycleReach
    StaleColumnRecycle:StaleColumnRecycleConsumeReach
    # `#netadv2`: the shared adversarial channel (NetChannel) must have teeth.
    # One wedge per obligation the channel imposes on a protocol over it:
    # fire-and-forget stalls on a Drop, a non-idempotent receiver applies a
    # Duplicate twice, a receiver without a generation check applies a message
    # that survived a Reconnect, and without FairLossy the adversary may drop
    # every resend forever. Reach proves the positive run completes after the
    # adversary actually duplicated and carried a message across a reconnect.
    NetChannelRetransmit:NetChannelRetransmitFireAndForgetWedge
    NetChannelRetransmit:NetChannelRetransmitDuplicateWedge
    NetChannelRetransmit:NetChannelRetransmitStaleGenWedge
    NetChannelRetransmit:NetChannelRetransmitUnfairWedge
    NetChannelRetransmit:NetChannelRetransmitReach
    # `#netadv3`: the hot-path models re-checked over NetChannel. One wedge per
    # fix (each is the shipped pre-fix behaviour and MUST violate), plus the
    # timeout-as-verdict wedges whose code fixes are owned by `#netadv5`
    # (TimeoutRefusal, GiveUpRefusal, Ttl) and the unfixed ADC-fence gap.
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetWakeOneShotWedge
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetRecoveryLatchWedge
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetSaveOneShotWedge
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetStaleAckWedge
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetTimeoutRefusalWedge
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetReach
    VisibleDeliveryReceiptNet:VisibleDeliveryReceiptNetDropReach
    EditorReplicaStrandNet:EditorReplicaStrandNetLatchWedge
    EditorReplicaStrandNet:EditorReplicaStrandNetGiveUpRefusalWedge
    EditorReplicaStrandNet:EditorReplicaStrandNetReach
    RecycleSettleDispatchNet:RecycleSettleDispatchNetUnreachableWedge
    RecycleSettleDispatchNet:RecycleSettleDispatchNetTtlWedge
    RecycleSettleDispatchNet:RecycleSettleDispatchNetReach
    RecycleSettleDispatchNet:RecycleSettleDispatchNetAbandonReach
    RecycleSettleDispatchNet:RecycleSettleDispatchNetUnstampedReach
    AgentDocCloseoutNet:AgentDocCloseoutNetOneShotClaimWedge
    AgentDocCloseoutNet:AgentDocCloseoutNetHeartbeatBreakWedge
    AgentDocCloseoutNet:AgentDocCloseoutNetIgnoreLossWedge
    AgentDocCloseoutNet:AgentDocCloseoutNetFenceWedge
    AgentDocCloseoutNet:AgentDocCloseoutNetReach
    PassiveTmuxSyncNet:PassiveTmuxSyncNetAdvanceFirstWedge
    PassiveTmuxSyncNet:PassiveTmuxSyncNetReorderWedge
    PassiveTmuxSyncNet:PassiveTmuxSyncNetReach
    # `#tmuxautosyncretired` / `#surfaceretireformal`: the editor-surface
    # retirement channel the model above never contained. WrongRetire is the
    # v0.35.502/503 self-retirement (TLC finds the 02:54:03Z trace);
    # FallbackMute is the Remote Dev sibling (a backend fallback resuming on a
    # frame the frontend handoff retired, same per-process generation) with no
    # re-registration; ExactForget proves the forget fence raise is
    # load-bearing once re-registration exists; NoFence proves the recovery did
    # not lift the retired fence. Reach: a live endpoint re-registers.
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetWrongRetireWedge
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetFallbackMuteWedge
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetExactForgetWedge
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetNoFenceWedge
    PassiveTmuxSyncRetireNet:PassiveTmuxSyncRetireNetReach
    LifecycleSequence:LifecycleSequenceReorderWedge
    LifecycleSequence:LifecycleSequenceReach
    # GH #136(a): every publisher runs over NetChannel. The wedge proves the
    # derived clamp carries the safety invariant; reach proves explicit editor
    # widening and a later derived trim both remain possible under adversity.
    LayoutWidthBound:LayoutWidthBoundWedge
    LayoutWidthBound:LayoutWidthBoundReach
    # GH #343: selection-only input never changes structure. Wedge restores
    # slot replacement; reach proves the later positive correction is live.
    RemoteLayoutNoGuess:RemoteLayoutNoGuessWedge
    RemoteLayoutNoGuess:RemoteLayoutNoGuessReach
    # GH #138: two refusal-producing editor paths logged their definitive NO
    # without moving transport health. Wedge proves that transition carries the
    # invariant; Reach proves the unregister state remains attainable.
    EditorTransportHealth:EditorTransportHealthWedge
    EditorTransportHealth:EditorTransportHealthReach
    # `#supthrash` — one wedge per lifecycle guard, plus reach witnesses.
    ControllerLifecycle:ControllerLifecycleRetryWedge
    ControllerLifecycle:ControllerLifecycleClosedOwnerWedge
    ControllerLifecycle:ControllerLifecycleNoIdleTickWedge
    ControllerLifecycle:ControllerLifecycleRetireReach
    ControllerLifecycle:ControllerLifecycleAbandonReach
    ControllerLifecycle:ControllerLifecyclePromoteReach
)

for module in "${modules[@]}"; do
    cp "${repo_root}/formal/tla/${module}.tla" "${work_dir}/"
    cp "${repo_root}/formal/tla/${module}.cfg" "${work_dir}/"
done
for library in "${libraries[@]}"; do
    cp "${repo_root}/formal/tla/${library}.tla" "${work_dir}/"
done
for entry in "${must_violate[@]}" "${must_pass[@]}"; do
    cp "${repo_root}/formal/tla/${entry#*:}.cfg" "${work_dir}/"
done

# Check turnaround: one TLC JVM per check, run `tla_jobs` at a time. Every check
# was a serial JVM with `-workers auto`; most models finish in well under a
# second, so the ~125 checks were dominated by JVM start-up (~235s in total).
# Each job gets its own `java.io.tmpdir` (see "Run isolation" above: TLC's
# standard-module extraction races between processes sharing one), its own
# metadir, and its own log. Verdicts are evaluated afterwards in the original
# order with exactly the same pass / must-violate rules, so a parallel run
# reports the same failure a serial one would.
tla_jobs="${TLA_JOBS:-}"
if [[ -z "${tla_jobs}" ]]; then
    cpus="$(nproc 2>/dev/null || echo 4)"
    tla_jobs=$(( cpus / 4 ))
    (( tla_jobs > 8 )) && tla_jobs=8
fi
(( tla_jobs >= 1 )) || tla_jobs=1
# Workers stay `auto` (one per core) per JVM: the few large models dominate the
# critical path, and once the short checks drain they get every core. A fixed
# split (cores / jobs) left AgentDocCloseoutNet at 126s on 4 workers.
tla_workers="${TLA_WORKERS:-auto}"
# Longest-first scheduling from the previous run's wall times (ordering only;
# it never changes which checks run or how they are judged).
durations="${receipt_dir}/tla-durations.tsv"

# run_job ID MODULE [CONFIG]: run one TLC check in the background-safe way.
# Never fails; the exit status lands in `${ID}.status`, wall time in `${ID}.secs`.
run_job() {
    local id="$1" module="$2" config="${3:-}"
    local java_tmp="${work_dir}/java-tmp-${id}"
    mkdir -p "${java_tmp}"
    local args=(-workers "${tla_workers}" -metadir "${work_dir}/states-${id}")
    [[ -n "${config}" ]] && args+=(-config "${config}.cfg")
    local start=${SECONDS} rc=0
    tla_java tlc2.TLC "${args[@]}" "${module}.tla" >"${id}.log" 2>&1 || rc=$?
    echo "${rc}" >"${id}.status"
    echo $(( SECONDS - start )) >"${id}.secs"
}

(
    cd "${work_dir}"
    for module in "${modules[@]}"; do
        if grep -Eq '\(\* --(fair )?algorithm' "${module}.tla"; then
            tla_java pcal.trans "${module}.tla"
        fi
    done

    # `-metadir` is explicit and per job because TLC otherwise derives it from
    # the current time to the SECOND, and two checks that start inside the same
    # second collide with "that directory already exists".
    jobs_list=()
    for module in "${modules[@]}"; do
        jobs_list+=("${module}|${module}|")
    done
    for entry in "${must_pass[@]}" "${must_violate[@]}"; do
        jobs_list+=("${entry%%:*}-${entry#*:}|${entry%%:*}|${entry#*:}")
    done

    if [[ -f "${durations}" ]]; then
        mapfile -t jobs_list < <(
            for job in "${jobs_list[@]}"; do
                id="${job%%|*}"
                secs="$(awk -F'\t' -v id="${id}" '$1 == id { print $2; exit }' "${durations}")"
                printf '%s\t%s\n' "${secs:-0}" "${job}"
            done | sort -t$'\t' -k1,1rn -s | cut -f2-
        )
    fi
    echo "[tla] ${#jobs_list[@]} checks, ${tla_jobs} at a time, -workers ${tla_workers} each"
    running=0
    for job in "${jobs_list[@]}"; do
        IFS='|' read -r id module config <<<"${job}"
        run_job "${id}" "${module}" "${config}" &
        running=$(( running + 1 ))
        if (( running >= tla_jobs )); then
            wait -n || true
            running=$(( running - 1 ))
        fi
    done
    wait || true

    for job in "${jobs_list[@]}"; do
        IFS='|' read -r id _ _ <<<"${job}"
        [[ -f "${id}.status" ]] || die "[tla] check ${id} produced no status; the job runner lost it"
    done

    # Positive runs: every one must pass (output kept on stdout as before).
    for module in "${modules[@]}"; do
        cat "${module}.log"
        if [[ "$(cat "${module}.status")" != 0 ]]; then
            standard_module_failure "${module}.log" && standard_module_diagnostic
            die "[tla] model check failed: tlc2.TLC ${module}.tla"
        fi
    done
    for entry in "${must_pass[@]}"; do
        module="${entry%%:*}"
        config="${entry#*:}"
        id="${module}-${config}"
        cat "${id}.log"
        if [[ "$(cat "${id}.status")" != 0 ]]; then
            standard_module_failure "${id}.log" && standard_module_diagnostic
            die "[tla] model check failed: tlc2.TLC -config ${config}.cfg ${module}.tla"
        fi
    done

    for entry in "${must_violate[@]}"; do
        module="${entry%%:*}"
        config="${entry#*:}"
        log="${module}-${config}.log"
        if [[ "$(cat "${module}-${config}.status")" == 0 ]]; then
            echo "[tla] NON-VACUITY FAILURE: ${module} with ${config}.cfg was expected to" >&2
            echo "[tla] report a violation and instead passed. The recovery edge that" >&2
            echo "[tla] config disables is no longer load-bearing, or the property no" >&2
            echo "[tla] longer constrains it - the positive run is now vacuous." >&2
            tail -40 "${log}" >&2
            exit 1
        fi
        if standard_module_failure "${log}"; then
            tail -40 "${log}" >&2
            standard_module_diagnostic
            die "[tla] ${module} with ${config}.cfg did not run; its non-vacuity obligation is unchecked"
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

    for job in "${jobs_list[@]}"; do
        IFS='|' read -r id _ _ <<<"${job}"
        printf '%s\t%s\n' "${id}" "$(cat "${id}.secs")"
    done | sort -t$'\t' -k2,2rn >"${work_dir}/durations.tsv"
    echo "[tla] slowest checks (wall seconds):"
    head -8 "${work_dir}/durations.tsv"
    if mkdir -p "${receipt_dir}" 2>/dev/null; then
        cp "${work_dir}/durations.tsv" "${durations}.tmp.$$" && mv -f "${durations}.tmp.$$" "${durations}" || true
    fi
)

record_receipt
