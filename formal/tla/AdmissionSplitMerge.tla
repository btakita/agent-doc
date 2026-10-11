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

THE SECOND MEASURED FAILURE: A REVISION TWIN (`typingforkdup`)
--------------------------------------------------------------
Observed 2026-10-11 on a live `sdk.md`. The operator was typing a NEW queue
item. A native save put an intermediate revision of it on disk (279 bytes,
ending mid-sentence); the live editor authority already held a longer revision
of the same line (344 bytes). Neither existed in the baseline, so BOTH sides
"added" an item. Admission ran the ladder:

  admission_three_way_merge outcome=adopted
    rungs=[Cell=equals_authority Semantic=adopted(...)]

and the adopted revision held the line twice: the full one and its truncated
prefix as a second queue item. The count guard admitted it -- it counts disk's
intermediate revision as an independent disk addition, so the bound was
baseline + 1 + 1 and the duplicate fit exactly. The editor-op lineage
projection (`#admissionopancestor`) could not suppress it either: the burst's
op checkpoint appends had failed (`editor_ops_record_failed`), so no op proof
existed. Replayed byte-exact on the reconstructed triple, the Semantic rung
returns exactly the logged 53079 bytes.

THE INVARIANT (both failures)
-----------------------------
Every item the merge emits is the image of exactly ONE authored item
(`NoDuplicateItem`: no identity appears twice). Two texts are revision twins
when they are distinct revisions of one authored item -- at runtime, when one
is a prefix of the other (an in-progress line only ever grows at its end
between two observations of one typing burst). The ASSUMPTION that makes the
textual test sound: an independently authored item is never a prefix or
extension of an item the operator added in the same window; the model encodes
it by giving independent work its own identity (`yd`) and twins the operator's
identity (`ya`).

`TwinCollapse` + `TwinPairGuard` are the shipped fix: `agent_doc_merge::merge` -- the one pure
entry every engine runs behind -- first projects the agent side, dropping each
agent-side-only item that is a revision twin of an item the operator added, so
the operator's text is the twin pair's only image. The engines and the count
guard both consume that projected input (`MergePlan::merged_agent`). A disk
revision NEWER than the operator's is not projected (that direction is unsound
for streamed agent text); the admission ladder instead refuses any candidate
holding more revision-twin pairs than its inputs authored, and proceeds on the
authority. Both rules are properties of the texts alone: no op ledger needed.

  AdmissionSplitMergeTwinWedge.cfg     count guard on, collapse off; MUST
                                       violate -- the 2026-10-11 duplicate.
  AdmissionSplitMergeTwinPairWedge.cfg collapse on, twin-pair guard off; MUST
                                       violate -- a NEWER disk revision is not
                                       projected, so only the pair guard stops
                                       the line rung keeping both.
  AdmissionSplitMergeTwinReach.cfg     asserts disk work never lands beside a
                                       stale twin; MUST violate with the
                                       shipped fix (the projection, not just
                                       the pair guard, carries it through).
  AdmissionSplitMergeTwinOutputWedge.cfg  collapse applied to each rung's
                                       OUTPUT instead of the input; MUST
                                       violate -- the unprojected twin keeps a
                                       spare slot in the count guard.
  AdmissionSplitMerge.cfg              guard + collapse; NoDuplicateItem holds
                                       for both failures.
  AdmissionSplitMergeReach.cfg         still MUST violate with the collapse
                                       on: independent disk work still lands.
***************************************************************************)

CONSTANTS StopOnEqualsAuthority, DuplicateGuard, TwinCollapse, TwinPairGuard

VARIABLES aOp, dOp, aAdd, dAdd, dTwin, phase, result, declined

vars == <<aOp, dOp, aAdd, dAdd, dTwin, phase, result, declined>>

\* One baseline item `x`. Each side keeps it, edits it (two distinct edits so
\* the sides can disagree), or deletes it, and may add one item of its own.
Ops == {"keep", "edit1", "edit2", "delete"}

Version(op) == CASE op = "keep" -> "base"
                 [] op = "edit1" -> "v1"
                 [] op = "edit2" -> "v2"
                 [] op = "delete" -> "none"

BaseLines(op) == IF op = "delete" THEN {} ELSE {<<"x", Version(op)>>}

