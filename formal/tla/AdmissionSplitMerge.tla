----------------------- MODULE AdmissionSplitMerge -----------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************
Whether preflight's admission three-way merge can duplicate a queue item.

THE MEASURED FAILURE
--------------------
Observed 2026-09-30 on `src/haiven-dev/tasks/infra.md` (`#admissionmergedup`).
The operator reworded one queue item. Disk carried the reword once; the editor
authority carried it twice (a controller recycle re-applied the editor's
unacknowledged typing). Both sides therefore EDITED the same baseline item to
different text. Admission found a two-writer split and ran its merge ladder:

  admission_three_way_merge outcome=adopted
    rungs=[Cell=equals_authority Semantic=adopted(...)]  merged_len=39081

`Cell` merges by node identity and answered that the authority already carried
disk's change. The ladder treated that answer like a decline and fell through
to `Semantic`, which merges by LINE: it saw two different edits of one line
and kept both. The adopted revision held the item twice. The only check on the
candidate, "the merge never drops authority content", cannot see an addition.
Replayed byte-exact against the real baseline (commit 5e6da24c): the Semantic
rung returns exactly 39081 bytes with the item on two lines.

THE RULE
--------
A merge may never hold a queue item more times than a correct identity merge
would. At runtime that is enforced by a count-conservation guard every rung must
clear: the merged queue holds at most baseline + authority additions + disk
additions items (`DuplicateGuard`). That guard is the shipped fix.

`StopOnEqualsAuthority` models the tempting alternative -- end the ladder when
the identity rung answers `equals_authority`. It is NOT sufficient (the identity
rung may also decline, which still hands the split to the line rung), and it is
not shipped: that answer can also mean the identity rung missed disk-only work
the line rung would recover.

  AdmissionSplitMerge.cfg              the shipped fix (guard only);
                                       NoDuplicateItem holds.
  AdmissionSplitMergeWedge.cfg         shipped ladder; MUST violate.
  AdmissionSplitMergeGuardOffWedge.cfg stop-on-equals instead of the guard;
                                       MUST violate (the decline path) -- the
                                       guard is the load-bearing edge.
  AdmissionSplitMergeReach.cfg         asserts a disk-only addition never
                                       lands; MUST violate, so the guarded
                                       ladder still merges real disk work.
***************************************************************************)

CONSTANTS StopOnEqualsAuthority, DuplicateGuard

VARIABLES aOp, dOp, aAdd, dAdd, phase, result

vars == <<aOp, dOp, aAdd, dAdd, phase, result>>

\* One baseline item `x`. Each side keeps it, edits it (two distinct edits so
\* the sides can disagree), or deletes it, and may add one item of its own.
Ops == {"keep", "edit1", "edit2", "delete"}

Version(op) == CASE op = "keep" -> "base"
                 [] op = "edit1" -> "v1"
                 [] op = "edit2" -> "v2"
                 [] op = "delete" -> "none"

BaseLines(op) == IF op = "delete" THEN {} ELSE {<<"x", Version(op)>>}

AuthLines == BaseLines(aOp) \cup (IF aAdd THEN {<<"ya", "v1">>} ELSE {})
DiskLines == BaseLines(dOp) \cup (IF dAdd THEN {<<"yd", "v1">>} ELSE {})
Additions == (IF aAdd THEN {<<"ya", "v1">>} ELSE {})
             \cup (IF dAdd THEN {<<"yd", "v1">>} ELSE {})

\* Identity merge (the `Cell` rung): the operator's change to `x` wins, else
\* disk's; each side's additions survive.
CellMerged ==
    (IF aOp # "keep" THEN BaseLines(aOp) ELSE BaseLines(dOp)) \cup Additions

\* Line merge (the `Semantic` rung): identical to the identity merge except
\* when both sides changed `x` to different surviving text -- then it keeps
\* both lines. That is the observed defect.
SemanticMerged ==
    IF /\ aOp \in {"edit1", "edit2"}
       /\ dOp \in {"edit1", "edit2"}
       /\ aOp # dOp
    THEN BaseLines(aOp) \cup BaseLines(dOp) \cup Additions
    ELSE CellMerged

Count(lines, id) == Cardinality({l \in lines : l[1] = id})

\* The runtime guard sees only item counts, never identities.
ItemBound == 1 + (IF aAdd THEN 1 ELSE 0) + (IF dAdd THEN 1 ELSE 0)
GuardAdmits(lines) == ~DuplicateGuard \/ Cardinality(lines) <= ItemBound

\* A rung's candidate is adoptable when it keeps all authority content,
\* actually changes the authority, and clears the guard.
Adoptable(lines) ==
    /\ AuthLines \subseteq lines
    /\ lines # AuthLines
    /\ GuardAdmits(lines)

TypeOK ==
    /\ aOp \in Ops /\ dOp \in Ops
    /\ aAdd \in BOOLEAN /\ dAdd \in BOOLEAN
    /\ phase \in {"cell", "semantic", "done"}

\* A genuine two-writer split: both sides advanced and they differ.
Init ==
    /\ aOp \in Ops /\ dOp \in Ops
    /\ aAdd \in BOOLEAN /\ dAdd \in BOOLEAN
    /\ aOp # "keep" \/ aAdd
    /\ dOp # "keep" \/ dAdd
    /\ AuthLines # DiskLines
    /\ phase = "cell"
    /\ result = {}

\* The identity rung may decline (`fell_back`) on any input.
CellDeclines ==
    /\ phase = "cell"
    /\ phase' = "semantic"
    /\ UNCHANGED <<aOp, dOp, aAdd, dAdd, result>>

CellAnswers ==
    /\ phase = "cell"
    /\ IF CellMerged = AuthLines
       THEN IF StopOnEqualsAuthority
            THEN phase' = "done" /\ result' = AuthLines
            ELSE phase' = "semantic" /\ UNCHANGED result
       ELSE IF Adoptable(CellMerged)
            THEN phase' = "done" /\ result' = CellMerged
            ELSE phase' = "semantic" /\ UNCHANGED result
    /\ UNCHANGED <<aOp, dOp, aAdd, dAdd>>

\* Last rung: adopt it, or proceed on the authority unchanged.
SemanticRung ==
    /\ phase = "semantic"
    /\ phase' = "done"
    /\ result' = IF Adoptable(SemanticMerged) THEN SemanticMerged ELSE AuthLines
    /\ UNCHANGED <<aOp, dOp, aAdd, dAdd>>

Done ==
    /\ phase = "done"
    /\ UNCHANGED vars

Next == CellDeclines \/ CellAnswers \/ SemanticRung \/ Done

Spec == Init /\ [][Next]_vars

\* No item identity appears more than once in the adopted revision.
NoDuplicateItem ==
    phase = "done" => \A id \in {"x", "ya", "yd"} : Count(result, id) <= 1

\* Reach obligation: disk-only work must still be able to land.
DiskAdditionNeverLands ==
    phase = "done" => <<"yd", "v1">> \notin result \/ <<"yd", "v1">> \in AuthLines

=============================================================================
