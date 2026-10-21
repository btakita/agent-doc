package com.github.btakita.agentdoc

import com.intellij.openapi.project.Project
import java.util.concurrent.ConcurrentHashMap

/**
 * GH #320 option B: the projects whose Remote Dev main-window layout is owned by the JetBrains
 * Client's measured geometry.
 *
 * The modular 262/263 backend forwards the client's measured editor-window columns into the
 * controller surface Source labelled `column_order=editor` and registers itself here while that
 * forward is live. While a project is registered, [EditorTabSyncListener]'s Remote Dev capture
 * (flat focus/liveness evidence, labelled `unknown`) does not publish a structural layout surface,
 * so backend membership order never competes with measured order. One-sided and pre-schema-2
 * installs remain structurally unknown. Nothing in a classic (242-261) build ever registers.
 */
internal object RemoteClientLayoutAuthority {
    private val owners = ConcurrentHashMap<Project, MutableSet<String>>()

    fun activate(project: Project, owner: String) {
        owners.computeIfAbsent(project) { ConcurrentHashMap.newKeySet() }.add(owner)
    }

    fun release(project: Project, owner: String) {
        owners.computeIfPresent(project) { _, current ->
            current.remove(owner)
            current.takeIf { it.isNotEmpty() }
        }
    }

    fun isActive(project: Project): Boolean = owners[project]?.isNotEmpty() == true
}
