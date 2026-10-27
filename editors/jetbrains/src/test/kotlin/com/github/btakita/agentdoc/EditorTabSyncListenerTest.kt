package com.github.btakita.agentdoc

import com.google.gson.Gson
import com.google.gson.JsonParser
import java.nio.file.Files
import java.nio.file.Paths
import java.util.concurrent.atomic.AtomicReference
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The plugin does not plan layout (`#jbsurfaceswap`) — dedup and reconciliation are the reactive
 * graph's, exercised by `agent-doc-editor-surface`. The editor reports the surface it actually
 * sees and separately publishes focused-document state to that document's own controller so
 * cross-project splits get a derived pane handoff.
 */
class EditorTabSyncListenerTest {
    private val gson = Gson()

    private fun surfaceJson(
        focusedFile: String,
        visibleMdFiles: List<String>,
        editorLayout: EditorLayout? = null,
        forceReconcile: Boolean = false,
        preserveFocus: Boolean = false,
    ): String =
        gson.toJson(
            EditorTabSyncListener.SurfaceReport.buildSurface(
                focusedFile = focusedFile,
                visibleMdFiles = visibleMdFiles,
                editorLayout = editorLayout,
                forceReconcile = forceReconcile,
                preserveFocus = preserveFocus,
            )
        )

