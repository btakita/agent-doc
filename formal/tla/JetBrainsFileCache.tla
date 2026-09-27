-------------------------- MODULE JetBrainsFileCache --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Models editor-first reload/reregister and granular retained-intent replay.

The live IntelliJ Document is authoritative while attached.  An operator may
author a prompt and delete a queue item without saving.  A plugin/native reload
must publish that exact editor cut before a retained agent response is replayed;
the replay changes only the response cell.  No transition may install an older
whole-document target, resurrect the deleted queue item, duplicate the exchange
boundary, or require a save before the operator cut becomes authoritative.

RE-REGISTRATION CAN BE REJECTED
-------------------------------
`ReregisterFromExactEditorCut` used to be an unconditional assignment: the
plugin always answered and always published the cut.  `EventuallyConverged` was
therefore true by construction rather than by design, and this module certified
convergence for months while production wedged on the one outcome it could not
express — a live endpoint that answers the re-registration with
`{"type":"receipt","status":"rejected"}`.

Measured 2026-09-27 18:28:54-18:28:58Z on five attached documents one minute
after a mid-session `make install`: three rejected receipts, then
`realtime_doc_resolve_missing_replica_terminal_rebuild_failed` and
`realtime_doc_resolve_disk_read_refused editor_open=true`, with no transition
left.  A cdylib generation swap drops the replica and leaves the old instance
unable to serve the document, while the attachment latch still says attached.

The endpoint now answers nondeterministically, so TLC explores both branches:

  * it serves      -> the original editor-first path, unchanged;
  * it refuses     -> a definitive negative answer, and the recovery edge
                      (`DemoteOnDefinitiveRejection`) demotes the stale
                      attachment so disk becomes a legal authority.

That split is why the liveness claim had to be split too.  Full editor-first
convergence needs the editor, so it CANNOT be promised when the endpoint refuses
— promising it anyway is exactly the vacuity this change removes.  What is
promised unconditionally is `EventuallyResolved`: every behaviour reaches an
authority.  `ServedEndpointEventuallyConverges` keeps the original, stronger
claim where it is actually achievable, and the safety invariants below pin the
refusal branch so the fallback cannot be a disguised `--force-disk`: it may
never publish a cut it could not read, never cost the operator the unsaved cut,
and never make disk authoritative while the endpoint still serves.

`JetBrainsFileCacheWedge.cfg` disables the recovery edge and is required to
VIOLATE `EventuallyResolved` (`scripts/run_tla.sh` must-violate list).  A
property that cannot fail is not evidence, and this module is the reason that
obligation now exists.
***************************************************************************)

CONSTANT DemoteOnDefinitiveRejection

(* --fair algorithm EditorFirstReconnect
variables
editorHasPrompt = FALSE,
editorQueuePresent = TRUE,
editorHasResponse = FALSE,
editorDirty = FALSE,
diskHasPrompt = FALSE,
diskQueuePresent = TRUE,
diskHasResponse = FALSE,
canonicalHasPrompt = FALSE,
canonicalQueuePresent = TRUE,
canonicalHasResponse = FALSE,
operatorCutAuthored = FALSE,
operatorCutPublished = FALSE,
retainedIntent = TRUE,
projectionSaved = FALSE,
boundaryCount = 1,
documentStamp = 0,
fileStamp = 0,
cacheConflict = FALSE,
endpointServes = TRUE,
endpointRefused = FALSE,
diskAuthoritative = FALSE;

process Operator = "operator"
begin
AuthorUnsavedCut:
editorHasPrompt := TRUE;
editorQueuePresent := FALSE;
editorDirty := TRUE;
operatorCutAuthored := TRUE;
OperatorDone:
while TRUE do
skip;
end while;
end process;

process Plugin = "plugin"
begin
AwaitOperatorCut:
await operatorCutAuthored;
AnswerReregister:
with serves \in BOOLEAN do
endpointServes := serves;
end with;
ClassifyReregisterAnswer:
if ~endpointServes then
goto RecordDefinitiveRefusal;
end if;
ReregisterFromExactEditorCut:
canonicalHasPrompt := editorHasPrompt;
canonicalQueuePresent := editorQueuePresent;
canonicalHasResponse := editorHasResponse;
operatorCutPublished := TRUE;
ReplayRetainedResponseCell:
await operatorCutPublished;
canonicalHasResponse := TRUE;
editorHasResponse := TRUE;
retainedIntent := FALSE;
SaveConvergedProjection:
diskHasPrompt := editorHasPrompt;
diskQueuePresent := editorQueuePresent;
diskHasResponse := editorHasResponse;
fileStamp := fileStamp + 1;
documentStamp := fileStamp;
editorDirty := FALSE;
projectionSaved := TRUE;
goto PluginDone;
RecordDefinitiveRefusal:
endpointRefused := TRUE;
if DemoteOnDefinitiveRejection then
diskAuthoritative := TRUE;
end if;
PluginDone:
while TRUE do
skip;
end while;
end process;

process Vfs = "vfs"
begin
VfsRefresh:
while TRUE do
if editorDirty /\ documentStamp # fileStamp then
cacheConflict := TRUE;
else
documentStamp := fileStamp;
end if;
end while;
end process;
end algorithm; *)

