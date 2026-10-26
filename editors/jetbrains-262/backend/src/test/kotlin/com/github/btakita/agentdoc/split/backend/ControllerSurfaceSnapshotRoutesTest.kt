package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.EditorWindowSnapshot
import com.github.btakita.agentdoc.split.FrontendSurface
import com.github.btakita.agentdoc.split.FrontendSurfaceSnapshot
import com.github.btakita.agentdoc.split.SurfaceRole
import com.intellij.platform.project.ProjectId
import fleet.util.UID
import kotlin.test.Test
import kotlin.test.assertEquals

class ControllerSurfaceSnapshotRoutesTest {
    @Test
    fun controllerRoutesComeFromEverySessionDocumentRootInsteadOfTheIdeBasePath() {
        val rootA = "/workspace/alpha"
        val rootB = "/workspace/beta"
        val plainMarkdown = "/ide/project/README.md"
        val sessionA = "$rootA/tasks/alpha.md"
        val sessionB = "$rootB/tasks/beta.md"
        val roots = mapOf(sessionA to rootA, sessionB to rootB)

        val resolved = ControllerSurfaceSnapshotRoutes.resolve(
            snapshot(sessionA, sessionB, plainMarkdown),
            isSessionDocument = roots::containsKey,
            controllerRoot = roots::get,
        )

        assertEquals(linkedSetOf(rootA, rootB), resolved)
    }

    private fun snapshot(vararg paths: String): FrontendSurfaceSnapshot = FrontendSurfaceSnapshot(
        frontendInstanceId = "frontend-1",
        sequence = 1,
        projectId = ProjectId(UID.random()),
        complete = true,
        surfaces = listOf(
            FrontendSurface(
                paneId = "root",
                surfaceGeneration = 1,
                role = SurfaceRole.MAIN,
                focused = true,
                windows = listOf(
                    EditorWindowSnapshot(
                        ordinal = 0,
                        selectedPath = paths.firstOrNull(),
                        openPaths = paths.toList(),
                        visiblePaths = paths.toList(),
                    ),
                ),
            ),
        ),
    )
}
