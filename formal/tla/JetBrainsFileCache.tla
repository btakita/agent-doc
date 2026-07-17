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
***************************************************************************)

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
cacheConflict = FALSE;

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

EventuallyConverged ==
<> (projectionSaved /\ ~retainedIntent /\ editorHasResponse /\ diskHasResponse)

=============================================================================
