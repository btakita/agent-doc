--------------------------- MODULE IpcBuildIdentity ---------------------------
EXTENDS Naturals, FiniteSets, TLC

(***************************************************************************
The IPC handshake decides whether two agent-doc processes run the same code.

WHY THIS MODULE EXISTS
----------------------
`#ipcverhandshake` gave every peer a `build_id` and made the handshake admit a
peer only when both sides advertise the same one. Every consumer reads a
rejection as PROOF that the two processes differ, and that reading is
load-bearing: `EditorReplicaStrand` shows what a rejection costs an attached
document, and `VisibleDeliveryReceipt` shows what it costs a delivery cut.

The identity was `CARGO_PKG_VERSION + "+" + <wall-clock seconds when the build
script ran>`, and a clock cannot decide code equality. The handshake was
therefore wrong in BOTH directions at once, which is why neither direction was
noticed: each failure looked like the other one working.

  * FALSE MISMATCH. Two builds of byte-identical source taken at different
    moments carry different stamps. That is the ordinary workflow, not a corner
    case: `cargo install --path .` and `cargo build --release` use different
    target directories and so run the build script twice, as do a `cargo clean`,
    a second checkout, and a CI build of the same commit. Measured 2026-09-28T05:08Z
    on `tasks/agent-doc/agent-doc-bugs.md`: client `0.35.418+1790563363` against
    listener `0.35.418+1790567494`, rejected roughly twice a second, ending in
    `realtime_doc_resolve_missing_replica_terminal_rebuild_failed` +
    `realtime_doc_resolve_disk_read_refused` with `operator_action=none`.

  * FALSE MATCH. The build script declared `rerun-if-changed=src/`, which names
    only the root package. A cargo build script is never re-run for a change in
    a DIFFERENT package, and every one of the workspace's ~140 members is a
    different package - including the IPC crates themselves. So an edit to
    `agent-doc-ipc-io` or `agent-doc-crdt-relay-io` rebuilt the binary and
    carried the old stamp over verbatim. Measured the same night:
    `~/.cargo/bin/agent-doc` was written at 23:51:45 advertising `+1790563363`
    (22:42:43), from a tree whose `agent-doc-crdt-relay-io/src/lib.rs` had
    changed at 23:25.

WHAT THE MODEL SAYS
-------------------
`ContentDerived` selects the identity function. FALSE is the shipped stamp:
the clock reading recorded when the build script last ran. TRUE is the fix: a
digest of the build-relevant sources (`agent-doc-hash`'s `source_digest`).

Three build events generate the two failures, and each corresponds to something
a person does every day:

  * `RebuildUnchanged` - build the same tree again, later or elsewhere. Under a
    clock this moves the identity while the code stands still.
  * `EditMemberPackage` - change a member crate. The root build script does not
    re-run, so under a clock the identity stands still while the code moves.
  * `EditRootPackage` - change the root package. Both move; this is the only
    case the old stamp handled, and it is why the design looked correct.

Both properties are stated against the code AS JUDGED at handshake time
(`judged`), not against whatever the tree later became: a rebuild after a
handshake says nothing about the verdict that handshake reached.

Deliberately ABSENT: any action that reconciles two peers. Recycling the fleet
is the operator action the design must not require, and modelling it would
assume away exactly the wedge being measured. What a genuine mismatch must then
cost is `EditorReplicaStrand`'s question, not this module's - here the only
question is whether the mismatch was genuine at all.
***************************************************************************)

CONSTANTS
    MaxCode,        (* distinct source-tree generations explored *)
    MaxClock,       (* bounded wall clock *)
    ContentDerived  (* the fix under test *)

Peers == {"client", "listener"}
Outcomes == {"admitted", "rejected"}

VARIABLES
    code,     (* peer -> the source generation it was built from *)
    stamp,    (* peer -> the clock reading its build script recorded *)
    clock,
    judged,   (* peer -> the source generation it had at the last handshake *)
    verdict,  (* "none" | "admitted" | "rejected" *)
    seen      (* every outcome the handshake has produced *)

vars == << code, stamp, clock, judged, verdict, seen >>

Init ==
    /\ code = [p \in Peers |-> 1]
    /\ stamp = [p \in Peers |-> 0]
    /\ clock = 0
    /\ judged = [p \in Peers |-> 1]
    /\ verdict = "none"
    /\ seen = {}

TypeOK ==
    /\ code \in [Peers -> 1..MaxCode]
    /\ stamp \in [Peers -> 0..MaxClock]
    /\ clock \in 0..MaxClock
    /\ judged \in [Peers -> 1..MaxCode]
    /\ verdict \in {"none"} \cup Outcomes
    /\ seen \subseteq Outcomes

(*************************************************************************)
(* The identity each peer advertises. This one line is the whole fix.    *)
(*************************************************************************)
Identity(p) == IF ContentDerived THEN code[p] ELSE stamp[p]

(* Same sources, built again - a second target directory, a clean, another
   checkout, CI. Under a clock the identity moves although nothing did. *)
RebuildUnchanged(p) ==
    /\ clock < MaxClock
    /\ clock' = clock + 1
    /\ stamp' = [stamp EXCEPT ![p] = clock + 1]
    /\ UNCHANGED << code, judged, verdict, seen >>

(* A member crate changed. The root build script is not re-run for another
   package, so under a clock the identity does NOT move although the code did. *)
EditMemberPackage(p) ==
    /\ code[p] < MaxCode
    /\ code' = [code EXCEPT ![p] = code[p] + 1]
    /\ UNCHANGED << stamp, clock, judged, verdict, seen >>

(* The root package changed, so the build script re-runs too. The only shape
   the stamp ever handled correctly. *)
EditRootPackage(p) ==
    /\ code[p] < MaxCode
    /\ clock < MaxClock
    /\ code' = [code EXCEPT ![p] = code[p] + 1]
    /\ clock' = clock + 1
    /\ stamp' = [stamp EXCEPT ![p] = clock + 1]
    /\ UNCHANGED << judged, verdict, seen >>

Handshake ==
    /\ verdict' = IF Identity("client") = Identity("listener")
                    THEN "admitted"
                    ELSE "rejected"
    /\ judged' = code
    /\ seen' = seen \cup {verdict'}
    /\ UNCHANGED << code, stamp, clock >>

