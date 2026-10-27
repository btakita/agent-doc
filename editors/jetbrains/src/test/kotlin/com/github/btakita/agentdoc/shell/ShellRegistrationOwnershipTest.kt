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

    @Test
    fun `the shell package names the implementation only through the generation class`() {
        val shellSources = File("src/main/kotlin/com/github/btakita/agentdoc/shell").listFiles().orEmpty()
            .filter { it.extension == "kt" }
        val implTypes = manifest.implSources().flatMap { file ->
            Regex("""^(?:internal |private )?(?:abstract |open |data |sealed |enum )*(?:class|object|interface) (\w+)""", RegexOption.MULTILINE)
                .findAll(file.readText()).map { it.groupValues[1] }.toList()
        }.toSet()
        val references = shellSources.flatMap { file ->
            val code = file.readLines().filterNot { it.trimStart().startsWith("*") || it.trimStart().startsWith("//") }
                .joinToString("\n")
                .replace(Regex(""""[^"\n]*""""), "\"\"")
            implTypes.filter { Regex("""\b$it\b""").containsMatchIn(code) }.map { "${file.name} -> $it" }
        }
        assertEquals("shell package references to implementation types", emptyList<String>(), references)
        assertTrue(IMPL_GENERATION_CLASS.startsWith("com.github.btakita.agentdoc."))
        assertFalse(manifest.isShell(sourceOf(IMPL_GENERATION_CLASS)))
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
        val PENDING: List<Pair<String, Regex>> = listOf(
            "message_bus_subscribe" to Regex("""\.subscribe\("""),
            "document_or_vfs_listener" to Regex("""add(?:Document|VirtualFile|Bulk\w*)Listener\("""),
            "invoke_later" to Regex("""invokeLater\s*[({]"""),
            "pooled_thread" to Regex("""executeOnPooledThread\s*[({]"""),
            "app_executor_util" to Regex("""AppExecutorUtil\."""),
            "raw_thread" to Regex("""\bThread\s*[({]"""),
            "executors_new" to Regex("""Executors\.new"""),
            "alarm" to Regex("""\bAlarm\("""),
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
