package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
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
                    true,
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
}