\* `dTwin`: disk also carries ANOTHER revision of the item the operator added
\* (`ya`) -- "stale" is an earlier cut of the typing burst (the observed case),
\* "newer" a later one (a native save ahead of the authority's replica).
TwinKinds == {"none", "stale", "newer"}
TwinVersion(k) == CASE k = "stale" -> "v0" [] k = "newer" -> "v2" [] OTHER -> "v1"

AuthAdditions == IF aAdd THEN {<<"ya", "v1">>} ELSE {}
DiskTwin == IF dTwin = "none" THEN {} ELSE {<<"ya", TwinVersion(dTwin)>>}

AuthLines == BaseLines(aOp) \cup AuthAdditions
DiskLines == BaseLines(dOp) \cup (IF dAdd THEN {<<"yd", "v1">>} ELSE {}) \cup DiskTwin
Additions == AuthAdditions \cup (IF dAdd THEN {<<"yd", "v1">>} ELSE {})

\* The shipped projection. It sees TEXT: before ANY rung runs, the disk (agent)
\* side drops each item that is not in the operator's revision and is a
\* revision twin (same identity under the stated assumption, different text) of
\* an item the operator ADDED. Every rung -- and the count guard -- then sees the
\* projected disk. Projecting the INPUT, not filtering each rung's output, is
\* load-bearing: filtered output would leave the twin in the guard's bound, and
\* that spare slot admits an unrelated duplicate (TLC found exactly that
\* counterexample: a twin plus two edits of `x`).
\*
\* `TwinCollapse` is "off" (pre-fix), "input" (shipped), or "output" -- the
\* tempting alternative of filtering each rung's result, kept as a wedge.
\* Only an EARLIER revision is projected: a disk text extending the operator's
\* is not removed (a streamed agent response legitimately extends its own
\* operator-visible checkpoint), so the "newer" twin reaches the rungs.
EffTwin == IF TwinCollapse = "input" /\ dTwin = "stale" THEN {} ELSE DiskTwin

OutputFilter(lines) ==
    IF TwinCollapse = "output"
    THEN lines \ {l \in lines : l \notin AuthLines /\ dTwin = "stale"
                                /\ \E m \in AuthAdditions : l[1] = m[1] /\ l # m}
    ELSE lines

\* Identity merge (the `Cell` rung): the operator's change to `x` wins, else
\* disk's; each side's additions survive.
CellMerged ==
    (IF aOp # "keep" THEN BaseLines(aOp) ELSE BaseLines(dOp)) \cup Additions

\* Line merge (the `Semantic` rung): identical to the identity merge except
\* when both sides changed `x` to different surviving text -- then it keeps
\* both lines. That is the observed defect.
\* A disk twin of the operator's addition differs from it as a line, so the line
\* rung keeps it beside the operator's revision -- the 2026-10-11 defect. The
\* identity rung pairs it with `ya` and keeps the operator's revision.
SemanticMerged ==
    (IF /\ aOp \in {"edit1", "edit2"}
        /\ dOp \in {"edit1", "edit2"}
        /\ aOp # dOp
     THEN BaseLines(aOp) \cup BaseLines(dOp) \cup Additions
     ELSE CellMerged)
    \cup EffTwin

\* What the ladder actually adopts from each rung.
CellCandidate == OutputFilter(CellMerged)
SemanticCandidate == OutputFilter(SemanticMerged)

Count(lines, id) == Cardinality({l \in lines : l[1] = id})


\* The runtime guard sees only item counts, never identities. An unprojected
\* twin is growth over the baseline on disk, so it raises the bound -- why the
\* guard alone cannot see a revision twin.
ItemBound == 1 + (IF aAdd THEN 1 ELSE 0) + (IF dAdd THEN 1 ELSE 0)
                + (IF EffTwin # {} THEN 1 ELSE 0)
GuardAdmits(lines) == ~DuplicateGuard \/ Cardinality(lines) <= ItemBound

\* The twin-pair conservation guard (`merge_invents_revision_twins`): a
\* candidate may hold no more revision-twin pairs than the operator authored
\* plus those disk added. Under the stated assumption twin pairs are exactly
\* two lines of the operator-added identity `ya`; neither input holds such a
\* pair, so the bound is zero.
TwinPairs(lines) == Cardinality({p \in SUBSET {l \in lines : l[1] = "ya"} :
                                    Cardinality(p) = 2})
TwinPairsAdmit(lines) == ~TwinPairGuard \/ TwinPairs(lines) = 0

\* A rung's candidate is adoptable when it keeps all authority content,
\* actually changes the authority, and clears the guard.
Adoptable(lines) ==
    /\ AuthLines \subseteq lines
    /\ lines # AuthLines
    /\ GuardAdmits(lines)
    /\ TwinPairsAdmit(lines)

TypeOK ==
    /\ aOp \in Ops /\ dOp \in Ops
    /\ aAdd \in BOOLEAN /\ dAdd \in BOOLEAN
    /\ dTwin \in TwinKinds
    /\ declined \in BOOLEAN
    /\ phase \in {"cell", "semantic", "done"}

\* A genuine two-writer split: both sides advanced and they differ.
Init ==
    /\ aOp \in Ops /\ dOp \in Ops
    /\ aAdd \in BOOLEAN /\ dAdd \in BOOLEAN
    \* A twin is a revision of the operator's addition, so it needs one.
    /\ dTwin \in (IF aAdd THEN TwinKinds ELSE {"none"})
    /\ aOp # "keep" \/ aAdd
    /\ dOp # "keep" \/ dAdd \/ dTwin # "none"
    /\ AuthLines # DiskLines
    /\ phase = "cell"
    /\ result = {}
    /\ declined = FALSE

\* The identity rung may decline (`fell_back`) on any input.
CellDeclines ==
    /\ phase = "cell"
    /\ phase' = "semantic"
    /\ declined' = TRUE
    /\ UNCHANGED <<aOp, dOp, aAdd, dAdd, dTwin, result>>

CellAnswers ==
    /\ phase = "cell"
    /\ IF CellCandidate = AuthLines
       THEN IF StopOnEqualsAuthority
            THEN phase' = "done" /\ result' = AuthLines
            ELSE phase' = "semantic" /\ UNCHANGED result
       ELSE IF Adoptable(CellCandidate)
            THEN phase' = "done" /\ result' = CellCandidate
            ELSE phase' = "semantic" /\ UNCHANGED result
    /\ UNCHANGED <<aOp, dOp, aAdd, dAdd, dTwin, declined>>

\* Last rung: adopt it, or proceed on the authority unchanged.
SemanticRung ==
    /\ phase = "semantic"
    /\ phase' = "done"
    /\ result' = IF Adoptable(SemanticCandidate) THEN SemanticCandidate ELSE AuthLines
    /\ UNCHANGED <<aOp, dOp, aAdd, dAdd, dTwin, declined>>

Done ==
    /\ phase = "done"
    /\ UNCHANGED vars

Next == CellDeclines \/ CellAnswers \/ SemanticRung \/ Done

Spec == Init /\ [][Next]_vars

\* No item identity appears more than once in the adopted revision.
NoDuplicateItem ==
    phase = "done" => \A id \in {"x", "ya", "yd"} : Count(result, id) <= 1

\* Operator-visible text is authoritative: the adopted revision never loses a
\* line of the operator's.
OperatorLinesKept == phase = "done" => AuthLines \subseteq result

\* Reach obligation for the projection: independent disk work must still land
\* when the same disk save ALSO carries a stale revision of the operator's
\* typing (the realistic shape -- a native save holds everything on disk). The
\* twin-pair guard alone would refuse that whole candidate once the identity
\* rung declines; the projection is what lets the independent addition through
\* the line rung. (Safety alone is carried by either rule: with the projection
\* off and the pair guard on, `NoDuplicateItem` still holds -- the projection
\* is load-bearing for PROGRESS, and for callers with no pair guard.)
DiskWorkNeverLandsBesideStaleTwin ==
    (phase = "done" /\ dTwin = "stale" /\ declined) => <<"yd", "v1">> \notin result

\* Reach obligation: disk-only work must still be able to land.
DiskAdditionNeverLands ==
    phase = "done" => <<"yd", "v1">> \notin result \/ <<"yd", "v1">> \in AuthLines

=============================================================================
