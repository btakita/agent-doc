package com.github.btakita.agentdoc.split.frontend

import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity

class SurfaceSnapshotFrontendActivity : ProjectActivity {
    override suspend fun execute(project: Project) {
        FrontendSurfaceSnapshotService.getInstance(project).start()
    }
}
