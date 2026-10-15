--------------------- MODULE AdmissionCutExclusivity ---------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

(***************************************************************************
Why preflight's admission merge kept duplicating queue items, and the rule
that stops it without guessing which lines are "the same item"
(`#queuecutexclusive`).

THE MEASURED FAILURE (2026-10-15, tasks/agent-doc/agent-doc-bugs.md)
--------------------------------------------------------------------
The operator pasted six queue lines, typed `#gh-dix` on all six with a
multi-caret, then corrected `dix` -> `fix ` on all six and pressed Run. The
native save put the CORRECTED lines on disk; the live editor authority had
only received the `#gh-dix` revision -- the correction's ops were still in
flight (ingested 11 s later). Admission saw a three-way split:

  admission_divergence reason=three_way_split_merged
    baseline_len=4411 authority_len=4753 disk_len=4759
  admission_three_way_merge outcome=adopted
    rungs=[Cell=equals_authority Semantic=adopted(disk_lines_all_verbatim=true
           advisories=0 conflicts=0)]
  crdt_cp_write source=preflight_queue_maintenance content_len=5101

5101 = 4411 + 342 (six `#gh-dix` lines) + 348 (six `#gh-fix ` lines): the
adopted revision held BOTH revisions of all six items, so preflight listed 12
queue heads. At 16:30:22 the in-flight correction ops arrived and rewrote the
editor's own `#gh-dix` items into `#gh-fix` -- next to the six `#gh-fix`
copies the merge had already inserted as NEW CRDT items. The queue then read
six `#gh-fix` lines, `release + publish`, and six `#gh-fix` lines again.

WHY FIVE EARLIER FIXES DID NOT HOLD
----------------------------------
Every incident since the merge replaced the refusal (258a2877a6) has the same
rung sequence, `Cell=equals_authority Semantic=adopted`: 2026-09-30 infra.md,
GH #198 (2026-10-08), `typingforkdup` (2026-10-11), `opcutahead` and this one
(2026-10-15). In every one, disk and the authority were two CUTS of ONE editor
history -- the editor's native save and the editor's own op stream. The
identity rung kept the authority (correct); the line rung unioned the cuts.

Each fix added a classifier for "is this disk line a revision of an authority
line?": a count bound (an addition-by-both looks like two additions), a
textual PREFIX test (a mid-line correction is not a prefix), and an op-ledger
lineage proof (the ledger was mislabelled with the pre-compaction base hash
`97b41a5fa5b6`, so no proof was available). Each classifier was sound for the
shape it was written from and blind to the next shape. The model below makes
that concrete: the old ladder is checked WITH all three guards and TLC still
finds the counterexample (`LadderWedge`).

THE RULE
--------
A cut is a state of the editor's history. Delivery of the editor's own ops is
idempotent per CRDT item; adopting text from disk is NOT -- the merge write
inserts disk's lines as new items, and the editor's in-flight ops then edit or
insert the originals. So a component BOTH planes changed since the baseline
may take exactly one plane's version, and that plane is the authority:

  CutExclusivity: for every component C with A[C] # B[C] and D[C] # B[C],
                  the adopted revision's C equals A[C].

It needs no notion of item identity, no prefix test and no op ledger. The
editor's own ops then roll A[C] forward to the editor's final text exactly once
(`ConvergesConcurrent`). Shipped as an input projection (disk's C reverts to
B[C] before any rung runs) plus a fail-closed guard every candidate must clear
(merged[C] is A[C] or D[C], never a third body built from both).

WHAT THE TEXT RULE DOES NOT CLAIM, AND THE PROVENANCE THAT CLOSES IT
-------------------------------------------------------------------
A component only DISK changed is still merged, so independent disk work lands
(`Reach`). If that disk-only change is itself the editor's in-flight typing,
it can still arrive twice: a text-only policy cannot tell the editor's next
keystrokes on disk from an independent writer's line (`ResidualTextOnly`).

Closing it needs provenance: the editor's own op ledger (`#keylogrebase`).
The ledger records the editor's steps keystroke by keystroke, so it can run
ahead of the authority's batched cut and cover the native save. Policy
"provenance" adds one projection: a component only disk changed whose disk
body is a state of the ledger's replay (and the replay passes through the
authority's cut) is the editor's own text, so disk's body reverts to the
baseline and the editor's ops land it exactly once (`ConvergesAttributed`).

That proof exists only if the ledger is labelled with the text its offsets
are relative to. Live 2026-10-15 it was not: a compaction fenced the epoch
while the merge baseline still named the pre-compaction text, and the
reopened ledger was stamped `merge_base=97b41a5fa5b6` while its offsets were
relative to the compacted buffer. Such a ledger never replays onto the
baseline, so no component is attributed (`LedgerBased = FALSE`,
`ResidualMislabelled`). With a correctly based ledger that covers the disk
cut, no component holding only the editor's text is ever duplicated
(`NoDuplicateWhenCovered`, `Residual`). What remains is stated and checked
(`Boundary`): a ledger that does not yet hold the saved keystrokes, and one
component holding both the editor's in-flight typing and independent work.

  AdmissionCutExclusivity.cfg           shipped rule; NoDuplicateInConcurrent
                                        Component and ConvergesConcurrent hold.
  AdmissionCutExclusivityLadderWedge.cfg  the pre-fix ladder with count guard,
                                        prefix-twin collapse and twin-pair guard
                                        ALL on; MUST violate. TLC's shortest
                                        trace is the GH #198 / `opcutahead`
                                        shape: a STALE disk cut of an item the
                                        operator then edited mid-line.
  AdmissionCutExclusivityInflightWedge.cfg  the same pre-fix ladder, restricted
                                        to a disk cut AHEAD of the authority
                                        (NoDuplicateWhenDiskAhead); MUST
                                        violate, and TLC's trace is the incident:
                                        a added then edited mid-line; the
                                        authority cut holds a@1, disk a@2; the
                                        merge copies a@2 as a NEW item, then the
                                        in-flight edit bumps the editor's a@1 to
                                        a@2 -- two a@2.
  AdmissionCutExclusivityReach.cfg      MUST violate ExternalNeverLands:
                                        independent disk work in a component the
                                        authority left alone still lands, with
                                        provenance on.
  AdmissionCutExclusivityResidual.cfg   provenance with a correctly based ledger:
                                        NoDuplicateWhenCovered and
                                        ConvergesAttributed hold.
  AdmissionCutExclusivityResidualTextOnly.cfg  the text-only rule ("exclusive");
                                        MUST violate NoDuplicateWhenCovered.
  AdmissionCutExclusivityResidualMislabelled.cfg  provenance over a ledger
                                        stamped with the lagging merge baseline
                                        (`#keylogrebase`); MUST violate
                                        NoDuplicateWhenCovered.
  AdmissionCutExclusivityBoundary.cfg   provenance on; MUST violate the
                                        all-cases NoDuplicateItem, so the
                                        remaining boundary is exact.
***************************************************************************)