    @Test
    fun `focus projection is generation fenced and requires the active project window`() {
        assertTrue(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 6,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = false,
            ),
        )
    }

    @Test
    fun `remote controller selection is focus evidence without a backend active window`() {
        assertTrue(
            EditorTabSyncListener.shouldPublishFocusProjection(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = false,
                remoteControllerSelectionFocusEvidence = true,
            ),
        )
        assertEquals(
            EditorTabSyncListener.FocusProjectionPublicationGate.Superseded,
            EditorTabSyncListener.focusProjectionPublicationGate(
                requestedGeneration = 6,
                currentGeneration = 7,
                projectWindowActive = false,
                remoteControllerSelectionFocusEvidence = true,
                listenerClosed = false,
                projectDisposed = false,
            ),
        )
        assertEquals(
            EditorTabSyncListener.FocusProjectionPublicationGate.Inactive,
            EditorTabSyncListener.focusProjectionPublicationGate(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = false,
                remoteControllerSelectionFocusEvidence = false,
                listenerClosed = false,
                projectDisposed = false,
            ),
        )
        assertEquals(
            EditorTabSyncListener.FocusProjectionPublicationGate.Disposed,
            EditorTabSyncListener.focusProjectionPublicationGate(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = true,
                remoteControllerSelectionFocusEvidence = true,
                listenerClosed = false,
                projectDisposed = true,
            ),
        )
        assertEquals(
            EditorTabSyncListener.FocusProjectionPublicationGate.Closed,
            EditorTabSyncListener.focusProjectionPublicationGate(
                requestedGeneration = 7,
                currentGeneration = 7,
                projectWindowActive = true,
                remoteControllerSelectionFocusEvidence = true,
                listenerClosed = true,
                projectDisposed = false,
            ),
        )
    }

    @Test
    fun `settled selection claims focus only for the current active split`() {
        assertTrue(
            EditorTabSyncListener.shouldClaimSettledSelectionFocus(
                selectionPath = "/repo/tasks/right.md",
                activeWindowPath = "/repo/tasks/right.md",
                requestedGeneration = 8,
                currentGeneration = 8,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldClaimSettledSelectionFocus(
                selectionPath = "/repo/tasks/right.md",
                activeWindowPath = "/repo/tasks/left.md",
                requestedGeneration = 8,
                currentGeneration = 8,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldClaimSettledSelectionFocus(
                selectionPath = "/repo/tasks/right.md",
                activeWindowPath = "/repo/tasks/right.md",
                requestedGeneration = 7,
                currentGeneration = 8,
                projectWindowActive = true,
            ),
        )
        assertFalse(
            EditorTabSyncListener.shouldClaimSettledSelectionFocus(
                selectionPath = "/repo/tasks/right.md",
                activeWindowPath = "/repo/tasks/right.md",
                requestedGeneration = 8,
                currentGeneration = 8,
                projectWindowActive = false,
            ),
        )
    }

    @Test
    fun `late stashed focus receipt cannot replace a newer document projection`() {
        val request =
            FocusProjectionEffectRequest(
                generation = 7,
                projectRoot = "/repo",
                filePath = "/repo/tasks/old.md",
                surfaceJson = "{}",
            )
        val stashedResponse =
            FocusProjectionEffectResponse(
                request = request,
                transport =
                    CpEditorRouteResult(
                        exitCode = 0,
                        output =
                            """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"actor_pane_not_visible\"}"}""",
                    ),
            )

        assertEquals(
            EditorTabSyncListener.FocusProjectionReceiptDecision.Superseded,
            EditorTabSyncListener.decideFocusProjectionReceipt(
                response = stashedResponse,
                currentGeneration = 8,
                projectWindowActive = true,
            ),
        )
        assertEquals(
            EditorTabSyncListener.FocusProjectionReceiptDecision.RepairLayout,
            EditorTabSyncListener.decideFocusProjectionReceipt(
                response = stashedResponse,
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )
        assertEquals(
            EditorTabSyncListener.FocusProjectionReceiptDecision.TransportUnavailable,
            EditorTabSyncListener.decideFocusProjectionReceipt(
                response =
                    stashedResponse.copy(
                        transport = CpEditorRouteResult(exitCode = 1, output = "timed out"),
                    ),
                currentGeneration = 7,
                projectWindowActive = true,
            ),
        )

        for (
            reason in
                listOf(
                    "focus_escalated_to_structural_layout",
                    "focus_escalated_to_lane_layout",
                    "focus_deferred:no_frontend_layout_snapshot",
                    "focus_deferred:focus_requires_frontend_layout",
                )
        ) {
            val terminalResponse =
                stashedResponse.copy(
                    transport =
                        CpEditorRouteResult(
                            exitCode = 0,
                            output =
                                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"$reason\"}"}""",
                        ),
                )
            assertEquals(
                reason,
                EditorTabSyncListener.FocusProjectionReceiptDecision.Retained,
                EditorTabSyncListener.decideFocusProjectionReceipt(
                    response = terminalResponse,
                    currentGeneration = 7,
                    projectWindowActive = true,
                ),
            )
        }
    }

    @Test
    fun `missing selected pane requests generation fenced structural repair`() {
        val repairReasons =
            listOf(
                "actor_pane_not_visible",
                "actor_not_focusable",
                "missing_actor_record",
                "actor_pane_not_alive",
                "registry_pane_not_alive",
                "live_owner_pane_not_alive",
            )

        for (reason in repairReasons) {
            val response =
                FocusProjectionEffectResponse(
                    request =
                        FocusProjectionEffectRequest(
                            generation = 11,
                            projectRoot = "/repo",
                            filePath = "/repo/tasks/reaped.md",
                            surfaceJson = "{}",
                        ),
                    transport =
                        CpEditorRouteResult(
                            exitCode = 0,
                            output =
                                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"$reason\"}"}""",
                        ),
                )

            assertEquals(
                reason,
                EditorTabSyncListener.FocusProjectionReceiptDecision.RepairLayout,
                EditorTabSyncListener.decideFocusProjectionReceipt(
                    response = response,
                    currentGeneration = 11,
                    projectWindowActive = true,
                ),
            )
        }

        assertFalse(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"desktop_editor_inactive\"}"}""",
            ),
        )
    }

    @Test
    fun `passive focus effect has a short transport deadline`() {
        assertTrue(CpRouteClient.EDITOR_FOCUS_OBSERVE_TIMEOUT_MS in 1..2_000)
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt",
                    ),
            )
        val focusRequest =
            source
                .substringAfter("fun observeEditorFocus(")
                .substringBefore("private fun observeEditorSurfaceWithClient(")
        assertTrue(focusRequest.contains("timeoutMs = EDITOR_FOCUS_OBSERVE_TIMEOUT_MS"))
    }

    @Test
    fun `surface admission owns focus effect invalidation`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val admission =
            source
                .substringAfter("private fun requestObservation(")
                .substringBefore("private fun scheduleRetainedSurfaceRetry(")
        assertTrue(
            admission.contains(
                "if (requiredFocusGeneration == null && !observation.preserveFocus)"
            )
        )
        assertTrue(admission.contains("invalidateFocusProjection()"))
        assertTrue(source.contains("activeFocusProjection.getAndSet(null)?.cancel(true)"))
    }

    @Test
    fun `focus projection captures editor surface before background scheduling`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val focusProjection =
            source
                .substringAfter("private fun requestFocusProjection(")
                .substringBefore("private fun shutdown()")
        val surfaceCapture = focusProjection.indexOf("val focusWindow =")
        val surfaceIdCapture =
            focusProjection.indexOf("JetBrainsEditorSurfaces.forEditorWindow(project, focusWindow)")
        val backgroundSchedule = focusProjection.indexOf("focusProjectionExecutor.schedule(")

        assertTrue(surfaceCapture >= 0)
        assertTrue(surfaceIdCapture > surfaceCapture)
        assertTrue(backgroundSchedule >= 0)
        assertTrue(surfaceIdCapture < backgroundSchedule)
        val backgroundTask = focusProjection.substring(backgroundSchedule)
        assertFalse(backgroundTask.contains("manager.currentWindow"))
        assertFalse(backgroundTask.contains("JetBrainsEditorSurfaces.forEditorWindow"))
        assertTrue(backgroundTask.contains("continueAfterGate(\"before_root_resolution\")"))
    }

    @Test
    fun `focus projection installs a handoff lease only after exact pane selection`() {
        assertTrue(
            EditorTabSyncListener.focusProjectionApplied(
                """{"idle":false,"outcome":"{\"focused\":true,\"reason\":\"focused\"}","error":null}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionApplied(
                """{"idle":false,"outcome":null,"error":"missing_actor_record"}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionApplied(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"missing_actor_record\"}"}""",
            ),
        )
    }

    @Test
    fun `unusable focus projection requests structural layout repair`() {
        assertTrue(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"actor_pane_not_visible\"}"}""",
            ),
        )
        assertTrue(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":false,\"reason\":\"missing_actor_record\"}"}""",
            ),
        )
        assertFalse(
            EditorTabSyncListener.focusProjectionRequiresLayoutRepair(
                """{"idle":false,"outcome":"{\"focused\":true,\"reason\":\"focused\"}"}""",
            ),
        )
    }

    @Test
    fun `selection event file wins over stale selected editor file`() {
        val visibleMdFiles =
            listOf(
                "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                "/repo/tasks/professional/sampleportal.md",
            )

        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = "/repo/tasks/agent-doc/agent-doc-bugs2.md",
                selectedEditorFile = "/repo/tasks/professional/sampleportal.md",
                visibleMdFiles = visibleMdFiles,
            )

        assertEquals("/repo/tasks/agent-doc/agent-doc-bugs2.md", activeFile)
    }

    @Test
    fun `focused document projection is explicitly selection only`() {
        val projection =
            JsonParser.parseString(
                gson.toJson(
                EditorTabSyncListener.SurfaceReport.buildFocusProjection(
                    "/repo/src/sample-app/tasks/sample-app.md",
                ),
            ),
        ).asJsonObject

        assertEquals(
            "/repo/src/sample-app/tasks/sample-app.md",
            projection.get("focused").asString,
        )
        assertTrue(projection.get("focus_only").asBoolean)
        assertTrue(projection.get("force_reconcile").asBoolean)
        assertEquals(0, projection.getAsJsonArray("columns").size())
    }

    @Test
    fun `captured selection releases precedence while controller delivery is in flight`() {
        val captured = Any()
        val newerObservation = Any()
        val slot = AtomicReference<Any?>(captured)

        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.releaseForDelivery(
                slot,
                captured,
            ),
        )
        assertTrue(slot.compareAndSet(null, newerObservation))
        assertFalse(
            EditorTabSyncListener.ObservationDeliveryOwnership.retainAfterFailure(
                slot,
                captured,
            ),
        )
        assertEquals(newerObservation, slot.get())
    }

    @Test
    fun `failed delivery retains the captured observation when no newer event exists`() {
        val captured = Any()
        val slot = AtomicReference<Any?>(captured)

        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.releaseForDelivery(
                slot,
                captured,
            ),
        )
        assertTrue(
            EditorTabSyncListener.ObservationDeliveryOwnership.retainAfterFailure(
                slot,
                captured,
            ),
        )
        assertEquals(captured, slot.get())
    }

    @Test
    fun `failed surface delivery retries promptly then settles at a bounded rate`() {
        assertEquals(100L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(1))
        assertEquals(200L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(2))
        assertEquals(1_600L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(5))
        assertEquals(2_000L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(6))
        assertEquals(2_000L, EditorTabSyncListener.SurfaceDeliveryRetry.delayMs(100))
    }

    @Test
    fun `publishing through a new controller root retires the prior layout authority`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempt("/repo/nested")
        assertTrue(ownership.markPublished("/repo/nested").isEmpty())

        ownership.recordAttempt("/repo")
        assertEquals(listOf("/repo/nested"), ownership.markPublished("/repo"))
        assertTrue(ownership.markForgotten("/repo/nested"))
        assertTrue(ownership.markPublished("/repo").isEmpty())
        assertEquals(listOf("/repo"), ownership.drain())
    }

    @Test
    fun `a fresh plugin process retires controller roots discovered from open documents`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempts(listOf("/repo", "/repo/nested"))

        assertEquals(
            listOf("/repo/nested"),
            ownership.rootsToRetireBeforePublishing("/repo"),
        )
        assertTrue(ownership.markForgotten("/repo/nested"))
        assertTrue(ownership.markPublished("/repo").isEmpty())
        assertEquals(listOf("/repo"), ownership.drain())
    }

    @Test
    fun `prior process roots are retired before replacement publication can block on layout`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempts(listOf("/repo", "/repo/nested"))

        assertEquals(
            listOf("/repo/nested"),
            ownership.rootsToRetireBeforePublishing("/repo"),
        )
        assertTrue(ownership.markForgotten("/repo/nested"))
        assertTrue(ownership.rootsToRetireBeforePublishing("/repo").isEmpty())
    }

    @Test
    fun `failed superseded root retirement remains retryable`() {
        val ownership = EditorTabSyncListener.SurfaceRootOwnership()

        ownership.recordAttempt("/repo/nested")
        ownership.markPublished("/repo/nested")
        ownership.recordAttempt("/repo")

        assertEquals(listOf("/repo/nested"), ownership.markPublished("/repo"))
        assertEquals(
            "an unacknowledged forget must remain eligible for the next delivery",
            listOf("/repo/nested"),
            ownership.markPublished("/repo"),
        )
    }

    @Test
    fun `closing a detached frame retires only that surface endpoint`() {
        val ownership = EditorTabSyncListener.SurfaceEndpointOwnership()
        val main = EditorTabSyncListener.SurfaceEndpoint("/repo", "main")
        val detached = EditorTabSyncListener.SurfaceEndpoint("/repo", "dock-window-7")

        assertTrue(ownership.markPublished(main, setOf("main", "dock-window-7")).isEmpty())
        assertTrue(ownership.markPublished(detached, setOf("main", "dock-window-7")).isEmpty())
        assertEquals(listOf(detached), ownership.missing(setOf("main")))
        assertEquals(listOf(detached), ownership.markPublished(main, setOf("main")))
        assertTrue(ownership.markForgotten(detached))
        assertEquals(listOf(main), ownership.drain())
    }

    @Test
    fun `missing backend windows do not retire a freshly published remote endpoint`() {
        val ownership = EditorTabSyncListener.SurfaceEndpointOwnership()
        val remote = EditorTabSyncListener.SurfaceEndpoint("/repo", "surface-unavailable")

        assertTrue(ownership.markPublished(remote, null).isEmpty())
        assertEquals(listOf(remote), ownership.drain())
    }

    @Test
    fun `published endpoint cannot retire itself when IDE window enumeration lags`() {
        val ownership = EditorTabSyncListener.SurfaceEndpointOwnership()
        val detached = EditorTabSyncListener.SurfaceEndpoint("/repo", "frame-detached")

        assertTrue(ownership.markPublished(detached, setOf("main-frame")).isEmpty())
        assertEquals(listOf(detached), ownership.drain())
    }

    @Test
    fun `a newer observation superseding an in-flight delivery keeps its endpoint live`() {
        // #tmuxautosyncretired: a tab switch while the previous delivery is in the controller
        // advances only the observation generation. Retiring the endpoint here made the
        // controller fence every later observation of that frame (rejected=retired_endpoint),
        // which silently disabled tab-switch auto-sync until a plugin reload or controller
        // recycle.
        assertEquals(
            EditorTabSyncListener.InFlightSurfaceDelivery.Decision.RecordEndpoint,
            EditorTabSyncListener.InFlightSurfaceDelivery.decide(
                requestedAuthorityEpoch = 4,
                currentAuthorityEpoch = 4,
            ),
        )
    }

    @Test
    fun `an authority handoff during an in-flight delivery retires that endpoint`() {
        assertEquals(
            EditorTabSyncListener.InFlightSurfaceDelivery.Decision.RetireEndpoint,
            EditorTabSyncListener.InFlightSurfaceDelivery.decide(
                requestedAuthorityEpoch = 4,
                currentAuthorityEpoch = 5,
            ),
        )
    }

    @Test
    fun `in-flight delivery retires its endpoint only through the authority gate`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val accepted =
            source
                .substringAfter("CpRouteClient.observeEditorSurface(")
                .substringBefore("retireClosedSurfaceEndpoints(observation.project, retiredEndpoints)")

        assertTrue(accepted.contains("InFlightSurfaceDelivery.decide("))
        assertFalse(
            "a superseding observation must not retire the endpoint it is about to republish",
            accepted.contains("generation.get() != requestedGeneration"),
        )
        val retire =
            source
                .substringAfter("internal fun retireControllerProjection()")
                .substringBefore("private fun shutdown(")
        assertTrue(
            "the handoff must advance the authority epoch under the drain lock",
            retire.contains("projectionAuthorityEpoch.incrementAndGet()\n            surfaceRoots.drain()"),
        )
    }

    @Test
    fun `a frontend handoff fences the listener's frames and the fallback makes them live again`() {
        // #surfaceretireformal (model: PassiveTmuxSyncRetireNet `live`): a retired-endpoint receipt
        // re-registers only while the listener owns its frames. After a handoff it must stay a
        // fence; once the backend selection fallback resumes, it must self-heal.
        val listener = EditorTabSyncListener()
        assertTrue("a classic listener is live from the start", listener.projectionLive())
        listener.retireControllerProjection()
        assertFalse("a handed-off listener must not re-register", listener.projectionLive())
        listener.resumeControllerProjection()
        assertTrue("the fallback owns the selection edge again", listener.projectionLive())
        listener.resumeControllerProjection()
        assertTrue("resuming a live listener is a no-op", listener.projectionLive())
    }

    @Test
    fun `both editor-surface publishers let a live listener re-register a retired endpoint`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        for (publisher in listOf("CpRouteClient.observeEditorSurface(", "CpRouteClient.observeEditorFocus(")) {
            val call = source.substringAfter(publisher).substringBefore(")")
            assertTrue(
                "$publisher must hand the transport the listener's liveness: $call",
                call.contains("mayReregister = ::projectionLive"),
            )
        }
        val live =
            source
                .substringAfter("internal fun projectionLive(): Boolean")
                .substringBefore("private val surfaceDeliveryExecutor")
        assertTrue(
            "liveness is read under the lock the handoff writes it under",
            live.contains("synchronized(lifecycleLock)") && live.contains("!closed && !projectionRetired"),
        )
        val retire =
            source
                .substringAfter("internal fun retireControllerProjection()")
                .substringBefore("internal fun resumeControllerProjection()")
        assertTrue(
            "the handoff marks the projection retired under the drain lock",
            retire.contains("projectionRetired = true\n            projectionAuthorityEpoch.incrementAndGet()"),
        )
    }

    @Test
    fun `surface capture delegates authoritative liveness to the shared adapter`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val capture =
            source
                .substringAfter("private fun captureSurface(")
                .substringBefore("private fun resolveSurface(")

        assertTrue(
            capture.contains(
                "JetBrainsEditorSurfaces.liveSurfaceIds(project, editorSurface.surfaceId)",
            ),
        )
        assertFalse(capture.contains("?.map { JetBrainsEditorSurfaces.forEditorWindow"))
    }

    @Test
    fun `surface projection waits while the selected document is absent`() {
        assertEquals(
            EditorTabSyncListener.SurfaceReport.ProjectionReadiness.AwaitingSelectedDocument,
            EditorTabSyncListener.SurfaceReport.projectionReadiness(
                preferredActiveFile = "/repo/left-next.md",
                visibleMdFiles = listOf("/repo/right.md"),
            ),
        )
    }

    @Test
    fun `surface projection becomes current when the selected document is visible`() {
        assertEquals(
            EditorTabSyncListener.SurfaceReport.ProjectionReadiness.Current,
            EditorTabSyncListener.SurfaceReport.projectionReadiness(
                preferredActiveFile = "/repo/left-next.md",
                visibleMdFiles = listOf("/repo/left-next.md", "/repo/right.md"),
            ),
        )
    }

    @Test
    fun `surface projection waits until every restored editor window selects a file`() {
        assertFalse(
            EditorTabSyncListener.SurfaceReport.restoredEditorWindowsReady(
                listOf("/repo/right.md", null),
            ),
        )
        assertEquals(
            EditorTabSyncListener.SurfaceReport.WindowSelectionReadiness
                .AwaitingBackendWindowSelection,
            EditorTabSyncListener.SurfaceReport.windowSelectionReadiness(
                selectedWindowFiles = listOf("/repo/right.md", null),
                useRemoteClientLayout = false,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SurfaceReport.restoredEditorWindowsReady(
                listOf("/repo/right.md", "/repo/left.md"),
            ),
        )
    }

    @Test
    fun `remote tab switch bypasses incomplete backend window and publishes two columns`() {
        val snapshot =
            LayoutDetector.RemoteLayoutSnapshot(
                projectRoot = "/repo",
                clients =
                    listOf(
                        LayoutDetector.RemoteClientSessionEditors(
                            visible = listOf("tasks/left.md", "tasks/right.md"),
                            selected = listOf("tasks/left.md"),
                            open = listOf("tasks/left.md", "tasks/right.md"),
                        ),
                    ),
                focusedSessionFiles = listOf("tasks/left.md"),
            )
        val useRemoteClientLayout =
            LayoutDetector.shouldUseRemoteClientLayout(
                backendWindowCount = 1,
                remoteClientSessionCount = snapshot.clients.size,
            )

        assertEquals(
            EditorTabSyncListener.SurfaceReport.WindowSelectionReadiness.Current,
            EditorTabSyncListener.SurfaceReport.windowSelectionReadiness(
                selectedWindowFiles = listOf(null),
                useRemoteClientLayout = useRemoteClientLayout,
            ),
        )

        val layout =
            LayoutDetector.detectEditorLayout(snapshot) { _, _ ->
                """{"columns":[{"files":["tasks/left.md"]},{"files":["tasks/right.md"]}],"source":"remote_client_visible_editors","reason":null}"""
            }
        val surface =
            EditorTabSyncListener.SurfaceReport.buildSurface(
                focusedFile = "tasks/left.md",
                visibleMdFiles = listOf("tasks/left.md", "tasks/right.md"),
                editorLayout = layout,
                forceReconcile = false,
            )

        assertEquals(2, surface.columns.size)
        assertFalse(surface.focusOnly)
    }

    @Test
    fun `every capture deferral logs an INFO reason`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val capture =
            source
                .substringAfter("private fun captureSurface(")
                .substringBefore("private fun resolveSurface(")

        // GH #326 phase 2: the EDT capture never defers; the derivation decides readiness on the
        // delivery worker, which logs every deferral with the same INFO line.
        assertTrue(capture.contains("LayoutDetector.snapshotRemoteLayout("))
        assertFalse(capture.contains("return null"))
        assertFalse(capture.contains("captureDeferred("))
        assertFalse(capture.contains("SurfaceReport.windowSelectionReadiness("))
        val worker =
            source
                .substringAfter("private fun deliverCapturedSurface(")
                .substringBefore("private fun retireClosedSurfaceEndpoints(")
        assertTrue(worker.contains("is SurfaceResolution.Deferred ->"))
        assertTrue(worker.contains("captureDeferred(resolution.reason)"))
        assertTrue(source.contains("LOG.info(\"[layout-sync] capture deferred reason=\$reason\")"))
        // The frozen floor keeps the GH #177 order: remote authority before backend readiness.
        val frozen =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/SurfaceProjection.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/SurfaceProjection.kt",
                    ),
            ).substringAfter("internal fun frozen(")
        assertTrue(
            frozen.indexOf("LayoutDetector.shouldUseRemoteClientLayout(") <
                frozen.indexOf("report.windowSelectionReadiness("),
        )
    }

    @Test
    fun `restored window selections remain authoritative while selected-files aggregate lags`() {
        val laggingSelectedFiles = listOf("/repo/nested/right.md")
        val restoredWindowFiles =
            listOf(
                "/repo/tasks/agent-doc/left.md",
                "/repo/nested/right.md",
            )

        assertEquals(1, laggingSelectedFiles.size)
        assertEquals(
            restoredWindowFiles,
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                restoredWindowFiles,
            ),
        )
    }

    @Test
    fun `a window switched to source keeps standing for its last agent-doc file`() {
        // #stickymdpane: two editor columns, the right one detoured to source.
        // The mirrored tmux layout must stay two panes with the last document
        // still selected, not collapse to one.
        assertEquals(
            listOf("/repo/tasks/left.md", "/repo/tasks/right.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/src/Thing.kt"),
                listOf("/repo/tasks/left.md", "/repo/tasks/right.md"),
            ),
        )
    }

    @Test
    fun `a window that never showed a document still contributes no pane`() {
        // Closing a split, or opening a fresh source-only one, must still
        // collapse the layout — the fallback is memory, not invention.
        assertEquals(
            listOf("/repo/tasks/left.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/src/Thing.kt"),
                listOf("/repo/tasks/left.md", null),
            ),
        )
        assertEquals(
            listOf("/repo/tasks/left.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/src/Thing.kt"),
                listOf("/repo/tasks/left.md", "/repo/src/Other.kt"),
            ),
        )
    }

    @Test
    fun `a live selection always beats the remembered document`() {
        assertEquals(
            listOf("/repo/tasks/left.md", "/repo/tasks/now.md"),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                listOf("/repo/tasks/left.md", "/repo/tasks/now.md"),
                listOf("/repo/tasks/left.md", "/repo/tasks/stale.md"),
            ),
        )
    }

    @Test
    fun `a selected non-session markdown plan falls back to the windows agent doc`() {
        val left = "/repo/tasks/left.md"
        val right = "/repo/tasks/backend.md"
        val plan = "/repo/docs/backend-fpe-contracts-sdk-pr-plan.md"

        assertEquals(
            listOf(left, right),
            EditorTabSyncListener.SurfaceReport.visibleMarkdownFilesFromRestoredWindows(
                selectedWindowFiles = listOf(left, plan),
                stickyWindowFallbacks = listOf(left, right),
                sessionDocumentPaths = setOf(left, right),
            ),
        )
    }

    @Test
    fun `stale selected-files projection is reread on later EDT turns`() {
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                remainingPasses = 1,
            ),
        )
        assertFalse(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                remainingPasses = 0,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.Layout,
                remainingPasses =
                    EditorTabSyncListener.SelectionProjectionSettling.MAX_REPROJECTION_PASSES,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                authority = EditorTabSyncListener.ObservationAuthority.FileOpened,
                remainingPasses =
                    EditorTabSyncListener.SelectionProjectionSettling.MAX_REPROJECTION_PASSES,
            ),
        )
    }

    @Test
    fun `exhausted selection settling substitutes the event edge into stale editor projections`() {
        val settled =
            EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                preferredFile = "/repo/left-next.md",
                previousFile = "/repo/left-old.md",
                visibleMdFiles = listOf("/repo/left-old.md", "/repo/right.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/left-old.md")),
                            LayoutColumn(listOf("/repo/right.md")),
                        ),
                    ),
            )

        assertEquals(
            listOf("/repo/left-next.md", "/repo/right.md"),
            settled.visibleMdFiles,
        )
        assertEquals(
            listOf(
                LayoutColumn(listOf("/repo/left-next.md")),
                LayoutColumn(listOf("/repo/right.md")),
            ),
            settled.editorLayout?.columns,
        )
    }

    @Test
    fun `exhausted selection settling repairs stale layout after selected files advance`() {
        assertEquals(
            EditorTabSyncListener.SurfaceReport.ProjectionReadiness.AwaitingSelectedDocument,
            EditorTabSyncListener.SurfaceReport.projectionReadiness(
                preferredActiveFile = "/repo/left-next.md",
                visibleMdFiles = listOf("/repo/left-next.md", "/repo/right.md"),
                layoutMdFiles = listOf("/repo/left-old.md", "/repo/right.md"),
            ),
        )

        val settled =
            EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                preferredFile = "/repo/left-next.md",
                previousFile = "/repo/left-old.md",
                visibleMdFiles = listOf("/repo/left-next.md", "/repo/right.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/left-old.md")),
                            LayoutColumn(listOf("/repo/right.md")),
                        ),
                    ),
            )

        assertEquals(
            listOf("/repo/left-next.md", "/repo/right.md"),
            settled.visibleMdFiles,
        )
        assertEquals(
            listOf(
                LayoutColumn(listOf("/repo/left-next.md")),
                LayoutColumn(listOf("/repo/right.md")),
            ),
            settled.editorLayout?.columns,
        )
    }

    @Test
    fun `selection settling never invents a replacement without the prior event file`() {
        val staleVisible = listOf("/repo/left-old.md", "/repo/right.md")
        val staleLayout =
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/left-old.md")),
                    LayoutColumn(listOf("/repo/right.md")),
                ),
            )

        val settled =
            EditorTabSyncListener.SelectionProjectionSettling.reconcileEventEdge(
                preferredFile = "/repo/left-next.md",
                previousFile = null,
                visibleMdFiles = staleVisible,
                editorLayout = staleLayout,
            )

        assertEquals(staleVisible, settled.visibleMdFiles)
        assertEquals(staleLayout, settled.editorLayout)
    }

    @Test
    fun `selected editor file is used when no selection event file is supplied`() {
        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = null,
                selectedEditorFile = "/repo/tasks/professional/sampleportal.md",
                visibleMdFiles = listOf("/repo/tasks/agent-doc/agent-doc-bugs2.md"),
            )

        assertEquals("/repo/tasks/professional/sampleportal.md", activeFile)
    }

    @Test
    fun `first visible markdown file is the last resort active file`() {
        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = "",
                selectedEditorFile = null,
                visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
            )

        assertEquals("/repo/a.md", activeFile)
    }

    @Test
    fun `no visible markdown means no active file to report`() {
        val activeFile =
            EditorTabSyncListener.SurfaceReport.resolveActiveFilePath(
                preferredActiveFile = null,
                selectedEditorFile = null,
                visibleMdFiles = emptyList(),
            )

        assertNull(activeFile)
    }

    @Test
    fun `observation reports the split layout the editor detected`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/b.md",
                visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/a.md")),
                            LayoutColumn(listOf("/repo/b.md")),
                        )
                    ),
            )

        val surface = JsonParser.parseString(json).asJsonObject
        assertEquals("/repo/b.md", surface.get("focused").asString)
        assertEquals(
            listOf("/repo/a.md", "/repo/b.md"),
            surface.getAsJsonArray("visible").map { it.asString },
        )
        assertEquals(
            listOf("/repo/b.md", "/repo/a.md"),
            surface.getAsJsonArray("open").map { it.asString },
        )
        val columns = surface.getAsJsonArray("columns")
        assertEquals(2, columns.size())
        assertEquals(
            listOf("/repo/a.md"),
            columns[0].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
        assertEquals(
            listOf("/repo/b.md"),
            columns[1].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
    }

    @Test
    fun `column order is preserved rather than sorted`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/b.md",
                visibleMdFiles = listOf("/repo/b.md", "/repo/a.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/b.md")),
                            LayoutColumn(listOf("/repo/a.md")),
                        )
                    ),
            )

        val columns = JsonParser.parseString(json).asJsonObject.getAsJsonArray("columns")
        assertEquals(
            listOf("/repo/b.md"),
            columns[0].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
    }

    @Test
    fun `an undetected layout reports no columns rather than a synthesized one`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/a.md",
                visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
                editorLayout = null,
            )

        val surface = JsonParser.parseString(json).asJsonObject
        assertEquals(0, surface.getAsJsonArray("columns").size())
        assertEquals(2, surface.getAsJsonArray("visible").size())
    }

    @Test
    fun `blank and duplicate layout entries are dropped from the observation`() {
        val json =
            surfaceJson(
                focusedFile = "/repo/a.md",
                visibleMdFiles = listOf("/repo/a.md", "/repo/a.md"),
                editorLayout =
                    EditorLayout(
                        listOf(
                            LayoutColumn(listOf("/repo/a.md", "", "/repo/a.md")),
                            LayoutColumn(listOf("", "")),
                        )
                    ),
            )

        val surface = JsonParser.parseString(json).asJsonObject
        assertEquals(listOf("/repo/a.md"), surface.getAsJsonArray("visible").map { it.asString })
        val columns = surface.getAsJsonArray("columns")
        assertEquals(1, columns.size())
        assertEquals(
            listOf("/repo/a.md"),
            columns[0].asJsonObject.getAsJsonArray("files").map { it.asString },
        )
    }

    @Test
    fun `force reconcile crosses the wire in the shape the graph reads`() {
        val forced =
            JsonParser.parseString(
                    surfaceJson("/repo/a.md", listOf("/repo/a.md"), forceReconcile = true)
                )
                .asJsonObject
        val unforced =
            JsonParser.parseString(
                    surfaceJson("/repo/a.md", listOf("/repo/a.md"), forceReconcile = false)
                )
                .asJsonObject

        assertTrue(forced.get("force_reconcile").asBoolean)
        assertFalse(unforced.get("force_reconcile").asBoolean)
    }

    @Test
    fun `passive layout observation preserves explicit pane focus`() {
        val passive =
            JsonParser.parseString(
                    surfaceJson(
                        "/repo/tasks/left.md",
                        listOf("/repo/tasks/left.md", "/repo/tasks/right.md"),
                        preserveFocus = true,
                    )
                )
                .asJsonObject

        assertTrue(passive.get("preserve_focus").asBoolean)
        assertFalse(passive.get("focus_only").asBoolean)
    }

    @Test
    fun `observation reports no layout_synced field for the controller to answer`() {
        val surface =
            JsonParser.parseString(surfaceJson("/repo/a.md", listOf("/repo/a.md"))).asJsonObject

        assertFalse(surface.has("layout_synced"))
        assertFalse(surface.has("layoutSynced"))
    }

    @Test
