package com.github.btakita.agentdoc

import com.intellij.openapi.editor.Editor
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.ToolWindowManager
import java.awt.Window

/**
 * Optional platform capabilities that have no supported IntelliJ API.
 *
 * Marketplace artifacts must not link private platform symbols. Keep the capability lookup here,
 * erase private types at the bytecode boundary, and let callers degrade when a build does not
 * expose the capability.
 */
internal object MarketplacePlatformBridge {
    private const val CLIENT_MANAGER = "com.intellij.openapi.fileEditor.ClientFileEditorManager"
    private const val CLIENT_KIND = "com.intellij.openapi.client.ClientKind"

    fun remoteFileEditorManagers(project: Project): List<Any> = runCatching {
        val managerClass = Class.forName(CLIENT_MANAGER)
        val kindClass = Class.forName(CLIENT_KIND)
        val remote = kindClass.getField("REMOTE").get(null)
        val method = project.javaClass.methods.firstOrNull {
            it.name == "getServices" && it.parameterCount == 2 &&
                it.parameterTypes[0] == Class::class.java && it.parameterTypes[1].isInstance(remote)
        } ?: return emptyList()
        (method.invoke(project, managerClass, remote) as? Iterable<*>)?.filterNotNull().orEmpty()
    }.getOrDefault(emptyList())

    fun allFiles(manager: Any): List<VirtualFile> =
        invokeIterable(manager, "getAllFiles").filterIsInstance<VirtualFile>()

    fun selectedFiles(manager: Any): List<VirtualFile> =
        invokeIterable(manager, "getSelectedFiles").filterIsInstance<VirtualFile>()

    fun allEditors(manager: Any): List<FileEditor> =
        invokeIterable(manager, "getAllEditors").filterIsInstance<FileEditor>()

    fun activeEditors(project: Project): Set<Editor> = runCatching {
        val trackerClass = Class.forName("com.intellij.codeInsight.daemon.impl.EditorTracker")
        val companion = trackerClass.getField("Companion").get(null)
        val tracker = companion.javaClass.methods.single {
            it.name == "getInstance" && it.parameterCount == 1
        }.invoke(companion, project)
        invokeIterable(tracker, "getActiveEditors").filterIsInstance<Editor>().toSet()
    }.getOrDefault(emptySet())

    fun toolWindowPanes(project: Project): List<Any> = runCatching {
        val manager = ToolWindowManager.getInstance(project)
        val method = manager.javaClass.methods.firstOrNull {
            it.name.startsWith("getToolWindowPanes") && it.parameterCount == 0
        } ?: return emptyList()
        (method.invoke(manager) as? Iterable<*>)?.filterNotNull().orEmpty()
    }.getOrDefault(emptyList())

    fun paneId(pane: Any): String? = runCatching {
        pane.javaClass.methods.firstOrNull {
            it.name == "getPaneId" && it.parameterCount == 0
        }?.invoke(pane) as? String
    }.getOrNull()

    fun paneFrame(pane: Any): Window? = runCatching {
        pane.javaClass.methods.firstOrNull {
            it.name.startsWith("getFrame") && it.parameterCount == 0
        }?.invoke(pane) as? Window
    }.getOrNull()

    fun relocateToolWindow(manager: ToolWindowManager, vararg arguments: Any): Boolean = runCatching {
        val method = manager.javaClass.methods.firstOrNull {
            it.name.startsWith("setSideToolAndAnchor") && it.parameterCount == arguments.size
        } ?: return false
        method.invoke(manager, *arguments)
        true
    }.getOrDefault(false)

    fun supportsToolWindowRelocation(manager: ToolWindowManager): Boolean =
        manager.javaClass.methods.any { it.name.startsWith("setSideToolAndAnchor") && it.parameterCount == 5 }

    private fun invokeIterable(receiver: Any, name: String): List<Any> = runCatching {
        val method = receiver.javaClass.methods.firstOrNull {
            it.name == name && it.parameterCount == 0
        } ?: return emptyList()
        (method.invoke(receiver) as? Iterable<*>)?.filterNotNull().orEmpty()
    }.getOrDefault(emptyList())

    fun pluginVersion(pluginId: String): String? = pluginDescriptor(pluginId)?.let { descriptor ->
        runCatching {
            descriptor.javaClass.methods.first { it.name == "getVersion" && it.parameterCount == 0 }
                .invoke(descriptor) as? String
        }.getOrNull()
    }

    fun pluginEnabled(pluginId: String): Boolean = pluginDescriptor(pluginId)?.let { descriptor ->
        runCatching {
            descriptor.javaClass.methods.first { it.name == "isEnabled" && it.parameterCount == 0 }
                .invoke(descriptor) as? Boolean
        }.getOrNull()
    } == true

    /** The directory the IDE loaded this plugin from (`<plugins>/agent-doc-jetbrains`). */
    fun pluginPath(pluginId: String): java.nio.file.Path? = pluginDescriptor(pluginId)?.let { descriptor ->
        runCatching {
            descriptor.javaClass.methods.first { it.name == "getPluginPath" && it.parameterCount == 0 }
                .invoke(descriptor) as? java.nio.file.Path
        }.getOrNull()
    }

    private fun pluginDescriptor(pluginId: String): Any? = runCatching {
        val manager = Class.forName("com.intellij.ide.plugins.PluginManagerCore")
        manager.getMethod("getPlugin", PluginId::class.java).invoke(null, PluginId.getId(pluginId))
    }.getOrNull()
}