CONSTANTS Policy, N, LedgerBased

ASSUME Policy \in {"ladder", "exclusive", "provenance"}
ASSUME N \in 2..4
ASSUME LedgerBased \in BOOLEAN

\* Editor steps. `appA` extends item a at its end (a typing burst: the textual
\* twin test can see it); `midA` edits it mid-line (a correction: it cannot).
Steps == {"addA", "appA", "midA", "addR", "noop"}
Comps == {"Q", "R"}
CompOf(it) == IF it = "a" THEN "Q" ELSE "R"

VARIABLES steps, baseA, tA, tD, tL, ext, auth, next, phase

vars == <<steps, baseA, tA, tD, tL, ext, auth, next, phase>>

Apply(s, st) ==
    CASE st = "addA" /\ s.a = 0 -> [s EXCEPT !.a = 1]
      [] st \in {"appA", "midA"} /\ s.a > 0 -> [s EXCEPT !.a = s.a + 1]
      [] st = "addR" /\ s.r = 0 -> [s EXCEPT !.r = 1]
      [] OTHER -> s

\* The editor's history: HistAt(1) is the baseline, HistAt(k+1) follows step k.
RECURSIVE HistAt(_)
HistAt(k) == IF k = 1 THEN [a |-> baseA, r |-> 0]
             ELSE Apply(HistAt(k - 1), steps[k - 1])

Effective(k) == HistAt(k + 1) # HistAt(k)

Lines(s) == (IF s.a > 0 THEN {<<"Q", "a", s.a>>} ELSE {})
            \cup (IF s.r > 0 THEN {<<"R", "r", s.r>>} ELSE {})

External == <<"R", "x", 1>>

LB == Lines(HistAt(1))
LA == Lines(HistAt(tA))
LD == Lines(HistAt(tD)) \cup (IF ext THEN {External} ELSE {})

Comp(L, c) == {l \in L : l[1] = c}
Changed(L, c) == Comp(L, c) # Comp(LB, c)
Concurrent(c) == Changed(LA, c) /\ Changed(LD, c)