fun `document selection publishes focus only when its split is active`() {
val source =
Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    )
            )
        val selection =
            source
                .substringAfter("override fun selectionChanged(event: FileEditorManagerEvent)")
.substringBefore("fun onEditorFocusGained")
assertTrue(selection.contains("requestObservation("))
assertTrue(selection.contains("requestFocusProjection("))
assertTrue(selection.contains("remoteControllerSelectionFocusEvidence ="))
assertTrue(selection.contains("requiredFocusGeneration = requestedFocusGeneration"))
assertTrue(selection.contains("FileEditorManagerEx.getInstanceEx(project).currentWindow"))
assertTrue(selection.contains("SelectionFocusAuthority.decide("))
assertTrue(selection.contains("IdeFocusManager.getInstance(project).doWhenFocusSettlesDown"))
assertTrue(selection.contains("preferredFile = file.takeIf { selectionOwnsFocus }"))
assertFalse(selection.contains("manager.selectedFiles"))
assertFalse(selection.contains("collectVisibleMarkdownFiles"))
        assertFalse(selection.contains("CpRouteClient"))
        assertTrue(selection.contains("forceReconcile = false"))
        assertTrue(selection.contains("previousFile = event.oldFile"))
        assertFalse(
            "ordinary tab focus must not force a competing full layout reconcile",
            selection.contains("forceReconcile = true"),
        )

        val focusGained =
            source
                .substringAfter("fun onEditorFocusGained(project: Project, file: VirtualFile)")
                .substringBefore("fun onEditorLayoutChanged")
        assertTrue(focusGained.contains("requestFocusProjection(project, file)"))
        assertTrue(focusGained.contains("requestObservation("))
        assertTrue(focusGained.contains("preferredFile = file"))
        assertTrue(focusGained.contains("forceReconcile = false"))
