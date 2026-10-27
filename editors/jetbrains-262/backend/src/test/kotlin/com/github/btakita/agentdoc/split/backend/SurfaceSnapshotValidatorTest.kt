package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceIngressStatus
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class SurfaceSnapshotValidatorTest {
    private val projectId = ProjectId(UID.random())

    @Test
    fun acceptsRootAndDetachedWithDuplicateDocumentBeforeDedupe() {
        val duplicate = window("/repo/session.md")
        assertNull(
            SurfaceSnapshotValidator.validate(
                snapshot(
                    surface("root", SurfaceRole.MAIN, duplicate),
                    surface("dock-window-1", SurfaceRole.DETACHED, duplicate, focused = true),
                ),
            ),
        )
    }

    @Test
    fun failsClosedWithoutExactlyOneRootMain() {
        val failure = SurfaceSnapshotValidator.validate(
            snapshot(surface("dock-window-1", SurfaceRole.DETACHED, window("/repo/a.md"))),
        )
        assertEquals(SurfaceIngressStatus.INVALID_MAIN_CARDINALITY, failure?.status)
    }

    @Test
    fun failsClosedForIncompleteSnapshot() {
        val failure = SurfaceSnapshotValidator.validate(
            snapshot(surface("root", SurfaceRole.MAIN, window("/repo/a.md")), complete = false),
        )
        assertEquals(SurfaceIngressStatus.INCOMPLETE, failure?.status)
    }

    /** Older schemas remain accepted; an unknown future schema fails closed. */
    @Test
    fun acceptsLegacySchemaOneAndRejectsAnUnknownSchema() {
        val main = surface("root", SurfaceRole.MAIN, window("/repo/a.md"))
        assertNull(SurfaceSnapshotValidator.validate(snapshot(main).copy(schemaVersion = 1)))
        assertNull(SurfaceSnapshotValidator.validate(snapshot(main).copy(schemaVersion = 2)))
        assertNull(SurfaceSnapshotValidator.validate(snapshot(main).copy(schemaVersion = 3)))
        assertNull(SurfaceSnapshotValidator.validate(snapshot(main).copy(schemaVersion = 4)))
        assertNull(SurfaceSnapshotValidator.validate(snapshot(main).copy(schemaVersion = 5)))
        assertEquals(
            SurfaceIngressStatus.INVALID_SCHEMA,
            SurfaceSnapshotValidator.validate(snapshot(main).copy(schemaVersion = 6))?.status,
        )
    }

    @Test
    fun focusedEditorMustBeSelectedInTheActiveSurface() {
        val active =
            surface("root", SurfaceRole.MAIN, window("/repo/a.md"), focused = true)
                .copy(focusedEditorPath = "/repo/b.md")
        assertEquals(
            SurfaceIngressStatus.INVALID_SURFACE,
            SurfaceSnapshotValidator.validate(snapshot(active))?.status,
        )
        assertNull(
            SurfaceSnapshotValidator.validate(
                snapshot(active.copy(focusedEditorPath = "/repo/a.md")),
            ),
        )
    }

    private fun snapshot(vararg surfaces: FrontendSurface, complete: Boolean = true) =
        FrontendSurfaceSnapshot(
            frontendInstanceId = "frontend-1",
            sequence = 1,
            projectId = projectId,
            complete = complete,
            surfaces = surfaces.toList(),
        )

    private fun window(path: String) = EditorWindowSnapshot(0, path, listOf(path), listOf(path))

    private fun surface(
        paneId: String,
        role: SurfaceRole,
        window: EditorWindowSnapshot,
        focused: Boolean = false,
    ) = FrontendSurface(paneId, 1, role, focused, listOf(window))
}
