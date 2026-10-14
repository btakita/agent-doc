import java.nio.file.Files
import org.gradle.api.file.DuplicatesStrategy
import org.gradle.jvm.tasks.Jar
import javax.xml.parsers.DocumentBuilderFactory
import javax.xml.transform.OutputKeys
import javax.xml.transform.TransformerFactory
import javax.xml.transform.dom.DOMSource
import javax.xml.transform.stream.StreamResult

val embeddedLazily by configurations.creating

dependencies {
    intellijPlatform {
        bundledModule("intellij.platform.kernel.backend")
        bundledModule("intellij.platform.rpc.backend")
        bundledModule("intellij.platform.backend")
        bundledModule("intellij.platform.vcs")
        bundledPlugin("org.jetbrains.plugins.terminal")
        bundledPlugin("org.intellij.plugins.markdown")
    }

    implementation(project(":shared"))
    compileOnly("io.github.lazily:lazily:0.39.0") {
        exclude(group = "org.jetbrains.kotlinx")
        exclude(group = "org.jetbrains.kotlin")
        exclude(group = "org.jetbrains", module = "annotations")
        exclude(group = "net.java.dev.jna")
    }
    testImplementation("io.github.lazily:lazily:0.39.0") {
        exclude(group = "org.jetbrains.kotlinx")
        exclude(group = "org.jetbrains.kotlin")
        exclude(group = "org.jetbrains", module = "annotations")
        exclude(group = "net.java.dev.jna")
    }
    embeddedLazily("io.github.lazily:lazily:0.39.0") {
        exclude(group = "org.jetbrains.kotlinx")
        exclude(group = "org.jetbrains.kotlin")
        exclude(group = "org.jetbrains", module = "annotations")
        exclude(group = "net.java.dev.jna")
    }
    compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
    testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
    testImplementation(kotlin("test"))
    testImplementation("junit:junit:4.13.2")
}

// Content modules have isolated classloaders. Keep the backend's private Lazily
// runtime in the backend module JAR instead of the root plugin lib directory.
tasks.named<Jar>("jar") {
    from({ embeddedLazily.map { zipTree(it) } }) {
        include("io/github/lazily/**")
    }
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
}

sourceSets {
    main {
        kotlin.srcDir("../../jetbrains/src/main/kotlin")
        kotlin.exclude("**/PluginLifecycleListener.kt")
        java.srcDir("../../jetbrains/src/main/java")
    }
}

val generatedResources = layout.buildDirectory.dir("generated/agent-doc-backend-resources")
val generatedSources = layout.buildDirectory.dir("generated/agent-doc-backend-sources")

val generateModularLifecycle by tasks.registering {
    val legacyLifecycle = layout.projectDirectory.file(
        "../../jetbrains/src/main/kotlin/com/github/btakita/agentdoc/PluginLifecycleListener.kt",
    )
    val output = generatedSources.map {
        it.file("com/github/btakita/agentdoc/PluginLifecycleListener262Generated.kt")
    }
    inputs.file(legacyLifecycle)
    outputs.file(output)

    doLast {
        val original = legacyLifecycle.asFile.readText()
        val startMarker =
            "        // Register EditorTabSyncListener via code (not XML) so it survives hot-reload"
        val endMarker =
            "        // Drive tmux pane focus on split-editor focus changes (#panefocussplit):"
        val start = original.indexOf(startMarker)
        val end = original.indexOf(endMarker)
        check(start >= 0 && end > start) {
            "legacy lifecycle layout-listener markers changed; review the 262 fail-closed override"
        }
        val replacement = """
        // The 262 frontend module is the only editor-surface authority. It captures pane-tagged
        // snapshots before dedupe and sends them through authenticated RPC. Installing the classic
        // backend EditorTabSyncListener here would reintroduce the ambiguous flat
        // ClientFileEditorManager path and race the generation-fenced source.

"""
        val transformed = original.substring(0, start) + replacement + original.substring(end)
        val target = output.get().asFile
        Files.createDirectories(target.toPath().parent)
        target.writeText(transformed)
    }
}

sourceSets.main {
    kotlin.srcDir(generatedSources)
}

val generateBackendDescriptor by tasks.registering {
    val legacyDescriptor = layout.projectDirectory.file("../../jetbrains/src/main/resources/META-INF/plugin.xml")
    val markdownDescriptor = layout.projectDirectory.file("../../jetbrains/src/main/resources/META-INF/agent-doc-markdown.xml")
    val output = generatedResources.map { it.file("agent.doc.backend.xml") }
    inputs.files(legacyDescriptor, markdownDescriptor)
    outputs.file(output)

    doLast {
        val factory = DocumentBuilderFactory.newInstance().apply {
            isNamespaceAware = true
            setFeature("http://apache.org/xml/features/disallow-doctype-decl", true)
        }
        val builder = factory.newDocumentBuilder()
        val legacy = builder.parse(legacyDescriptor.asFile)
        val markdown = builder.parse(markdownDescriptor.asFile)
        val result = builder.newDocument()
        val root = result.createElement("idea-plugin")
        result.appendChild(root)

        val dependencies = result.createElement("dependencies")
        listOf(
            "intellij.platform.backend",
            "intellij.platform.kernel.backend",
            "intellij.platform.vcs",
            "agent.doc.shared",
        ).forEach { name ->
            dependencies.appendChild(result.createElement("module").apply { setAttribute("name", name) })
        }
        listOf("org.jetbrains.plugins.terminal", "org.intellij.plugins.markdown").forEach { id ->
            dependencies.appendChild(result.createElement("plugin").apply { setAttribute("id", id) })
        }
        root.appendChild(dependencies)

        val copiedSections = linkedMapOf<String, org.w3c.dom.Element>()
        listOf("extensions", "projectListeners", "actions").forEach { tag ->
            val node = legacy.documentElement.childNodes
            for (index in 0 until node.length) {
                val child = node.item(index)
                if (child is org.w3c.dom.Element && child.tagName == tag) {
                    val imported = result.importNode(child, true) as org.w3c.dom.Element
                    root.appendChild(imported)
                    copiedSections[tag] = imported
                }
            }
        }
        val extensions = copiedSections.getValue("extensions")
        val markdownExtensions = markdown.documentElement.getElementsByTagName("extensions").item(0)
        if (markdownExtensions != null) {
            val children = markdownExtensions.childNodes
            for (index in 0 until children.length) {
                val child = children.item(index)
                if (child is org.w3c.dom.Element) extensions.appendChild(result.importNode(child, true))
            }
        }
        extensions.appendChild(
            result.createElement("platform.rpc.backend.remoteApiProvider").apply {
                setAttribute("implementation", "com.github.btakita.agentdoc.split.backend.SurfaceSnapshotRemoteApiProvider")
            },
        )

        val target = output.get().asFile
        Files.createDirectories(target.toPath().parent)
        TransformerFactory.newInstance().newTransformer().apply {
            setOutputProperty(OutputKeys.INDENT, "yes")
            setOutputProperty("{http://xml.apache.org/xslt}indent-amount", "4")
        }.transform(DOMSource(result), StreamResult(target))
    }
}

sourceSets.main {
    resources.srcDir(generatedResources)
}

tasks.processResources {
    dependsOn(generateBackendDescriptor)
}

tasks.compileKotlin {
    dependsOn(generateModularLifecycle)
}
