--------------------------- MODULE AgentDocCloseout ---------------------------
EXTENDS Naturals, TLC

CONSTANT MaxAckFailures

(***************************************************************************
The PlusCal algorithm models the closeout fault family behind sample-app and
sample-portal incidents.  A turn starts with a retained response and an
old preflight/plugin generation.  `make install` first converges the package on
disk, an editor restart publishes that package as the live generation, and the
closeout actor must revalidate before replaying and committing.  Delivery ACKs
may fail a bounded number of times; neither those retries nor the concurrent
install/restart may duplicate the response or erase later steering.
***************************************************************************)

(* --fair algorithm CloseoutGeneration
variables
    sourcePluginGen = 2,
    diskPluginGen = 1,
    livePluginGen = 1,
    preflightPluginGen = 1,
    recognizedPluginGen = 1,
    nativeGen = 2,
    installCompleted = FALSE,
    editorRestarted = FALSE,
    responseState = "retained",
    responseCopies = 1,
    steeringPresent = TRUE,
    ackFailuresRemaining = MaxAckFailures,
    acked = FALSE,
    committed = FALSE;

process Installer = "installer"
begin
InstallPackage:
    diskPluginGen := sourcePluginGen;
    installCompleted := TRUE;
InstallerDone:
    while TRUE do
        skip;
    end while;
end process;

process Editor = "editor"
begin
AwaitInstalledPackage:
    await installCompleted;
RestartEditor:
    livePluginGen := diskPluginGen;
    editorRestarted := TRUE;
EditorDone:
    while TRUE do
        skip;
    end while;
end process;

process Closeout = "closeout"
begin
RetainedCapture:
    assert responseState = "retained" /\ responseCopies = 1;
AwaitLiveGeneration:
    await editorRestarted;
RevalidateGeneration:
    recognizedPluginGen := livePluginGen;
ReplayCapture:
    responseState := "replayed";
AckLoop:
    while ~acked do
        if ackFailuresRemaining > 0 then
            ackFailuresRemaining := ackFailuresRemaining - 1;
        else
            acked := TRUE;
        end if;
    end while;
CommitCapture:
    responseState := "committed";
    committed := TRUE;
CloseoutDone:
    while TRUE do
        skip;
    end while;
end process;
end algorithm; *)

TypeOK ==
    /\ sourcePluginGen \in Nat
    /\ diskPluginGen \in Nat
    /\ livePluginGen \in Nat
    /\ preflightPluginGen \in Nat
    /\ recognizedPluginGen \in Nat
    /\ nativeGen \in Nat
    /\ installCompleted \in BOOLEAN
    /\ editorRestarted \in BOOLEAN
    /\ responseState \in {"retained", "replayed", "committed"}
    /\ responseCopies \in Nat
    /\ steeringPresent \in BOOLEAN
    /\ ackFailuresRemaining \in 0..MaxAckFailures
    /\ acked \in BOOLEAN
    /\ committed \in BOOLEAN

MakeInstallConvergesDiskPackage ==
    installCompleted => diskPluginGen = sourcePluginGen

NativeReloadDoesNotMasqueradeAsPackageUpdate ==
    nativeGen = sourcePluginGen /\ preflightPluginGen < sourcePluginGen
        => ~installCompleted \/ diskPluginGen = sourcePluginGen

RecognizedGenerationNeverLeadsLiveEditor ==
    recognizedPluginGen <= livePluginGen

RetainedResponseIsUnique ==
    responseCopies = 1

CommitRequiresExactDurableResponse ==
    committed =>
        /\ acked
        /\ responseState = "committed"
        /\ responseCopies = 1
        /\ steeringPresent
        /\ recognizedPluginGen = livePluginGen

CommittedTurnAdoptsNewerLiveGeneration ==
    committed => recognizedPluginGen > preflightPluginGen

EventuallyInstalled == <>installCompleted
EventuallyEditorPublishesInstalledGeneration ==
    <>(editorRestarted /\ livePluginGen = diskPluginGen)
EventuallyCommitted == <>committed

=============================================================================
