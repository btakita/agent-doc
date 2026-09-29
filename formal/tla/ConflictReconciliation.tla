----------------------- MODULE ConflictReconciliation -----------------------
EXTENDS Integers, Sequences, TLC

(***************************************************************************
Conflict reconciliation between the operator's live edit and the agent's
incoming merge (`#editorauth1`, plan:
tasks/agent-doc/plan-editor-authority-ladder.md).

EditorAuthorityLadder.tla models content as a set, which proves nothing is
lost but cannot say WHERE text lands. Here content is a sequence and the
operator has a cursor, so the operator's ordering rules are checkable:

  1. Both append at the same point: the agent's content goes BEFORE the
     operator's in-progress append, and the cursor stays at the end of the
     operator's edit, so continued typing extends the operator's text.
  2. The operator edits existing text while the agent edits elsewhere: both
     apply; the regions are independent.
  3. Both edit the same existing text: a true conflict, surfaced in the buffer
     and never dropped on either side. Resolving it to either side reproduces
     that side's edit exactly. The rendering is compact: text both sides share
     stays outside the conflict (operator decision 2026-09-29; implementation
     agent-doc-merge/src/conflict_render.rs, whose inline CriticMarkup and
     block markers are two notations for one conflict segment).

An edit replaces [pos, pos + len) of the shared base with `ins`; an append is
an edit with len = 0. The initial state is every pair of edits over Base; the
operator then keeps typing at the cursor.

formal/authority_ladder/ConflictReconciliation.lean proves the same merge for
every base and every pair of edits. This module adds the dynamic half (typing
on at the cursor after the merge) and the wedge configs:

  OperatorFirst = TRUE        -- a same-point merge puts the operator's append
                                 first (what a naive insert at the cursor
                                 does). `SamePointAgentFirst` MUST be violated
                                 (ConflictReconciliationOperatorFirstWedge.cfg).
  DropAgentOnConflict = TRUE  -- an overlap keeps the operator's text and
                                 silently drops the agent's (last writer wins).
                                 `AgentPreserved` MUST be violated
                                 (ConflictReconciliationDropWedge.cfg).

ConflictReconciliationReach.cfg asserts no conflict is ever surfaced and must
be violated, so rule 3 is actually exercised.
***************************************************************************)

CONSTANTS OperatorFirst, DropAgentOnConflict, MaxTyped

\* Repeated atoms exercise the common prefix/suffix stripping.
Base == <<"b1", "b2", "b1">>
OperatorAtoms == {"x", "y"}
AgentAtoms == {"x", "z"}
TypedAtom == "t"

Ins(A) == {<<>>} \cup {<<a>> : a \in A} \cup {<<a, b>> : a, b \in A}
Edits(A) == {[pos |-> p, len |-> l, ins |-> s] :
                p \in 0..Len(Base), l \in 0..Len(Base), s \in Ins(A)}
ValidEdit(e) == e.pos + e.len <= Len(Base)
Stop(e) == e.pos + e.len

Take(s, n) == IF n <= 0 THEN <<>> ELSE SubSeq(s, 1, IF n > Len(s) THEN Len(s) ELSE n)
Drop(s, n) == IF n >= Len(s) THEN <<>> ELSE SubSeq(s, n + 1, Len(s))
Reverse(s) == [i \in 1..Len(s) |-> s[Len(s) - i + 1]]
Min(a, b) == IF a < b THEN a ELSE b
Max(a, b) == IF a > b THEN a ELSE b

Apply(b, e) == Take(b, e.pos) \o e.ins \o Drop(b, Stop(e))

\* Buffer segments. One record shape for both kinds keeps TLC comparisons total.
Plain(a) == [kind |-> "plain", a |-> a, y |-> <<>>, g |-> <<>>]
Conflict(y, g) == [kind |-> "conflict", a |-> "none", y |-> y, g |-> g]
Plains(s) == [i \in 1..Len(s) |-> Plain(s[i])]

RECURSIVE CommonPrefixLen(_, _)
CommonPrefixLen(s, t) ==
    IF Len(s) = 0 \/ Len(t) = 0 \/ Head(s) # Head(t) THEN 0
    ELSE 1 + CommonPrefixLen(Tail(s), Tail(t))

\* Compact rendering: only the differing middle is inside the conflict.
Render(y, g) ==
    LET p  == CommonPrefixLen(y, g)
        r1 == Drop(y, p)
        r2 == Drop(g, p)
        q  == CommonPrefixLen(Reverse(r1), Reverse(r2))
        m1 == Take(r1, Len(r1) - q)
        m2 == Take(r2, Len(r2) - q)
    IN IF m1 = <<>> /\ m2 = <<>> THEN Plains(y)
       ELSE Plains(Take(y, p)) \o <<Conflict(m1, m2)>> \o Plains(Drop(r1, Len(r1) - q))

RECURSIVE Resolve(_, _)
Resolve(buf, k) ==
    IF Len(buf) = 0 THEN <<>>
    ELSE LET s == Head(buf)
             here == IF s.kind = "plain" THEN <<s.a>>
                     ELSE IF k = "yours" THEN s.y ELSE s.g
         IN here \o Resolve(Tail(buf), k)

ConflictFree(buf) == \A i \in 1..Len(buf) : buf[i].kind = "plain"

SamePoint(o, g) == o.len = 0 /\ g.len = 0 /\ o.pos = g.pos
Overlaps(o, g) == ~SamePoint(o, g) /\ ~(Stop(g) <= o.pos) /\ ~(Stop(o) <= g.pos)

