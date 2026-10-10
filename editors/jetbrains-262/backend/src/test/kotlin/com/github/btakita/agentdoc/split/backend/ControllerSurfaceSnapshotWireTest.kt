package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendPresentationKind
import com.github.btakita.agentdoc.split.FrontendPresentationReceipt
import com.github.btakita.agentdoc.split.FrontendPresentationReceiptOutcome
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceIdentity
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceRole
import com.google.gson.JsonParser
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse

class ControllerSurfaceSnapshotWireTest {
    @Test
    fun oneAtomicRequestPreservesEveryTaggedSurfaceIncludingEmptyMain() {
        val snapshot = FrontendSurfaceSnapshot(
            frontendInstanceId = "frontend-1",
            sequence = 9,
            projectId = ProjectId(UID.random()),
            complete = true,
            surfaces = listOf(
                FrontendSurface("root", 3, SurfaceRole.MAIN, false, emptyList()),
                FrontendSurface(
                    "detached-1",
                    4,
                    SurfaceRole.DETACHED,
                    false,
                    listOf(
                        EditorWindowSnapshot(
                            ordinal = 0,
                            selectedPath = "/repo/session.md",
                            openPaths = listOf("/repo/session.md", "/repo/not-a-session.txt"),
                            visiblePaths = listOf("/repo/session.md"),
                        ),
                    ),
                ),
            ),
        )

        val request = ControllerSurfaceSnapshotWire.snapshotRequest(
            "jetbrains-rd:authenticated-client",
            7,
            snapshot,
        ) { it.endsWith(".md") }
        val payload = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        val surfaces = payload.getAsJsonArray("surfaces")

        assertEquals("editor_view_snapshot_observe", request.get("command").asString)
        assertFalse(request.toString().contains("\"command\":\"editor_surface_observe\""))
        assertFalse(payload.get("terminal_capable").asBoolean)
        assertEquals(2, surfaces.size())
        assertEquals("root", surfaces[0].asJsonObject.get("surface_id").asString)
        assertEquals(0, surfaces[0].asJsonObject.getAsJsonArray("visible").size())
        assertEquals(4, surfaces[1].asJsonObject.get("surface_generation").asLong)
        assertEquals("/repo/session.md", surfaces[1].asJsonObject.get("focused").asString)
        assertEquals(
            1,
            surfaces[1].asJsonObject.getAsJsonArray("visible").size(),
        )
    }

    @Test
    fun awtFocusChangeDoesNotClearDetachedSelectedDocument() {
        fun focusedValue(awtFocused: Boolean): String {
            val snapshot = FrontendSurfaceSnapshot(
                frontendInstanceId = "frontend-1",
                sequence = 9,
                projectId = ProjectId(UID.random()),
                complete = true,
                surfaces = listOf(
                    FrontendSurface("root", 1, SurfaceRole.MAIN, !awtFocused, emptyList()),
                    FrontendSurface(
                        "detached-1",
                        2,
                        SurfaceRole.DETACHED,
                        awtFocused,
                        listOf(EditorWindowSnapshot(0, "/repo/session.md", listOf("/repo/session.md"), listOf("/repo/session.md"))),
                    ),
                ),
            )
            val request = ControllerSurfaceSnapshotWire.snapshotRequest("jetbrains-rd:client", 7, snapshot) {
                it.endsWith(".md")
            }
            val payload = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
            return payload.getAsJsonArray("surfaces")[1].asJsonObject.get("focused").asString
        }

        assertEquals("/repo/session.md", focusedValue(true))
        assertEquals("/repo/session.md", focusedValue(false))
    }

    @Test
    fun explicitProjectionListAndReceiptPreserveIdentityRevisionAndBoundSession() {
        val projection = ControllerSurfaceSnapshotWire.presentationProjection(
            JsonParser.parseString(
                """{"status":"applied","presentation_revision":12,"retry_suggested":false,"presentations":[{"client_id":"jetbrains-rd:client","connection_generation":7,"surface_id":"detached-1","surface_generation":4,"presentation_revision":12,"kind":"terminal","document":"/repo/session.md","reason":null,"view_session":"agent-doc-view-bound"}]}""",
            ).asJsonObject,
        )
        val presentation = projection.presentations.single()
        assertEquals(12, projection.revision)
        assertEquals(FrontendPresentationKind.TERMINAL, presentation.kind)
        assertEquals("agent-doc-view-bound", presentation.viewSession)

        val request = ControllerSurfaceSnapshotWire.presentationReceiptRequest(
            "jetbrains-rd:client",
            7,
            FrontendPresentationReceipt(
                projectId = ProjectId(UID.random()),
                identity = FrontendSurfaceIdentity("jetbrains-rd:client", 7, "detached-1", 4),
                presentationRevision = 12,
                kind = FrontendPresentationKind.TERMINAL,
                document = "/repo/session.md",
                viewSession = "agent-doc-view-bound",
                outcome = FrontendPresentationReceiptOutcome.APPLIED,
            ),
        )
        val payload = JsonParser.parseString(request.get("diagnostic_payload").asString).asJsonObject
        assertEquals("editor_view_presentation_receipt", request.get("command").asString)
        assertEquals(12, request.get("sequence").asLong)
        assertEquals("agent-doc-view-bound", payload.get("view_session").asString)
        assertEquals(4, payload.get("surface_generation").asLong)
    }
}
