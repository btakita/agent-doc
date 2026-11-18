-------------------- MODULE QueueProjectionEditLineage --------------------
EXTENDS Naturals, Sequences, TLC

(***************************************************************************
Lifecycle projection while the operator edits another queue row.

The incident this model excludes had three logical rows. Two sibling rows were
repainted with claim generations while the operator repeatedly extended the
third row. A whole-document text alignment treated the repaint and edit as one
fork and copied both sibling lineages into the edited row. Repeating the
projection copied them again.

The ownership rule is structural:

  OperatorEdit(i) may change only work[i].
  RuntimeProjection(i) may change only decoration[i].

Therefore erasing binary decoration before and after any RuntimeProjection is
an ordered, occurrence-sensitive equality. In particular the work value of one
row can never contain a sibling's work value. `ScopedProjection=TRUE` is the
shipped rule. The wedge permits the former whole-document alignment and must
violate `RowWorkIsolation`, proving the component/row scope is load-bearing.
***************************************************************************)

CONSTANT ScopedProjection

Rows == {"first", "second", "draft"}
DraftRevisions == {"draft-0", "draft-1", "draft-2"}
Decorations == {"bare", "claimed", "running"}

VARIABLES work, decoration, revision
vars == <<work, decoration, revision>>

Init ==
    /\ work = [row \in Rows |->
          CASE row = "first" -> "work-first"
            [] row = "second" -> "work-second"
            [] OTHER -> "draft-0"]
    /\ decoration = [row \in Rows |-> "bare"]
    /\ revision = 0

OperatorEdit ==
    /\ revision < 2
    /\ revision' = revision + 1
    /\ work' = [work EXCEPT !["draft"] =
                    IF revision' = 1 THEN "draft-1" ELSE "draft-2"]
    /\ UNCHANGED decoration

ScopedRuntimeProjection ==
    /\ decoration' = [row \in Rows |->
          IF row \in {"first", "second"}
          THEN (IF decoration[row] = "claimed" THEN "running" ELSE "claimed")
          ELSE decoration[row]]
    /\ UNCHANGED <<work, revision>>

WholeDocumentAlignment ==
    /\ decoration' = [row \in Rows |->
          IF row \in {"first", "second"} THEN "claimed" ELSE decoration[row]]
    /\ work' = [work EXCEPT !["draft"] =
          IF work["draft"] \in DraftRevisions
          THEN work["draft"] \o "+work-first+work-second"
          ELSE work["draft"] \o "+work-first+work-second"]
    /\ UNCHANGED revision

RuntimeProjection ==
    IF ScopedProjection
    THEN ScopedRuntimeProjection
    ELSE ScopedRuntimeProjection \/ WholeDocumentAlignment

Next == OperatorEdit \/ RuntimeProjection
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ work \in [Rows -> STRING]
    /\ decoration \in [Rows -> Decorations]
    /\ revision \in 0..2

RowWorkIsolation ==
    /\ work["first"] = "work-first"
    /\ work["second"] = "work-second"
    /\ work["draft"] \in DraftRevisions

=============================================================================