Next ==
    \/ \E p \in Peers : RebuildUnchanged(p)
    \/ \E p \in Peers : EditMemberPackage(p)
    \/ \E p \in Peers : EditRootPackage(p)
    \/ Handshake

Spec == Init /\ [][Next]_vars /\ WF_vars(Handshake)

(*************************************************************************)
(* SAFETY - the two halves of "the identity decides code equality".      *)
(*************************************************************************)

(* Soundness of the rejection. A rejection is consumed as proof that the peers
   differ, and every consumer acts on it irreversibly, so a rejection between
   equivalent peers is a fabricated proof. This is the half whose cost is an
   unreachable document. *)
EquivalentCodeIsAdmitted ==
    (verdict = "rejected") => judged["client"] # judged["listener"]

(* Completeness of the rejection. The handshake exists to stop skewed peers from
   speaking; admitting two peers that differ is the check silently not running.
   This is the half that hid the other one - a stamp that never moved looked
   exactly like a fleet in agreement. *)
DivergentCodeIsRejected ==
    (verdict = "admitted") => judged["client"] = judged["listener"]

(*************************************************************************)
(* NON-VACUITY. Both properties above are implications, so a handshake    *)
(* that never admitted - or never rejected - would satisfy one of them    *)
(* for free. `IpcBuildIdentityReach.cfg` requires this to be VIOLATED     *)
(* under the fix, which is what proves the fixed handshake still makes    *)
(* both decisions rather than having become a constant.                   *)
(*************************************************************************)
BothOutcomesUnreachable == seen # Outcomes

===============================================================================