\* The prefix test the shipped twin guards use: two revisions of `a` are seen
\* as twins only when every edit between them extended the line at its end.
TwinSees(v1, v2) ==
    LET lo == IF v1 < v2 THEN v1 ELSE v2
        hi == IF v1 < v2 THEN v2 ELSE v1
    IN \A k \in 1..(N - 1) :
          (Effective(k) /\ steps[k] \in {"appA", "midA"} /\ HistAt(k).a \in lo..(hi - 1))
              => steps[k] = "appA"

\* `#keylogrebase` provenance. The op ledger holds the editor's steps
\* 1..tL-1, so its replay states are HistAt(1..tL). It is usable only when it
\* is labelled with the text its offsets are relative to (LedgerBased) and its
\* replay passes through the authority's cut.
LineageProven == LedgerBased /\ \E k \in 1..tL : Lines(HistAt(k)) = LA

\* A component only disk changed whose disk body is a ledger replay state is
\* the editor's own text.
Attributed(c) ==
    /\ Policy = "provenance"
    /\ LineageProven
    /\ ~Changed(LA, c)
    /\ Changed(LD, c)
    /\ \E k \in 1..tL : Comp(Lines(HistAt(k)), c) = Comp(LD, c)

\* `#queuecutexclusive` input projection: disk's version of a component both
\* planes changed reverts to the baseline, so no rung can see two versions.
\* With provenance, so does disk's version of an attributed component.
ProjectedD ==
    IF Policy \in {"exclusive", "provenance"}
    THEN UNION {IF Concurrent(c) \/ Attributed(c) THEN Comp(LB, c) ELSE Comp(LD, c)
                : c \in Comps}
    ELSE LD

\* `typingforkdup` input projection: drop a disk line that is an EARLIER twin of
\* a line the operator added.
Din == ProjectedD \ {l \in ProjectedD :
                        /\ l \notin LA
                        /\ \E m \in LA \ LB : /\ m[2] = l[2]
                                              /\ l[3] < m[3]
                                              /\ TwinSees(l[3], m[3])}

\* Line rung: a base line survives only if both sides kept it; every other
\* line either side holds survives.
UnionComp(D, c) == {l \in Comp(LA, c) \cup Comp(D, c) : l \notin LB \/ (l \in LA /\ l \in D)}
UnionMerge(D) == UNION {UnionComp(D, c) : c \in Comps}

