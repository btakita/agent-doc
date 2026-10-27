package com.github.btakita.agentdoc.shell

import com.github.btakita.agentdoc.AgentDocImplGeneration
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.project.DumbAware
import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/**
 * GH #339 R1 acceptance audit: "zero direct platform registrations from implementation
 * packages", enforced per registration class as each class moves behind the shell
 * (editors/jetbrains/SPEC.md "R1 status"). A class still listed in [PENDING] is measured and
 * printed, not enforced, until its forwarders land; then it moves to an enforced test here.
 */
class ShellRegistrationOwnershipTest {
    private val pluginXml = File("src/main/resources/META-INF/plugin.xml")
    private val manifest = ShellSourceManifest.read(File("shell-sources.txt"))

    private fun descriptorAttributes(tag: String, attribute: String, xml: File = pluginXml): List<String> {
        val document = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(xml)
        val nodes = document.getElementsByTagName(tag)
        return (0 until nodes.length)
            .map { (nodes.item(it) as Element).getAttribute(attribute) }
            .filter { it.isNotBlank() }
    }

    private fun load(name: String): Class<*> =
        Class.forName(name, false, ShellRegistrationOwnershipTest::class.java.classLoader)

    /** The source file that declares top-level class [className] in the main source set. */
    private fun sourceOf(className: String): String {
        val outer = className.substringBefore('$')
        val pkg = outer.substringBeforeLast('.').replace('.', '/')
        val simple = outer.substringAfterLast('.')
        val dir = File("src/main/kotlin/$pkg")
        val declaring = dir.listFiles().orEmpty().filter { it.extension == "kt" }.singleOrNull { file ->
            Regex("""^(?:internal |abstract |open |final )*class $simple\b""", RegexOption.MULTILINE)
                .containsMatchIn(file.readText())
        } ?: error("no source in $dir declares class $simple")
        return declaring.relativeTo(File(".")).invariantSeparatorsPath
    }

    private val actionIds = descriptorAttributes("action", "id")

    @Test
    fun `every plugin xml action is a shell forwarder for its own id`() {
        val classes = descriptorAttributes("action", "class")
        assertEquals("plugin.xml should still declare all 27 Agent Doc actions", 27, actionIds.size)
        assertEquals(actionIds.size, classes.size)
        val wrong = actionIds.zip(classes).mapNotNull { (id, className) ->
            val forwarder = load(className)
            val expectedName = "com.github.btakita.agentdoc.shell.${id.removePrefix("AgentDoc.")}Forwarder"
            when {
                className != expectedName -> "$id is declared by $className, expected $expectedName"
                !ShellActionForwarder::class.java.isAssignableFrom(forwarder) -> "$className is not a ShellActionForwarder"
                !DumbAware::class.java.isAssignableFrom(forwarder) -> "$className is not DumbAware"
                (forwarder.getDeclaredConstructor().newInstance() as ShellActionForwarder).actionId != id ->
                    "$className forwards a different id than $id"
                !manifest.isShell(sourceOf(className)) -> "$className is not in the shell file set"
                else -> null
            }
        }
        assertEquals(emptyList<String>(), wrong)
    }

    @Test
    fun `the implementation generation serves exactly the declared action ids`() {
        assertEquals(actionIds, AgentDocImplGeneration.ACTION_HANDLERS.keys.toList())
        val generation = load(IMPL_GENERATION_CLASS).getDeclaredConstructor().newInstance() as ImplGeneration
        assertNull("an undeclared id must not resolve", generation.action("AgentDoc.NotDeclared"))
        val leaked = actionIds.mapNotNull { id ->
            val handler = generation.action(id) ?: return@mapNotNull "$id has no handler"
            when {
                handler is AnAction -> "$id handler ${handler.javaClass.name} is an AnAction the platform never sees"
                manifest.isShell(sourceOf(handler.javaClass.name)) -> "$id handler ${handler.javaClass.name} lives in the shell"
                generation.action(id) !== handler -> "$id handler is not stable within one generation"
                else -> null
            }
        }
        assertEquals(emptyList<String>(), leaked)
    }