assertTrue(focusGained.contains("authority = ObservationAuthority.EditorFocus"))
        assertTrue(source.contains("authority == ObservationAuthority.Layout"))
        assertTrue(source.contains("authority == ObservationAuthority.FileOpened"))
        assertTrue(source.contains("authority == ObservationAuthority.IdeActivation"))
        assertTrue(source.contains("preserveFocus = observation.preserveFocus"))
        assertFalse(focusGained.contains("delayMs"))

        val focusProjection =
            source
                .substringAfter(
                    "private fun requestFocusProjection(project: Project, file: VirtualFile): Long?",
                )
                .substringBefore("private fun shutdown()")
        assertTrue(focusProjection.contains("TerminalUtil.resolveProject(project, file)"))
        assertTrue(focusProjection.contains("CpRouteClient.observeEditorFocus("))
        assertFalse(focusProjection.contains("submitFocusDocumentPane("))
        assertTrue(
            focusProjection.indexOf("decideFocusProjectionReceipt(") <
                focusProjection.indexOf("TmuxPaneFocusSync.recordEditorFocusIntent("),
        )
        assertTrue(focusProjection.contains("FocusProjectionReceiptDecision.RepairLayout"))
        assertTrue(focusProjection.contains("requiredFocusGeneration = requestedGeneration"))
        assertTrue(focusProjection.contains("forceReconcile = true"))
        assertTrue(focusProjection.contains("authority = ObservationAuthority.EditorFocus"))
        assertTrue(focusProjection.contains("catch (error: Throwable)"))
        assertTrue(focusProjection.contains("focus projection gated gate="))
        assertTrue(focusProjection.contains("focus projection completed outcome="))
    }

