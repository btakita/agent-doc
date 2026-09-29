----------------------- MODULE RealtimeSteeringStop -----------------------
EXTENDS Naturals, TLC

(***************************************************************************
Whether operator steering that lands after a commit is always answered.

THE MEASURED FAILURE
--------------------
Observed 2026-09-29 on `src/haiven-dev/tasks/api.md`. The operator typed into
the queue while the turn ran. The response committed, and session-check then
reported the typing as steering:

  cycle `cycle-1790707653839` is `committed` ... unresolved prompt-bearing user
  changes with no new agent-doc cycle started: content_edit: Even if formal
  logic code is not directly understandable, ...

The Codex Stop hook recognized steering only when it was labelled
`prompt_target`. A `content_edit` (typing inside an existing queue item or
prompt) fell through to "already continued once ... the cycle is still open",
and the agent stopped with the operator's edit unanswered.

THE RULE
--------
Steering is steering whatever its label. After a commit, the Stop hook hands
every steering kind back to the agent to answer in the current turn.
`ContentEditIsSteering` gates the fix:

  TRUE  -- `SteeringEventuallyAnswered` holds (RealtimeSteeringStop.cfg).
  FALSE -- only `prompt_target` is recognized; `SteeringEventuallyAnswered`
           MUST be violated (RealtimeSteeringStopWedge.cfg).

`RealtimeSteeringStopReach.cfg` asserts a `content_edit` is never handed back
and must be violated, so the positive run exercises the new edge.
***************************************************************************)

CONSTANT ContentEditIsSteering

VARIABLES steering, turn, handedBack

vars == <<steering, turn, handedBack>>

Kinds == {"none", "prompt_target", "content_edit"}

TypeOK ==
    /\ steering \in Kinds
    /\ turn \in {"running", "committed", "answering", "stopped"}
    /\ handedBack \in {"none", "prompt_target", "content_edit"}

Init ==
    /\ steering = "none"
    /\ turn = "running"
    /\ handedBack = "none"

\* The operator types while the turn runs: a new prompt or an edit inside an
\* existing queue item.
OperatorTypes ==
    /\ turn = "running"
    /\ steering = "none"
    /\ \E kind \in {"prompt_target", "content_edit"} : steering' = kind
    /\ UNCHANGED <<turn, handedBack>>

Commit ==
    /\ turn = "running"
    /\ turn' = "committed"
    /\ UNCHANGED <<steering, handedBack>>

Recognized(kind) == kind = "prompt_target" \/ (ContentEditIsSteering /\ kind = "content_edit")

\* The (recursive) Stop hook after the commit.
StopHook ==
    /\ turn = "committed"
    /\ IF steering # "none" /\ Recognized(steering)
          THEN /\ turn' = "answering"
               /\ handedBack' = steering
          ELSE /\ turn' = "stopped"
               /\ UNCHANGED handedBack
    /\ UNCHANGED steering

\* The agent answers the handed-back steering in the same turn.
Answer ==
    /\ turn = "answering"
    /\ steering' = "none"
    /\ turn' = "stopped"
    /\ UNCHANGED handedBack

Next == OperatorTypes \/ Commit \/ StopHook \/ Answer

Spec == Init /\ [][Next]_vars /\ WF_vars(Commit) /\ WF_vars(StopHook) /\ WF_vars(Answer)

\* Once the turn stops, no operator steering is left unanswered.
SteeringEventuallyAnswered == <>[](turn = "stopped" => steering = "none")

NeverHandsBackContentEdit == handedBack # "content_edit"

=============================================================================
