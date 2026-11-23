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

    /**
     * GH #435: each owning endpoint's last publication state on the frontend -> backend ->
     * controller channel, so a route or sync that defers to the measured layout can name the hop
     * that has not delivered it (`awaiting_first_snapshot`, `snapshot_refused`, `forwarded`, ...).
     */
    private val states = ConcurrentHashMap<String, String>()

    fun activate(project: Project, owner: String) {
        owners.computeIfAbsent(project) { ConcurrentHashMap.newKeySet() }.add(owner)
        states[owner] = STATE_AWAITING_FIRST_SNAPSHOT
    }

    fun release(project: Project, owner: String) {
        owners.computeIfPresent(project) { _, current ->
            current.remove(owner)
            current.takeIf { it.isNotEmpty() }
        }
        states.remove(owner)
    }

    fun isActive(project: Project): Boolean = owners[project]?.isNotEmpty() == true

    /** Record [owner]'s publication [state]; ignored once the owner released its authority. */
    fun note(project: Project, owner: String, state: String) {
        if (owners[project]?.contains(owner) == true) states[owner] = state
    }

    /**
     * The `frontend_layout=[...]` diagnostic token for [project]'s owners, or null when no
     * measured forward owns its layout. One `owner:state` entry per endpoint, sorted.
     */
    fun describe(project: Project): String? {
        val current = owners[project]?.sorted()?.takeIf { it.isNotEmpty() } ?: return null
        return current.joinToString(",", "frontend_layout=[", "]") { owner ->
            "$owner:${states[owner] ?: STATE_AWAITING_FIRST_SNAPSHOT}"
        }
    }

    const val STATE_AWAITING_FIRST_SNAPSHOT = "awaiting_first_snapshot"
}