@Test
fun `selection focus authority rejects background split events`() {
assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.ActiveEditorSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/fpe.md",
activeWindowPath = "/repo/tasks/fpe.md",
),
)
assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.BackgroundOrUnknownSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/left.md",
activeWindowPath = "/repo/tasks/fpe.md",
),
)
        // GH #88 / #102: a Remote Dev backend can enumerate editor windows while currentWindow
        // remains null. The selection itself must own focus regardless of the window count.
        assertEquals(
            EditorTabSyncListener.SelectionFocusAuthority.ActiveEditorSplit,
            EditorTabSyncListener.SelectionFocusAuthority.decide(
                selectionPath = "/repo/tasks/left.md",
                activeWindowPath = null,
            ),
        )
        // GH #88: with no editor windows the per-split selections come from selectedFiles.
assertEquals(
listOf<String?>("/repo/a.md", "/repo/b.md"),
EditorTabSyncListener.SurfaceReport.splitSelections(emptyList(), listOf("/repo/a.md", "/repo/b.md")),
)
assertEquals(
listOf<String?>("/repo/a.md"),
EditorTabSyncListener.SurfaceReport.splitSelections(listOf("/repo/a.md"), listOf("/repo/a.md", "/repo/b.md")),
)
        // IDEA may emit selectionChanged before currentWindow advances from old to new. The event still