\* Identity rung: where both sides changed a component it keeps the operator's
\* (authority's) version -- the `Cell=equals_authority` answer of every incident.
CellMerge(D) ==
    UNION {IF Changed(LA, c) /\ Comp(D, c) # Comp(LB, c)
           THEN Comp(LA, c) ELSE UnionComp(D, c) : c \in Comps}

Adds(L, c) == IF Cardinality(Comp(L, c)) > Cardinality(Comp(LB, c))
              THEN Cardinality(Comp(L, c)) - Cardinality(Comp(LB, c)) ELSE 0

\* `#admissionmergedup` count-conservation guard.
CountOK(cand) == \A c \in Comps :
    Cardinality(Comp(cand, c)) <= Cardinality(Comp(LB, c)) + Adds(LA, c) + Adds(Din, c)

\* `typingforkdup` twin-pair guard: no candidate may hold two revisions of one
\* item the prefix test recognizes (neither input holds such a pair).
PairOK(cand) == ~\E l1, l2 \in cand : /\ l1[2] = "a" /\ l2[2] = "a"
                                      /\ l1[3] < l2[3]
                                      /\ TwinSees(l1[3], l2[3])

\* `#queuecutexclusive` fail-closed guard: a concurrently changed component is
\* exactly one plane's version, never a body built from both.
ExclusiveOK(cand) ==
    /\ \/ Policy = "ladder"
       \/ \A c \in Comps : Concurrent(c) => Comp(cand, c) \in {Comp(LA, c), Comp(LD, c)}
    \* `#keylogrebase` fail-closed guard: an attributed component is the
    \* authority's body; it reaches the authority only through the editor's ops.
    /\ \A c \in Comps : Attributed(c) => Comp(cand, c) = Comp(LA, c)

Adoptable(cand) ==
    /\ LA \subseteq cand
    /\ cand # LA
    /\ CountOK(cand)
    /\ PairOK(cand)
    /\ ExclusiveOK(cand)

Adopted ==
    LET cell == CellMerge(Din)
        sem == UnionMerge(Din)
    IN IF cell # LA /\ Adoptable(cell) THEN cell
       ELSE IF Adoptable(sem) THEN sem
       ELSE LA

Elem(l, src) == [c |-> l[1], item |-> l[2], ver |-> l[3], src |-> src]

TypeOK ==
    /\ steps \in [1..(N - 1) -> Steps]
    /\ baseA \in {0, 1}
    /\ tA \in 1..N /\ tD \in 1..N /\ tL \in 1..N
    /\ ext \in BOOLEAN
    /\ phase \in {"merge", "deliver", "done"}
    /\ next \in 1..N

\* A genuine split: the classifier only reaches the merge when both branches
\* durably advanced and the authority does not already carry disk.
Init ==
    /\ steps \in [1..(N - 1) -> Steps]
    /\ baseA \in {0, 1}
    /\ tA \in 1..N
    /\ tD \in 1..N
    /\ tL \in 1..N
    /\ ext \in BOOLEAN
    /\ LA # LB
    /\ LD # LB
    /\ LA # LD
    /\ ~(LD \subseteq LA)
    /\ auth = {Elem(l, "ed") : l \in LA}
    /\ next = tA
    /\ phase = "merge"

\* Admission writes the adopted revision. Lines it adds become NEW CRDT items
\* (`srv`): text copied from disk carries no editor item identity.
MergeWrite ==
    /\ phase = "merge"
    /\ auth' = auth \cup {Elem(l, "srv") : l \in Adopted \ LA}
    /\ phase' = "deliver"
    /\ UNCHANGED <<steps, baseA, tA, tD, tL, ext, next>>

\* The editor's ops past the authority's cut arrive, in order, and act on the
\* editor's OWN items -- never on the copies admission inserted.
Deliver ==
    /\ phase = "deliver"
    /\ next < N
    /\ LET st == steps[next]
           mine == {e \in auth : e.item = "a" /\ e.src = "ed"}
       IN auth' =
            IF ~Effective(next) THEN auth
            ELSE CASE st = "addA" -> auth \cup {Elem(<<"Q", "a", 1>>, "ed")}
                   [] st = "addR" -> auth \cup {Elem(<<"R", "r", 1>>, "ed")}
                   [] OTHER -> (auth \ mine) \cup {[e EXCEPT !.ver = e.ver + 1] : e \in mine}
    /\ next' = next + 1
    /\ UNCHANGED <<steps, baseA, tA, tD, tL, ext, phase>>

Finish ==
    /\ phase = "deliver"
    /\ next = N
    /\ phase' = "done"
    /\ UNCHANGED <<steps, baseA, tA, tD, tL, ext, auth, next>>

Done == phase = "done" /\ UNCHANGED vars

Next == MergeWrite \/ Deliver \/ Finish \/ Done

Spec == Init /\ [][Next]_vars

Live(it) == {e \in auth : e.item = it}
Text(c) == {<<e.c, e.item, e.ver>> : e \in {f \in auth : f.c = c}}

\* Every item is held once, in every component.
NoDuplicateItem == phase = "done" => \A it \in {"a", "r", "x"} : Cardinality(Live(it)) <= 1

\* The shipped claim: a component both planes changed never holds an item twice.
NoDuplicateInConcurrentComponent ==
    phase = "done" =>
        \A it \in {"a", "r", "x"} : Concurrent(CompOf(it)) => Cardinality(Live(it)) <= 1

\* The same claim restricted to a disk cut AHEAD of the authority's cut -- the
\* 2026-10-15 shape, where the duplicate is completed by the in-flight ops.
NoDuplicateWhenDiskAhead == tD > tA => NoDuplicateInConcurrentComponent

\* Exactly-once: once the editor's ops have arrived, a concurrently changed
\* component is exactly the editor's final text.
ConvergesConcurrent ==
    phase = "done" => \A c \in Comps : Concurrent(c) => Text(c) = Comp(Lines(HistAt(N)), c)

\* Exactly-once for provenance: an attributed component is exactly the
\* editor's final text once its ops have arrived.
ConvergesAttributed ==
    phase = "done" => \A c \in Comps : Attributed(c) => Text(c) = Comp(Lines(HistAt(N)), c)

\* Disk's body of `c` is purely the editor's native save (no independent work).
EditorOnly(c) == Comp(LD, c) = Comp(Lines(HistAt(tD)), c)

\* The ledger holds every editor step through both cuts.
Covered == tL >= tD /\ tL >= tA

\* `#keylogrebase`: with a ledger covering both cuts, no component holding only
\* the editor's own text ever holds an item twice.
NoDuplicateWhenCovered ==
    (phase = "done" /\ Covered) =>
        \A it \in {"a", "r", "x"} : EditorOnly(CompOf(it)) => Cardinality(Live(it)) <= 1

\* Reach obligation: independent disk work can still land.
ExternalNeverLands == phase = "done" => Live("x") = {}

=============================================================================
