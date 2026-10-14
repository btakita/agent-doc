------------------ MODULE SupervisorGenerationTransition ------------------
EXTENDS Naturals, TLC

(***************************************************************************
This finite model checks the supervisor-generation topology gate. An open
document cycle may cross generations only through captured-response replay or
exact live-child transport continuity. IPC drain is independent and always
required. Cycle close, continuity proof, and IPC drain are environment edges,
not timeouts.
***************************************************************************)

VARIABLES cycleOpen,
          replayCheckpoint,
          liveChildContinuity,
          ipcDrained,
          cause,
          phase,
          replacements,
          unsafeReplacements,
          uncheckpointedOpenReplacements

vars == <<cycleOpen, replayCheckpoint, liveChildContinuity, ipcDrained, cause, phase,
          replacements, unsafeReplacements,
          uncheckpointedOpenReplacements>>

CapturedRecovery ==
    /\ cause = "captured_recovery"
    /\ replayCheckpoint

GenerationContinuity == CapturedRecovery \/ liveChildContinuity

Init ==
    /\ cycleOpen \in BOOLEAN
    /\ replayCheckpoint \in BOOLEAN
    /\ liveChildContinuity \in BOOLEAN
    /\ ipcDrained \in BOOLEAN
    /\ cause \in {"install", "stale", "restart", "captured_recovery"}
    /\ phase = "requested"
    /\ replacements = 0
    /\ unsafeReplacements = 0
    /\ uncheckpointedOpenReplacements = 0

CloseCycle ==
    /\ phase = "requested"
    /\ cycleOpen
    /\ cycleOpen' = FALSE
    /\ UNCHANGED <<replayCheckpoint, liveChildContinuity, ipcDrained, cause, phase,
                    replacements, unsafeReplacements,
                    uncheckpointedOpenReplacements>>

DrainIpc ==
    /\ phase = "requested"
    /\ ~ipcDrained
    /\ ipcDrained' = TRUE
    /\ UNCHANGED <<cycleOpen, replayCheckpoint, liveChildContinuity, cause, phase,
                    replacements, unsafeReplacements,
                    uncheckpointedOpenReplacements>>

PublishCapturedReplay ==
    /\ phase = "requested"
    /\ cause = "captured_recovery"
    /\ ~replayCheckpoint
    /\ replayCheckpoint' = TRUE
    /\ UNCHANGED <<cycleOpen, liveChildContinuity, ipcDrained, cause, phase, replacements,
                    unsafeReplacements, uncheckpointedOpenReplacements>>

ProveLiveChildContinuity ==
    /\ phase = "requested"
    /\ ~liveChildContinuity
    /\ liveChildContinuity' = TRUE
    /\ UNCHANGED <<cycleOpen, replayCheckpoint, ipcDrained, cause, phase,
                    replacements, unsafeReplacements,
                    uncheckpointedOpenReplacements>>

Replace ==
    /\ phase = "requested"
    /\ ipcDrained
    /\ (~cycleOpen \/ GenerationContinuity)
    /\ phase' = "replaced"
    /\ replacements' = replacements + 1
    /\ unsafeReplacements' = unsafeReplacements
        + IF ipcDrained THEN 0 ELSE 1
    /\ uncheckpointedOpenReplacements' = uncheckpointedOpenReplacements
        + IF cycleOpen /\ ~GenerationContinuity THEN 1 ELSE 0
    /\ UNCHANGED <<cycleOpen, replayCheckpoint, liveChildContinuity, ipcDrained, cause>>

Done ==
    /\ phase = "replaced"
    /\ UNCHANGED vars

Next == CloseCycle \/ DrainIpc \/ PublishCapturedReplay
        \/ ProveLiveChildContinuity \/ Replace \/ Done

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(Replace)

TypeOK ==
    /\ cycleOpen \in BOOLEAN
    /\ replayCheckpoint \in BOOLEAN
    /\ liveChildContinuity \in BOOLEAN
    /\ ipcDrained \in BOOLEAN
    /\ cause \in {"install", "stale", "restart", "captured_recovery"}
    /\ phase \in {"requested", "replaced"}
    /\ replacements \in Nat
    /\ unsafeReplacements \in Nat
    /\ uncheckpointedOpenReplacements \in Nat

OpenUncheckpointedNeverReplaced == uncheckpointedOpenReplacements = 0

UnsafeCheckpointNeverReplaced == unsafeReplacements = 0

AtMostOneReplacement == replacements <= 1

OnlyProvenContinuityCrossesOpenCycle ==
    (phase = "replaced" /\ cycleOpen) => GenerationContinuity

TerminalRequestEventuallyReplaced ==
    [](phase = "requested" /\ ~cycleOpen /\ ipcDrained
       => <> (phase = "replaced"))

=============================================================================
