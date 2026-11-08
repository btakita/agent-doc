package com.github.btakita.agentdoc

import com.intellij.openapi.editor.Editor
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.ToolWindowManager
import java.awt.Window
import java.lang.reflect.Method

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
        val method = selectToolWindowPanesMethod(manager.javaClass.methods) ?: return emptyList()
        toolWindowPaneInstances(method.invoke(manager))
    }.getOrDefault(emptyList())

    /**
     * IDEA 2026.3 publishes two public zero-arg `getToolWindowPanes$…` methods:
     * the property getter returns `LinkedHashMap`, and the list getter returns
     * `List`. `Class.getMethods` order is unspecified and the map getter is
     * first in the 2026.3 class file. A map is not a `java.lang.Iterable`, so
     * taking the first match yields no panes and every auto-sync gate stays
     * `project_surface_inactive`. Prefer the list. A platform that only has the
     * map still contributes its values.
     */
    internal fun selectToolWindowPanesMethod(methods: Array<out Method>): Method? {
        val zeroArg = methods.filter { method ->
            method.parameterCount == 0 && method.name.startsWith("getToolWindowPanes")
        }
        return zeroArg.firstOrNull { java.util.List::class.java.isAssignableFrom(it.returnType) }
            ?: zeroArg.firstOrNull { java.util.Map::class.java.isAssignableFrom(it.returnType) }
    }

    internal fun toolWindowPaneInstances(value: Any?): List<Any> = when (value) {
        is Map<*, *> -> value.values.filterNotNull()
        is Iterable<*> -> value.filterNotNull()
        else -> emptyList()
    }

    fun paneId(pane: Any): String? = runCatching {
        pane.javaClass.methods.firstOrNull {
            it.name == "getPaneId" && it.parameterCount == 0
        }?.invoke(pane) as? String
    }.getOrNull()

    internal fun selectPaneFrameMethod(methods: Array<out Method>): Method? =
        methods.firstOrNull { method ->
            method.parameterCount == 0 &&
                method.name.startsWith("getFrame") &&
                Window::class.java.isAssignableFrom(method.returnType)
        }

    fun paneFrame(pane: Any): Window? = runCatching {
        selectPaneFrameMethod(pane.javaClass.methods)?.invoke(pane) as? Window
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