// belongs to the active split when the active window reports the event's previous selection.
assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.ActiveEditorSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/agent-doc-bugs.md",
activeWindowPath = "/repo/tasks/tsift.md",
previousSelectionPath = "/repo/tasks/tsift.md",
),
)

assertEquals(
EditorTabSyncListener.SelectionFocusAuthority.BackgroundOrUnknownSplit,
EditorTabSyncListener.SelectionFocusAuthority.decide(
selectionPath = "/repo/tasks/agent-doc-bugs.md",
activeWindowPath = "/repo/tasks/contracts.md",
previousSelectionPath = "/repo/tasks/tsift.md",
),
)
}

    /**
     * GH #320: the issue's Remote Dev trace. The backend's own selection names no session
     * document (`activeWindow=<none>`), but the remote client reports `visible=[b,a]`
     * `selected=[b]`. The capture must keep that state instead of deferring with
     * `no_visible_session_documents`.
     */
    @Test
    fun `remote dev capture keeps the remote client state when the backend selection is empty`() {
        val absolute =
            mapOf(
                "tasks/a.md" to "/repo/tasks/a.md",
                "tasks/b.md" to "/repo/tasks/b.md",
            )
        assertEquals(
            listOf("/repo/tasks/b.md", "/repo/tasks/a.md"),
            EditorTabSyncListener.SurfaceReport.remoteClientVisibleSessionFiles(
                backendVisible = emptyList(),
                clientVisible = listOf(listOf("tasks/b.md", "tasks/a.md")),
                clientSelected = listOf(listOf("tasks/b.md")),
                absoluteByRelative = absolute,
            ),
        )
        // The backend's own selection still counts, and a path that is not an open session
        // document is never invented into the surface.
        assertEquals(
            listOf("/repo/tasks/a.md", "/repo/tasks/b.md"),
            EditorTabSyncListener.SurfaceReport.remoteClientVisibleSessionFiles(
                backendVisible = listOf("/repo/tasks/a.md"),
                clientVisible = listOf(listOf("tasks/b.md", "tasks/gone.md")),
                clientSelected = emptyList(),
                absoluteByRelative = absolute,
            ),
        )
    }

    /** GH #320: the surface state names its column order provenance on the wire. */
    @Test
    fun `surface payload carries the layout column order provenance`() {
        val columns = listOf(LayoutColumn(listOf("/repo/a.md")), LayoutColumn(listOf("/repo/b.md")))
        val remote =
            JsonParser.parseString(
                surfaceJson(
                    focusedFile = "/repo/a.md",
                    visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
                    editorLayout = EditorLayout(columns, "unknown"),
                ),
            ).asJsonObject
        assertEquals("unknown", remote.get("column_order").asString)
        val local =
            JsonParser.parseString(
                surfaceJson(
                    focusedFile = "/repo/a.md",
                    visibleMdFiles = listOf("/repo/a.md", "/repo/b.md"),
                    editorLayout = EditorLayout(columns),
                ),
            ).asJsonObject
        assertEquals("editor", local.get("column_order").asString)
    }

    @Test
    fun `editor snapshot leaves project root and controller work off the EDT`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val capture =
            source
                .substringAfter("private fun captureSurface(")
                .substringBefore("private fun resolveSurface(")
        val resolution =
            source
                .substringAfter("private fun resolveSurface(")
                .substringBefore("private fun requestFocusProjection(")

        assertFalse(capture.contains("TerminalUtil.resolveProject("))
        assertFalse(capture.contains("nearestAgentDocProjectRoot"))
        assertTrue(resolution.contains("TerminalUtil.resolveProject("))
        assertTrue(resolution.contains("nearestAgentDocProjectRoot"))
        assertTrue(source.contains("surfaceDeliveryExecutor.execute"))
        assertTrue(source.contains("resolveSurface(captured)"))
    }

    @Test
    fun `IDE activation republishes and settles the visible editor surface`() {
        assertTrue(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                EditorTabSyncListener.ObservationAuthority.IdeActivation,
                remainingPasses = 1,
            ),
        )
        assertFalse(
            EditorTabSyncListener.SelectionProjectionSettling.shouldReproject(
                EditorTabSyncListener.ObservationAuthority.IdeActivation,
                remainingPasses = 0,
            ),
        )
    }

    @Test
    fun `markdown file open republishes the editor surface after the startup seed`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt",
                    ),
            )
        val fileOpenedMarker =
            "override fun fileOpened(source: FileEditorManager, file: VirtualFile)"

        assertTrue(source.contains(fileOpenedMarker))
        val fileOpened =
            source
                .substringAfter(fileOpenedMarker)
                .substringBefore("override fun fileClosed")
        assertTrue(fileOpened.contains("if (!AgentDocSessionFiles.isSessionDocument(file))"))
        assertTrue(fileOpened.contains("authority = ObservationAuthority.Layout"))
        assertTrue(fileOpened.contains("preferredFile = file"))
        assertTrue(fileOpened.contains("authority = ObservationAuthority.FileOpened"))
        assertFalse(fileOpened.contains("onEditorLayoutChanged(source.project)"))
    }

    @Test
    fun `focused and adjacent tabs lead the open document priority`() {
        assertEquals(
            listOf(
                "/repo/b.md",
                "/repo/a.md",
                "/repo/c.md",
                "/repo/d.md",
                "/repo/split.md",
            ),
            EditorTabSyncListener.SurfaceReport.prioritizeOpenDocuments(
                focusedFile = "/repo/b.md",
                nearbyTabs =
                    EditorTabSyncListener.SurfaceReport.tabsByProximity(
                        focusedFile = "/repo/b.md",
                        tabs =
                            listOf(
                                "/repo/a.md",
                                "/repo/b.md",
                                "/repo/c.md",
                                "/repo/d.md",
                            ),
                    ),
                visibleMdFiles = listOf("/repo/b.md", "/repo/split.md"),
                openMdFiles = listOf("/repo/d.md", "/repo/split.md"),
            ),
        )
    }

    @Test
    fun `structural layout detection uses one short event coalescing window`() {
        assertTrue(LayoutChangeDetector.STRUCTURAL_COALESCE_MS in 1L..75L)
    }

    @Test
    fun `layout changes retain selected intent only until the surface is captured`() {
        val listenerPath =
            listOf(
                    Paths.get(
                        "src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                )
                .first { Files.exists(it) }
        val source = Files.readString(listenerPath)
        val layoutChange =
            source
                .substringAfter("fun onEditorLayoutChanged(project: Project)")
                .substringBefore("override fun fileClosed")
        val report =
            source
                .substringAfter(
                    "private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long)"
                )
                .substringBefore("private fun captureSurface(")

        assertTrue(layoutChange.contains("latestSurfaceObservation.get()"))
        assertTrue(layoutChange.contains("it.preferredFile != null"))
        assertFalse(layoutChange.contains("delayMs"))
        assertTrue(report.contains("GenerationWork.invokeLater"))
        assertTrue(report.contains("SelectionProjectionSettling.shouldReproject"))
        assertTrue(report.contains("remainingSelectionPasses - 1"))
        assertTrue(report.contains("ObservationDeliveryOwnership.releaseForDelivery"))
        assertTrue(report.contains("ObservationDeliveryOwnership.retainAfterFailure"))
        assertFalse(report.contains("requestObservation("))
        assertFalse(report.contains("invokeAndWait"))
        assertFalse(report.contains("schedule("))
    }

    @Test
    fun `surface reporting uses the existing controller socket off the EDT`() {
        val listenerPath =
            listOf(
                    Paths.get(
                        "src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                )
                .first { Files.exists(it) }
        val reportBody =
            Files.readString(listenerPath)
                .substringAfter(
                    "private fun projectLatestSurfaceOnEditorThread(requestedGeneration: Long)"
                )
                .substringBefore("private fun captureSurface(")

        assertTrue(reportBody.contains("CpRouteClient.observeEditorSurface("))
        assertTrue(reportBody.contains("surfaceDeliveryExecutor.execute"))
        assertTrue(reportBody.contains("synchronized(lifecycleLock)"))
        assertTrue(reportBody.contains("if (closed)"))
        assertTrue(reportBody.contains("surfaceRoots.recordAttempts("))
        assertTrue(reportBody.contains("surfaceRoots.markPublished("))
        assertTrue(reportBody.contains("CpRouteClient.forgetEditorSurface("))
        assertTrue(reportBody.contains("surfaceRoots.markForgotten("))
        assertFalse(reportBody.contains("requestObservation("))
        assertFalse(reportBody.contains("NativeAdminControls.editorSurface"))
        assertFalse(reportBody.contains("editorSurfaceObserve("))
        assertFalse(reportBody.contains("syncHintFromReceipt("))
    }

    @Test
    fun `component focus republishes spanning surface and selection state`() {
        val listenerPath =
            listOf(
                    Paths.get(
                        "src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                    Paths.get(
                        "editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"
                    ),
                )
                .first { Files.exists(it) }
        val focusBody =
            Files.readString(listenerPath)
                .substringAfter("fun onEditorFocusGained(project: Project, file: VirtualFile)")
                .substringBefore("fun onEditorLayoutChanged(project: Project)")

        assertTrue(focusBody.contains("requestFocusProjection(project, file)"))
        assertTrue(focusBody.contains("requestObservation("))
        assertTrue(focusBody.contains("preferredFile = file"))
        assertTrue(focusBody.contains("authority = ObservationAuthority.EditorFocus"))
    }

    @Test
    fun `component focus cannot supersede a pending document selection`() {
        assertFalse(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
            ),
        )
        assertFalse(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
            ),
        )
    }

    @Test
    fun `component focus publishes when no document selection is pending`() {
        assertTrue(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = null,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
            ),
        )
        assertTrue(
            EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                currentAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
                incomingAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
            ),
        )
    }

    @Test
    fun `passive surface cannot supersede pending explicit focus`() {
        for (
            passive in
                listOf(
                    EditorTabSyncListener.ObservationAuthority.Layout,
                    EditorTabSyncListener.ObservationAuthority.FileOpened,
                    EditorTabSyncListener.ObservationAuthority.IdeActivation,
                )
        ) {
            assertFalse(
                EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                    currentAuthority = EditorTabSyncListener.ObservationAuthority.EditorFocus,
                    incomingAuthority = passive,
                ),
            )
            assertFalse(
                EditorTabSyncListener.SurfaceObservationOrdering.shouldReplace(
                    currentAuthority = EditorTabSyncListener.ObservationAuthority.DocumentSelection,
                    incomingAuthority = passive,
                ),
            )
        }
    }

    @Test
    fun `IDE activation promotes the selected session document to explicit focus`() {
        val sourcePath = listOf(
            Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"),
            Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"),
        ).first { Files.exists(it) }
        val source = Files.readString(sourcePath)
        val activation =
            source.substringAfter("fun onIdeActivated(project: Project)")
                .substringBefore("fun onEditorFocusGained")

        assertTrue(activation.contains("doWhenFocusSettlesDown"))
        assertTrue(activation.contains("activationGeneration"))
        assertTrue(activation.contains("selectionFocusProbeGeneration.get() != activationGeneration"))
        assertTrue(activation.contains("currentWindow?.selectedFile"))
        assertTrue(activation.contains("JetBrainsEditorSurfaces.isProjectSurfaceActive(project)"))
        assertTrue(activation.contains("AgentDocSessionFiles.isSessionDocument(selectedFile)"))
        assertTrue(activation.contains("onEditorFocusGained(project, selectedFile)"))
    }

    @Test
    fun `GH 326 a worker-resolved local layout that omits the preferred document is withheld`() {
        val layout =
            EditorLayout(
                listOf(
                    LayoutColumn(listOf("/repo/left-old.md")),
                    LayoutColumn(listOf("/repo/right.md")),
                ),
            )
        assertNull(
            EditorTabSyncListener.SurfaceReport.layoutForPreferredDocument(
                preferredActiveFile = "/repo/left-next.md",
                editorLayout = layout,
            ),
        )
        assertEquals(
            layout,
            EditorTabSyncListener.SurfaceReport.layoutForPreferredDocument(
                preferredActiveFile = "/repo/right.md",
                editorLayout = layout,
            ),
        )
        assertEquals(
            layout,
            EditorTabSyncListener.SurfaceReport.layoutForPreferredDocument(
                preferredActiveFile = null,
                editorLayout = layout,
            ),
        )
    }

    @Test
    fun `GH 326 the EDT capture reads window evidence and resolves no layout`() {
        val source =
            Files.readString(Paths.get("src/main/kotlin/com/github/btakita/agentdoc/EditorTabSyncListener.kt"))
        val capture = source.substringAfter("private fun captureSurface(").substringBefore("private fun resolveSurface(")
        assertTrue(capture.contains("LayoutDetector.captureEditorWindows("))
        for (rule in listOf(
            "LayoutDetector.detectEditorLayout(",
            "SurfaceReport.",
            "SelectionProjectionSettling.reconcileEventEdge(",
            "SurfaceProjection.derive(",
        )) {
            assertFalse("GH #326 phase 2: the EDT capture must not run $rule", capture.contains(rule))
        }
        val resolve = source.substringAfter("private fun resolveSurface(").substringBefore("internal class CommandSurfaceCapture")
        assertTrue(resolve.contains("SurfaceProjection.derive(captured.evidence)"))
        assertFalse(resolve.contains("SurfaceReport.buildSurface("))
    }
}
