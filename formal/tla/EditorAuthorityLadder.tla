------------------------ MODULE EditorAuthorityLadder ------------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************
Which state is canonical for a session document, and whether the in-memory
CRDT can ever roll an editor back.

THE MEASURED FAILURE
--------------------
src/haiven-dev/tasks/api.md, 2026-09-29 15:35. The operator typed ~468
characters. The editor forwarded them and the controller acknowledged the
visible state (projectState content_hash=90a06be...), but quarantined the
update pending a lazy canonical projection. An install then handed the
controller off. The controller's canonical text lacked the typing, and when the
editor re-registered after the plugin reload it adopted that canonical:
"registration retained the controller canonical projection". The operator's
text was gone. The editor was the source of truth and it was rolled back.

THE DESIGN (the operator's authority ladder)
--------------------------------------------
  1. Two or more editors open: canonical = the reconciliation of the editors'
     states.
  2. One editor open: canonical = that editor's buffer.
  3. No editor open: canonical = disk.

The in-memory CRDT is a working copy and merge engine for the current rung,
never an authority over an open editor:

  * Editors only ever MERGE FORWARD: an editor receives operations (the other
    editors', the agent's) and never a whole-text replacement.
  * Registration publishes the editor's state into the CRDT and then merges
    forward; it never adopts the CRDT over the editor.
  * A controller handoff or restart reseeds the CRDT from the current rung
    (the open editors' states, else disk), never from an older checkpoint.

`EditorAuthority` gates the design:

  TRUE  -- `NoRollback` holds and editors converge
           (EditorAuthorityLadder.cfg).
  FALSE -- the shipped behaviour: registration adopts the CRDT, and a handoff
           reseeds it from disk. `NoRollback` MUST be violated
           (EditorAuthorityLadderWedge.cfg): the api.md rollback.

`EditorAuthorityLadderReach.cfg` asserts an agent write never reaches an
editor and must be violated, so forward merging still delivers agent work.

MODEL
-----
Document content is a set of atoms. Operator atoms are typed in an editor and
deleted only by the operator; agent atoms are written and deleted by the agent
through the CRDT. `deleted` is the set of atoms some actor intentionally
deleted, i.e. the tombstones. An editor "rolls back" when it loses an atom
nobody deleted.
***************************************************************************)

CONSTANTS Editors, OperatorAtoms, AgentAtoms, EditorAuthority

Atoms == OperatorAtoms \cup AgentAtoms

VARIABLES open, buf, crdt, disk, deleted, lost

vars == <<open, buf, crdt, disk, deleted, lost>>

TypeOK ==
    /\ open \subseteq Editors
    /\ buf \in [Editors -> SUBSET Atoms]
    /\ crdt \subseteq Atoms
    /\ disk \subseteq Atoms
    /\ deleted \subseteq Atoms
    /\ lost \in BOOLEAN

Init ==
    /\ open = {}
    /\ buf = [e \in Editors |-> {}]
    /\ crdt = {}
    /\ disk = {}
    /\ deleted = {}
    /\ lost = FALSE

\* The canonical state for the current rung of the ladder.
Canonical ==
    IF open # {} THEN (UNION {buf[e] : e \in open}) \ deleted ELSE disk

\* Replacing an editor's buffer with `next` rolls it back when an atom nobody
\* deleted disappears.
RollsBack(e, next) == \E a \in buf[e] : a \notin next /\ a \notin deleted

OpenEditor(e) ==
    /\ e \notin open
    /\ open' = open \cup {e}
    \* An opening editor reads the current canonical state.
    /\ buf' = [buf EXCEPT ![e] = IF open = {} THEN disk ELSE Canonical]
    /\ UNCHANGED <<crdt, disk, deleted, lost>>

CloseEditor(e) ==
    /\ e \in open
    \* Closing saves the editor's buffer; the last editor hands authority to disk.
    /\ disk' = IF open = {e} THEN buf[e] ELSE disk
    /\ open' = open \ {e}
    /\ UNCHANGED <<buf, crdt, deleted, lost>>

OperatorTypes(e, a) ==
    /\ e \in open
    /\ a \in OperatorAtoms
    /\ a \notin buf[e]
    /\ a \notin deleted
    /\ buf' = [buf EXCEPT ![e] = @ \cup {a}]
    /\ UNCHANGED <<open, crdt, disk, deleted, lost>>

OperatorDeletes(e, a) ==
    /\ e \in open
    /\ a \in OperatorAtoms \cap buf[e]
    /\ buf' = [buf EXCEPT ![e] = @ \ {a}]
    /\ deleted' = deleted \cup {a}
    /\ UNCHANGED <<open, crdt, disk, lost>>

\* The editor forwards its operations into the CRDT.
Forward(e) ==
    /\ e \in open
    /\ crdt' = (crdt \cup buf[e]) \ deleted
    /\ UNCHANGED <<open, buf, disk, deleted, lost>>

AgentWrites(a) ==
    /\ a \in AgentAtoms
    /\ a \notin crdt
    /\ a \notin deleted
    /\ IF open = {}
          THEN /\ disk' = disk \cup {a}
               /\ crdt' = crdt \cup {a}
          ELSE /\ crdt' = crdt \cup {a}
               /\ UNCHANGED disk
    /\ UNCHANGED <<open, buf, deleted, lost>>

AgentDeletes(a) ==
    /\ a \in AgentAtoms \cap crdt
    /\ crdt' = crdt \ {a}
    /\ deleted' = deleted \cup {a}
    /\ disk' = IF open = {} THEN disk \ {a} ELSE disk
    /\ UNCHANGED <<open, buf, lost>>

\* The CRDT delivers operations forward to an editor. Both designs do this.
Deliver(e) ==
    /\ e \in open
    /\ buf' = [buf EXCEPT ![e] = (@ \cup crdt) \ deleted]
    /\ UNCHANGED <<open, crdt, disk, deleted, lost>>

\* A plugin reload re-registers the editor with the CRDT: publish the editor
\* state, then merge forward.
RegisterForward(e) ==
    /\ EditorAuthority
    /\ e \in open
    /\ crdt' = (crdt \cup buf[e]) \ deleted
    /\ buf' = [buf EXCEPT ![e] = (@ \cup crdt) \ deleted]
    /\ UNCHANGED <<open, disk, deleted, lost>>

\* Shipped: registration adopts the CRDT over the editor.
RegisterAdopt(e) ==
    /\ ~EditorAuthority
    /\ e \in open
    /\ buf' = [buf EXCEPT ![e] = crdt]
    /\ lost' = (lost \/ RollsBack(e, crdt))
    /\ UNCHANGED <<open, crdt, disk, deleted>>

Register(e) == RegisterForward(e) \/ RegisterAdopt(e)

\* A controller handoff/restart rebuilds the in-memory CRDT.
Handoff ==
    /\ crdt' = IF EditorAuthority THEN Canonical ELSE disk
    /\ UNCHANGED <<open, buf, disk, deleted, lost>>

Next ==
    \/ \E e \in Editors :
        \/ OpenEditor(e) \/ CloseEditor(e) \/ Forward(e) \/ Deliver(e) \/ Register(e)
        \/ \E a \in Atoms : OperatorTypes(e, a) \/ OperatorDeletes(e, a)
    \/ \E a \in Atoms : AgentWrites(a) \/ AgentDeletes(a)
    \/ Handoff

Spec == Init /\ [][Next]_vars

\* No editor ever loses text that nobody deleted.
NoRollback == ~lost

\* Non-vacuity: agent work reaches an open editor by forward merge.
AgentNeverReachesAnEditor == \A e \in open : buf[e] \cap AgentAtoms = {}

=============================================================================
