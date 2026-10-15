package com.github.btakita.agentdoc

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import java.io.File
import java.net.URI
import java.util.jar.JarFile
import java.lang.reflect.Modifier
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/**
 * GH #265: an action that does not override `getActionUpdateThread()` inherits
 * the platform's `OLD_EDT` default, so its `update()` runs on the EDT. Reading a
 * rule-provided data key there (`CommonDataKeys.VIRTUAL_FILE`, which build 263
 * resolves through a rule) makes `PreCachedDataContext` log a SEVERE "requested
 * on EDT ... See ActionUpdateThread javadoc" blamed on Agent Doc, once per action
 * per IDE session. Every agent-doc action and group must therefore declare its
 * update thread itself, and must not pick `OLD_EDT`.
 *
 * The action set is derived from the compiled plugin classes (every concrete
 * [AnAction] subclass, including runtime-built groups and anonymous actions)
 * and from `plugin.xml` registrations, never a hand-maintained list, so a new
 * action that forgets the override fails here.
 */
class ActionUpdateThreadTest {
    private val packagePrefix = "com.github.btakita.agentdoc."
    private val pluginXml = File("src/main/resources/META-INF/plugin.xml")

    private fun load(name: String): Class<*> =
        Class.forName(name, false, ActionUpdateThreadTest::class.java.classLoader)

    /** Binary names of every class compiled into the plugin's main package. */
    private fun compiledClassNames(): List<String> {
        val anchor = SubmitAction::class.java.name.replace('.', '/') + ".class"
        val url = ActionUpdateThreadTest::class.java.classLoader.getResource(anchor)
            ?: error("cannot locate compiled $anchor")
        val entries = when (url.protocol) {
            "file" -> {
                val root = File(url.toURI()).path.removeSuffix(anchor.replace('/', File.separatorChar))
                File(root).walkTopDown()
                    .filter { it.isFile }
                    .map { it.relativeTo(File(root)).path.replace(File.separatorChar, '/') }
                    .toList()
            }
            "jar" -> {
                val jarPath = URI(url.path.substringBefore("!/")).path
                JarFile(jarPath).use { jar -> jar.entries().asSequence().map { it.name }.toList() }
            }
            else -> error("unsupported class location $url")
        }
        return entries
            .filter { it.endsWith(".class") }
            .map { it.removeSuffix(".class").replace('/', '.') }
            .filter { it.startsWith(packagePrefix) }
    }

    /** Every concrete [AnAction] subclass compiled from the plugin's main sources. */
    private fun compiledActionClasses(): List<Class<*>> {
        val unloadable = mutableListOf<String>()
        val actions = compiledClassNames().asSequence()
            .mapNotNull { name ->
                try {
                    load(name)
                } catch (e: LinkageError) {
                    unloadable += "$name: $e"
                    null
                }
            }
            .filter { AnAction::class.java.isAssignableFrom(it) }
            .filterNot { Modifier.isAbstract(it.modifiers) || it.isInterface }
            .toList()
        // A class that cannot be loaded cannot be checked; never skip it silently.
        assertEquals("unloadable plugin classes", emptyList<String>(), unloadable)
        return actions
    }

    private fun registeredClassNames(tag: String): List<String> {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(pluginXml)
        val nodes = document.getElementsByTagName(tag)
        return (0 until nodes.length)
            .map { (nodes.item(it) as Element).getAttribute("class") }
            .filter { it.isNotBlank() }
    }

    /** The class that supplies `getActionUpdateThread()` for [action]. */
    private fun updateThreadDeclarer(action: Class<*>): Class<*> =
        action.getMethod("getActionUpdateThread").declaringClass

    private fun missingOverride(actions: Collection<Class<*>>): List<String> =
        actions
            .filterNot { updateThreadDeclarer(it).name.startsWith(packagePrefix) }
            .map { "${it.name} inherits getActionUpdateThread() from ${updateThreadDeclarer(it).name}" }
            .sorted()

    @Test
    fun `every compiled agent-doc action declares its update thread`() {
        val actions = compiledActionClasses()
        assertTrue("expected agent-doc action classes, found ${actions.size}", actions.size >= 20)
        assertTrue(
            "the runtime popup group must be part of the scan",
            actions.any { it.name == "${packagePrefix}DumbAwareGroup" },
        )
        assertEquals(emptyList<String>(), missingOverride(actions))
    }

    @Test
    fun `every action and group registered in plugin xml declares its update thread`() {
        val registered = (registeredClassNames("action") + registeredClassNames("group")).map(::load)
        assertTrue("plugin.xml should register agent-doc actions", registered.size >= 20)
        assertEquals(emptyList<String>(), missingOverride(registered))
        val compiled = compiledActionClasses().map { it.name }.toSet()
        assertEquals(
            "plugin.xml registers a class the compiled-class scan did not find",
            emptyList<String>(),
            registered.map { it.name }.filterNot { it in compiled },
        )
    }

    @Test
    fun `no agent-doc action returns the deprecated OLD_EDT update thread`() {
        val oldEdt = compiledActionClasses()
            .filter { it.declaredConstructors.any { ctor -> ctor.parameterCount == 0 } }
            .filter { cls ->
                val ctor = cls.getDeclaredConstructor().apply { isAccessible = true }
                (ctor.newInstance() as AnAction).actionUpdateThread == ActionUpdateThread.OLD_EDT
            }
            .map { it.name }
        assertEquals(emptyList<String>(), oldEdt)
    }
}
