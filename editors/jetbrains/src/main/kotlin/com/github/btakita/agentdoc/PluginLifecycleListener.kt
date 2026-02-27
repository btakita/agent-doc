package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManagerListener

/**
 * Disposes per-project resources (PromptPoller, PromptPanel) when a project closes
 * or when the plugin is dynamically unloaded.
 *
 * Registered in plugin.xml as a projectListener so IntelliJ manages the lifecycle.
 * This enables `require-restart="false"` (dynamic plugin install/update/unload).
 */
class PluginLifecycleListener : ProjectManagerListener {
    override fun projectClosed(project: Project) {
        PromptPanel.dismiss(project)
        PromptPoller.disposeProject(project)
    }
}