    @Test
    fun `no implementation class or object extends AnAction`() {
        val subclasses = manifest.implSources().flatMap { file ->
            file.readLines().withIndex()
                .filter { (_, line) -> Regex(""":\s*AnAction\(""").containsMatchIn(line.substringBefore("//")) }
                .map { (index, _) -> "${file.name}:${index + 1}" }
        }
        assertEquals("implementation AnAction subclasses", emptyList<String>(), subclasses)
    }

    @Test
    fun `xml services settings and configurables are shell classes`() {
        val declared = descriptorAttributes("applicationService", "serviceImplementation") +
            descriptorAttributes("projectService", "serviceImplementation") +
            descriptorAttributes("applicationConfigurable", "instance") +
            descriptorAttributes("listener", "class")
        assertEquals("plugin.xml services, configurables and project listener", 7, declared.size)
        assertEquals(emptyList<String>(), declared.filterNot { manifest.isShell(sourceOf(it)) })
        val lightServices = manifest.implSources().filter { "@Service" in it.readText() }.map { it.name }
        assertEquals("implementation light services", emptyList<String>(), lightServices)
    }

    /** Implementation types [file]'s code (comments and string literals removed) names. */
    private fun implReferences(file: File): List<String> {
        val implTypes = manifest.implSources().flatMap { source ->
            Regex("""^(?:internal |private )?(?:abstract |open |data |sealed |enum )*(?:class|object|interface) (\w+)""", RegexOption.MULTILINE)
                .findAll(source.readText()).map { it.groupValues[1] }.toList()
        }.toSet()
        val code = file.readLines().filterNot { it.trimStart().startsWith("*") || it.trimStart().startsWith("//") || it.trimStart().startsWith("/*") }
            .joinToString("\n")
            .replace(Regex(""""[^"\n]*""""), "\"\"")
        return implTypes.filter { Regex("""\b$it\b""").containsMatchIn(code) }.map { "${file.name} -> $it" }
    }

    @Test
    fun `the shell package names the implementation only through the generation class`() {
        val shellSources = File("src/main/kotlin/com/github/btakita/agentdoc/shell").listFiles().orEmpty()
            .filter { it.extension == "kt" }
        val references = shellSources.flatMap(::implReferences)
        assertEquals("shell package references to implementation types", emptyList<String>(), references)
        assertTrue(IMPL_GENERATION_CLASS.startsWith("com.github.btakita.agentdoc."))
        assertFalse(manifest.isShell(sourceOf(IMPL_GENERATION_CLASS)))
    }

    /**
     * Project lifecycle: the `<projectListeners>` owner and the services it declares forward to the
     * generation and run no implementation code themselves; the attach bridge's static hooks
     * stay on the listener class, which the bridge resolves by name.
     */
    @Test
    fun `the project lifecycle shell forwards to the generation`() {
        val lifecycle = File("src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt")
        assertTrue(manifest.isShell(lifecycle.path))
        assertEquals("project lifecycle shell references to implementation types", emptyList<String>(), implReferences(lifecycle))
        val listener = load("com.github.btakita.agentdoc.PluginLifecycleListener")
        listOf("initializeOpenProjectsAfterDynamicLoad", "disposeOpenProjectsForDynamicUnload").forEach { hook ->
            val method = listener.getMethod(hook)
            assertTrue("$hook must stay a static method on PluginLifecycleListener", java.lang.reflect.Modifier.isStatic(method.modifiers))
        }
        assertFalse(manifest.isShell("src/main/kotlin/com/github/btakita/agentdoc/AgentDocProjectLifecycle.kt"))
    }

    /**
     * Editor and language extensions: every XML extension class in `plugin.xml` and the optional
     * Markdown descriptor is a shell forwarder in the `shell` package, and no implementation class
     * extends a platform extension or widget type, so the platform caches no implementation
     * object. The two notification groups are XML-only (no class).
     */
    @Test
    fun `xml editor and language extensions are shell forwarders`() {
        val markdownXml = File("src/main/resources/META-INF/agent-doc-markdown.xml")
        val extensionClasses =
            listOf(
                "completion.contributor" to "implementationClass",
                "statusBarWidgetFactory" to "implementation",
                "editorNotificationProvider" to "implementation",
            ).flatMap { (tag, attribute) -> descriptorAttributes(tag, attribute) } +
                listOf(
                    "lang.foldingBuilder" to "implementationClass",
                    "codeInsight.lineMarkerProvider" to "implementationClass",
                    "lang.structureViewExtension" to "implementation",
                ).flatMap { (tag, attribute) -> descriptorAttributes(tag, attribute, markdownXml) }
        assertEquals("editor and language extensions", 6, extensionClasses.size)
        val notShell = extensionClasses.filterNot { className ->
            load(className)
            className.startsWith("com.github.btakita.agentdoc.shell.") && manifest.isShell(sourceOf(className))
        }
        assertEquals("extension classes outside the shell", emptyList<String>(), notShell)
        assertEquals(2, descriptorAttributes("notificationGroup", "id").size)
        assertEquals(emptyList<String>(), descriptorAttributes("notificationGroup", "class"))

        val platformTypes = Regex(
            """:\s*(?:CompletionContributor|FoldingBuilder\w*|LineMarkerProvider|StructureViewExtension|""" +
                """StatusBarWidgetFactory|StatusBarWidget|EditorNotificationProvider)\b""",
        )
        val implementers = manifest.implSources().flatMap { file ->
            file.readLines().withIndex()
                .filter { (_, line) -> platformTypes.containsMatchIn(line.substringBefore("//")) }
                .map { (index, _) -> "${file.name}:${index + 1}" }
        }
        assertEquals("implementation classes of platform extension types", emptyList<String>(), implementers)
    }

    /**
     * Tier A listeners and EDT/pooled work: implementation code stages every message-bus subscription and
     * editor-factory or multicaster listener through `GenerationListeners`, which hands it to the
     * shell's per-topic forwarder. Only that bridge, in the generation entry point, may call the
     * context directly.
     */
    @Test
    fun `implementation code registers no message bus or editor listener directly`() {
        val bridge = "AgentDocImplGeneration.kt"
        val direct = manifest.implSources().filter { it.name != bridge }.flatMap { file ->
            val text = file.readText()
            ENFORCED_ZERO.flatMap { (name, pattern) ->
                pattern.findAll(text).map { match ->
                    "$name ${file.name}:${text.substring(0, match.range.first).count { it == '\n' } + 1}"
                }.toList()
            }
        }
        assertEquals("direct implementation registrations", emptyList<String>(), direct)
        val staged = manifest.implSources().filter { it.name != bridge }.sumOf { file ->
            Regex("""GenerationListeners\.(?:subscribe|add\w+Listener)\(""").findAll(file.readText()).count()
        }
        assertEquals("listener registrations staged through GenerationListeners", STAGED_LISTENERS, staged)
    }

    /**
     * EDT and pooled work: implementation code queues EDT tasks and pooled or scheduled work, and
     * creates executors, threads and alarms, only through `GenerationWork`, which hands them to the
     * generation's shell work owner (`GenerationContext.invokeLater` / `GenerationContext.work`).
     * The direct patterns are enforced at zero by the test above; this pins the staged count.
     */
    @Test
    fun `implementation work is staged through the generation`() {
        val staged = manifest.implSources().filter { it.name != "AgentDocImplGeneration.kt" }.flatMap { file ->
            Regex("""GenerationWork\.(\w+)\s*[({]""").findAll(file.readText()).map { it.groupValues[1] }.toList()
        }.groupingBy { it }.eachCount().toSortedMap()
        assertEquals("work staged through GenerationWork", STAGED_WORK, staged)
    }

    /**
     * The registration classes R1 has not moved yet, measured by text search of the
     * implementation file set. Printed so a follow-up can compare; enforced once it lands.
     */
    @Test
    fun `pending registration classes are measured`() {
        val sources = manifest.implSources().map { it.readText() }
        val report = PENDING.map { (name, pattern) ->
            "$name=${sources.sumOf { text -> pattern.findAll(text).count() }}"
        }
        println("[shell-audit] implementation registrations still direct: ${report.joinToString(" ")}")
        assertTrue(report.isNotEmpty())
    }

    private companion object {
        /**
         * Registration classes that have moved behind the shell: zero in implementation files.
         * `GenerationListeners.` calls are the staged form and do not match.
         */
        val ENFORCED_ZERO: List<Pair<String, Regex>> = listOf(
            "message_bus_subscribe" to Regex("""(?<!GenerationListeners)\.subscribe\("""),
            "message_bus_connect" to Regex("""messageBus\s*\.\s*connect\b"""),
            "document_or_vfs_listener" to Regex("""(?<!GenerationListeners)\.add(?:Document|VirtualFile|Bulk\w*)Listener\("""),
            "editor_factory_or_multicaster_listener" to Regex("""(?<!GenerationListeners)\.add(?:EditorFactory|EditorMouse\w*)Listener\("""),
            // EDT and pooled work: `GenerationWork.` calls are the staged form.
            "invoke_later" to Regex("""(?<!GenerationWork\.)invokeLater\s*[({]"""),
            "pooled_thread" to Regex("""(?<!GenerationWork\.)executeOnPooledThread\s*[({]"""),
            "app_executor_util" to Regex("""AppExecutorUtil\."""),
            "raw_thread" to Regex("""(?<!\w)Thread\s*[({]"""),
            "executors_new" to Regex("""Executors\.new"""),
            "alarm" to Regex("""\bAlarm\("""),
        )

        /** Every Tier A registration the implementation stages (one per former direct call). */
        const val STAGED_LISTENERS = 17

        /** Every EDT, pooled, executor, thread and alarm call the implementation stages. */
        val STAGED_WORK: Map<String, Int> = sortedMapOf(
            "executeOnPooledThread" to 20,
            "invokeLater" to 43,
            "newPooledAlarm" to 1,
            "newSingleThreadExecutor" to 1,
            "newSingleThreadScheduledExecutor" to 8,
            "newThread" to 14,
            "schedule" to 5,
            "scheduleWithFixedDelay" to 1,
            "submit" to 1,
        )

        val PENDING: List<Pair<String, Regex>> = listOf(
            // AWT and per-editor component listeners are not Tier A topics; measured until moved.
            "awt_or_component_listener" to Regex("""add(?:AWTEvent|Focus)Listener\("""),
            // Notification objects the platform keeps in its notification log (the groups
            // themselves are XML-only); measured until the shell builds them.
            "notification" to Regex("""\bNotification\(|createNotification\("""),
            "key_create" to Regex("""Key\.create\b"""),
            "put_user_data" to Regex("""putUserData\("""),
        )
    }
}

/** Reads `shell-sources.txt` with the same rules as `shell-identity.gradle.kts`. */
internal class ShellSourceManifest private constructor(
    private val base: File,
    private val shell: List<String>,
    private val impl: List<String>,
) {
    fun isShell(path: String): Boolean = shell.any { matches(it, path) }

    fun isImpl(path: String): Boolean = !isShell(path) && impl.any { matches(it, path) }

    /** Kotlin and Java sources in the implementation file set. */
    fun implSources(): List<File> =
        base.resolve("src/main").walkTopDown()
            .filter { it.isFile && (it.extension == "kt" || it.extension == "java") }
            .filter { isImpl(it.relativeTo(base).invariantSeparatorsPath) }
            .sortedBy { it.path }
            .toList()

    private fun matches(entry: String, path: String) =
        if (entry.endsWith("/")) path.startsWith(entry) else path == entry

    companion object {
        fun read(file: File): ShellSourceManifest {
            val sections = mutableMapOf<String, MutableList<String>>()
            var current: MutableList<String>? = null
            file.readLines().map(String::trim).filter { it.isNotEmpty() && !it.startsWith("#") }.forEach { line ->
                if (line.startsWith("[")) {
                    current = sections.getOrPut(line.removeSurrounding("[", "]")) { mutableListOf() }
                } else {
                    current!!.add(line)
                }
            }
            return ShellSourceManifest(
                file.absoluteFile.parentFile,
                sections.getValue("shell"),
                sections.getValue("impl"),
            )
        }
    }
}
