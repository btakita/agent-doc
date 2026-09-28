------------------------- MODULE SubmoduleFocusRoot -------------------------
EXTENDS Naturals, TLC

(***************************************************************************
`#focussubmodulerecord`.  Editor auto-focus resolves a document's owning pane by
reading that document's actor record and session-registry entry.  Both live
under the project root that OWNS the document.  A document inside a nested
agent-doc project (a git submodule such as `src/haiven-dev`) is owned by the
submodule root, not by the controller's root.

The defect: the resolution read both bindings under the controller's root
unconditionally, so every nested document resolved to a root that holds no
binding and focus reported `missing_actor_record` with no pane -- silently, and
only for submodule documents.

`ResolveToOwningRoot` is the fix edge.  FALSE reproduces the pre-fix behavior,
which is what the wedge configuration checks.

Two properties matter beyond "nested documents focus":

  * resolution must never escape the controller's scope (the Rust guard is
    `root.starts_with(controller_root)`), so a document owned by a foreign root
    falls back to the controller root rather than steering focus into another
    project's state; and
  * a top-level document must resolve exactly as before, so the fix cannot
    perturb the paths that already worked.
***************************************************************************)

CONSTANT ResolveToOwningRoot

Documents == {"toplevel", "nested", "foreign"}

ControllerRoot == "controller"
SubmoduleRoot  == "submodule"
ForeignRoot    == "foreign_root"
Roots == {ControllerRoot, SubmoduleRoot, ForeignRoot}

\* Where each document's actor + registry binding actually lives.
OwningRoot == [ d \in Documents |->
    CASE d = "toplevel" -> ControllerRoot
      [] d = "nested"   -> SubmoduleRoot
      [] OTHER          -> ForeignRoot ]

\* Roots at or below the controller's scope.
WithinController == {ControllerRoot, SubmoduleRoot}

VARIABLES lookupRoot, focused, resolved
vars == <<lookupRoot, focused, resolved>>

\* The resolution rule under test.  Mirrors `focus_document_lookup_root`.
ChosenRoot(d) ==
    IF ResolveToOwningRoot /\ OwningRoot[d] \in WithinController
    THEN OwningRoot[d]
    ELSE ControllerRoot

Init ==
    /\ lookupRoot = [d \in Documents |-> ControllerRoot]
    /\ focused = [d \in Documents |-> FALSE]
    /\ resolved = {}

\* A focus attempt finds a binding exactly when it looked in the owning root.
Resolve(d) ==
    /\ d \notin resolved
    /\ lookupRoot' = [lookupRoot EXCEPT ![d] = ChosenRoot(d)]
    /\ focused' = [focused EXCEPT ![d] = (ChosenRoot(d) = OwningRoot[d])]
    /\ resolved' = resolved \cup {d}

Done == resolved = Documents /\ UNCHANGED vars

Next == (\E d \in Documents : Resolve(d)) \/ Done

Spec == Init /\ [][Next]_vars /\ \A d \in Documents : WF_vars(Resolve(d))

TypeOK ==
    /\ lookupRoot \in [Documents -> Roots]
    /\ focused \in [Documents -> BOOLEAN]
    /\ resolved \subseteq Documents

\* The bounded-escape guard: resolution never leaves the controller's scope.
LookupRootNeverEscapesController ==
    \A d \in Documents : lookupRoot[d] \in WithinController

\* Behavior preservation: a top-level document resolves exactly as before.
TopLevelResolutionUnchanged ==
    lookupRoot["toplevel"] = ControllerRoot

\* A document owned outside this controller is never claimed as focused.
ForeignDocumentNeverFocuses ==
    ~focused["foreign"]

\* Liveness: every in-scope document eventually focuses.
EventuallyInScopeDocumentsFocus ==
    <> (focused["toplevel"] /\ focused["nested"])

\* Reach obligation: asserted so it MUST be violated, proving the happy path is
\* reachable and the liveness property above is not passing vacuously.
NeverBothFocused ==
    [] ~(focused["toplevel"] /\ focused["nested"])

=============================================================================