TypeOK ==
/\ editorHasPrompt \in BOOLEAN
/\ editorQueuePresent \in BOOLEAN
/\ editorHasResponse \in BOOLEAN
/\ editorDirty \in BOOLEAN
/\ diskHasPrompt \in BOOLEAN
/\ diskQueuePresent \in BOOLEAN
/\ diskHasResponse \in BOOLEAN
/\ canonicalHasPrompt \in BOOLEAN
/\ canonicalQueuePresent \in BOOLEAN
/\ canonicalHasResponse \in BOOLEAN
/\ operatorCutAuthored \in BOOLEAN
/\ operatorCutPublished \in BOOLEAN
/\ retainedIntent \in BOOLEAN
/\ projectionSaved \in BOOLEAN
/\ boundaryCount \in Nat
/\ documentStamp \in Nat
/\ fileStamp \in Nat
/\ cacheConflict \in BOOLEAN
/\ endpointServes \in BOOLEAN
/\ endpointRefused \in BOOLEAN
/\ diskAuthoritative \in BOOLEAN

OperatorIntentIsMonotonic ==
operatorCutAuthored => editorHasPrompt /\ ~editorQueuePresent

PublishedBaselineContainsOperatorCut ==
operatorCutPublished => canonicalHasPrompt /\ ~canonicalQueuePresent

ResponseReplayIsGranular ==
canonicalHasResponse => canonicalHasPrompt /\ ~canonicalQueuePresent

SavedProjectionContainsOperatorCut ==
projectionSaved =>
/\ diskHasPrompt
/\ ~diskQueuePresent
/\ diskHasResponse
/\ ~editorDirty
/\ documentStamp = fileStamp

SingletonBoundary == boundaryCount = 1

NoFileCacheConflict == ~cacheConflict

(***************************************************************************
Safety of the refusal branch.  Falling back must not become a `--force-disk`.
***************************************************************************)

\* A refused re-registration never read the editor, so it must never claim to
\* have published the operator's cut.
RefusalNeverPublishesACut ==
endpointRefused => ~operatorCutPublished

\* And it must not cost the operator the unsaved cut: the text stays in the live
\* editor, to be merged back when a tab re-registers.
OperatorCutSurvivesRefusal ==
endpointRefused => (editorHasPrompt /\ ~editorQueuePresent)

\* Disk may become authoritative only on a proven refusal — never while the
\* endpoint is still serving the document.
DiskAuthorityRequiresRefusal ==
diskAuthoritative => endpointRefused

(***************************************************************************
Liveness.  Split because full editor-first convergence needs the editor, and
cannot be promised for an endpoint that refuses to serve the document.
***************************************************************************)

\* Unconditional progress: every behaviour reaches an authority.  This is the
\* property the production wedge violated, and the one the wedge config must
\* keep violating.
EventuallyResolved ==
<>(projectionSaved \/ diskAuthoritative)

\* The original claim, kept exactly where it is achievable.
ServedEndpointEventuallyConverges ==
<>(endpointRefused
   \/ (projectionSaved /\ ~retainedIntent /\ editorHasResponse /\ diskHasResponse))

(***************************************************************************
Reachability obligation, the counterpart to the wedge config.

Splitting the liveness claim opened a SECOND way to go vacuous: if the served
branch stopped being reachable — a mistyped guard, an `await` that never fires —
then `ServedEndpointEventuallyConverges` would hold because `endpointRefused` is
always true, and every invariant below it would hold because its antecedent never
becomes true. The module would pass while checking nothing about the editor-first
path it exists to specify.

So `JetBrainsFileCacheReach.cfg` checks the negation as an INVARIANT and REQUIRES
it to be violated. A violation is TLC exhibiting a behaviour that fully converges,
which is the proof that the happy path is still live. The wedge config proves the
refusal branch is reachable the same way, through its counter-example trace.
***************************************************************************)
FullConvergenceIsUnreachable ==
~(projectionSaved /\ ~retainedIntent /\ editorHasResponse /\ diskHasResponse)

=============================================================================