\* The merge: buffer and cursor (-1 when a conflict is surfaced).
Merge(b, o, g) ==
    IF SamePoint(o, g) THEN
        IF OperatorFirst
        THEN [buf |-> Plains(Take(b, o.pos) \o o.ins \o g.ins \o Drop(b, o.pos)),
              cursor |-> o.pos + Len(o.ins)]
        ELSE [buf |-> Plains(Take(b, o.pos) \o g.ins \o o.ins \o Drop(b, o.pos)),
              cursor |-> o.pos + Len(g.ins) + Len(o.ins)]
    ELSE IF Stop(g) <= o.pos THEN
        [buf |-> Plains(Take(b, g.pos) \o g.ins \o Take(Drop(b, Stop(g)), o.pos - Stop(g))
                        \o o.ins \o Drop(b, Stop(o))),
         cursor |-> g.pos + Len(g.ins) + (o.pos - Stop(g)) + Len(o.ins)]
    ELSE IF Stop(o) <= g.pos THEN
        [buf |-> Plains(Take(b, o.pos) \o o.ins \o Take(Drop(b, Stop(o)), g.pos - Stop(o))
                        \o g.ins \o Drop(b, Stop(g))),
         cursor |-> o.pos + Len(o.ins)]
    ELSE
        LET lo == Min(o.pos, g.pos)
            hi == Max(Stop(o), Stop(g))
            Side(e) == Take(Drop(b, lo), e.pos - lo) \o e.ins \o Take(Drop(b, Stop(e)), hi - Stop(e))
        IN IF DropAgentOnConflict
           THEN [buf |-> Plains(Take(b, lo) \o Side(o) \o Drop(b, hi)), cursor |-> -1]
           ELSE [buf |-> Plains(Take(b, lo)) \o Render(Side(o), Side(g)) \o Plains(Drop(b, hi)),
                 cursor |-> -1]

VARIABLES o, g, buf, cursor, typed

vars == <<o, g, buf, cursor, typed>>

Init ==
    /\ o \in {e \in Edits(OperatorAtoms) : ValidEdit(e)}
    /\ g \in {e \in Edits(AgentAtoms) : ValidEdit(e)}
    /\ buf = Merge(Base, o, g).buf
    /\ cursor = Merge(Base, o, g).cursor
    /\ typed = <<>>

\* The operator keeps typing at the cursor (only outside a conflict: a
\* surfaced conflict is reconciled before the edit continues).
OperatorTypes ==
    /\ cursor >= 0
    /\ Len(typed) < MaxTyped
    /\ buf' = Take(buf, cursor) \o <<Plain(TypedAtom)>> \o Drop(buf, cursor)
    /\ cursor' = cursor + 1
    /\ typed' = Append(typed, TypedAtom)
    /\ UNCHANGED <<o, g>>

Next == OperatorTypes \/ UNCHANGED vars

Spec == Init /\ [][Next]_vars

Text == Resolve(buf, "yours")
OperatorText == o.ins \o typed
IsInfix(s, t) == \E i \in 0..(Len(t) - Len(s)) : SubSeq(t, i + 1, i + Len(s)) = s

TypeOK ==
    /\ cursor \in -1..(Len(Base) + 4 + MaxTyped)
    /\ \A i \in 1..Len(buf) : buf[i].kind \in {"plain", "conflict"}

\* Rules 1-3: the operator's text is never lost. Outside a conflict it sits
\* intact immediately before the cursor; in a conflict the operator's side
\* resolves to exactly the operator's edit.
OperatorNeverLost ==
    IF cursor >= 0
    THEN /\ cursor >= Len(OperatorText)
         /\ SubSeq(Text, cursor - Len(OperatorText) + 1, cursor) = OperatorText
    ELSE Resolve(buf, "yours") = Apply(Base, o)

\* Agent inserts are preserved: in the buffer outside a conflict, and as the
\* agent's resolution inside one.
AgentPreserved ==
    IF ConflictFree(buf) THEN IsInfix(g.ins, Text)
    ELSE Resolve(buf, "agent") = Apply(Base, g)

\* Rule 1: agent content first, operator's append (and typing) after it.
SamePointAgentFirst ==
    SamePoint(o, g) =>
        Take(Text, cursor) = Take(Base, o.pos) \o g.ins \o OperatorText

\* Rule 2: independent regions apply both edits and never conflict.
IndependentAppliesBoth ==
    (~SamePoint(o, g) /\ ~Overlaps(o, g)) => ConflictFree(buf)

\* Rule 3: a disagreeing overlap is surfaced, never silently resolved.
ConflictSurfaced ==
    (Overlaps(o, g) /\ Apply(Base, o) # Apply(Base, g)) => ~ConflictFree(buf)

\* Compact rendering: shared ends stay outside the marks.
CompactConflict ==
    \A i \in 1..Len(buf) :
        buf[i].kind = "conflict" =>
            /\ ~(buf[i].y = <<>> /\ buf[i].g = <<>>)
            /\ ~(buf[i].y # <<>> /\ buf[i].g # <<>> /\ Head(buf[i].y) = Head(buf[i].g))
            /\ ~(buf[i].y # <<>> /\ buf[i].g # <<>>
                 /\ buf[i].y[Len(buf[i].y)] = buf[i].g[Len(buf[i].g)])

\* Reach: MUST be violated (ConflictReconciliationReach.cfg).
NoConflictEver == ConflictFree(buf)

=============================================================================
