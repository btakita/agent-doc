-------------------------- MODULE JetBrainsFileCache --------------------------
EXTENDS Naturals, TLC

(***************************************************************************
Models the repaired deferred-reconnect delivery.  The JetBrains adapter first
refreshes a clean VirtualFile stamp, installs and saves the canonical frontier,
and only then ACKs.  The controller observes that disk already has the same
bytes and performs no redundant atomic rewrite.  Concurrent VFS refreshes must
therefore never observe an unsaved document with a newer disk stamp.
***************************************************************************)

Texts == {"baseline", "canonical"}

(* --fair algorithm SavedReconnectProjection
variables
editorText = "baseline",
diskText = "baseline",
documentStamp = 0,
fileStamp = 0,
documentDirty = FALSE,
deliveryAcked = FALSE,
controllerDiskWrites = 0,
cacheConflict = FALSE;

process Plugin = "plugin"
begin
RefreshCleanTarget:
assert ~documentDirty;
documentStamp := fileStamp;
InstallAndSaveCanonical:
editorText := "canonical";
diskText := "canonical";
fileStamp := fileStamp + 1;
documentStamp := fileStamp;
documentDirty := FALSE;
AckSavedFrontier:
assert editorText = "canonical" /\ diskText = "canonical" /\ ~documentDirty;
deliveryAcked := TRUE;
PluginDone:
while TRUE do
skip;
end while;
end process;

process Controller = "controller"
begin
AwaitAck:
await deliveryAcked;
ProjectIfNeeded:
if diskText # "canonical" then
diskText := "canonical";
fileStamp := fileStamp + 1;
controllerDiskWrites := controllerDiskWrites + 1;
end if;
ControllerDone:
while TRUE do
skip;
end while;
end process;

process Vfs = "vfs"
begin
VfsRefresh:
while TRUE do
if documentDirty /\ documentStamp # fileStamp then
cacheConflict := TRUE;
else
documentStamp := fileStamp;
end if;
end while;
end process;
end algorithm; *)

TypeOK ==
/\ editorText \in Texts
/\ diskText \in Texts
/\ documentStamp \in Nat
/\ fileStamp \in Nat
/\ documentDirty \in BOOLEAN
/\ deliveryAcked \in BOOLEAN
/\ controllerDiskWrites \in Nat
/\ cacheConflict \in BOOLEAN

NoFileCacheConflict == ~cacheConflict

AckRequiresSavedCanonical ==
deliveryAcked =>
/\ editorText = "canonical"
/\ diskText = "canonical"
/\ ~documentDirty
/\ documentStamp = fileStamp

NoRedundantControllerProjection == controllerDiskWrites = 0

EventuallySavedAndAcked ==
<> (deliveryAcked /\ editorText = diskText /\ documentStamp = fileStamp)

=============================================================================
