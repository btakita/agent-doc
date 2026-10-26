import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.tasks.aware.SplitModeAware
import org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask
import org.w3c.dom.Element
import java.io.ByteArrayInputStream
import java.util.EnumSet
import java.util.jar.JarInputStream
import java.util.zip.ZipFile
import javax.xml.parsers.DocumentBuilderFactory

data class ContentModuleContract(
    val loading: String?,
    val requiredIfAvailable: String?,
)

fun parsePluginXml(content: String) = DocumentBuilderFactory.newInstance().apply {
    isNamespaceAware = true
    setFeature("http://apache.org/xml/features/disallow-doctype-decl", true)
}.newDocumentBuilder().parse(ByteArrayInputStream(content.toByteArray()))

fun Element.directChildren(tagName: String): List<Element> = buildList {
    val children = childNodes
    for (index in 0 until children.length) {
        val child = children.item(index)
        if (child is Element && child.tagName == tagName) add(child)
    }
}

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm")
    id("org.jetbrains.intellij.platform")
    id("rpc") apply false
    id("org.jetbrains.kotlin.plugin.serialization") apply false
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

val platformVersion = providers.gradleProperty("platformVersion")
val distributionName = providers.gradleProperty("distributionName")
    .orElse("agent-doc-jetbrains-262")
val compatibilitySinceBuild = providers.gradleProperty("compatibilitySinceBuild")
    .orElse("262")
val compatibilityUntilBuild = providers.gradleProperty("compatibilityUntilBuild")
    .orElse("262.*")

subprojects {
    apply(plugin = "org.jetbrains.intellij.platform.module")
    apply(plugin = "rpc")
    apply(plugin = "org.jetbrains.kotlin.jvm")
    apply(plugin = "org.jetbrains.kotlin.plugin.serialization")

    group = rootProject.group
    version = rootProject.version

    // The module JAR basename is the module descriptor name.  The platform
    // resolves agent.doc.frontend.xml from agent.doc.frontend.jar (and so on),
    // including when the JAR is installed into the thin client.
    extensions.configure<org.jetbrains.intellij.platform.gradle.extensions.IntelliJPlatformExtension> {
        projectName.set("agent.doc.${project.name}")
    }

    extensions.configure<org.jetbrains.kotlin.gradle.dsl.KotlinJvmProjectExtension> {
        compilerOptions.jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_25)
    }
}

dependencies {
    intellijPlatform {
        intellijIdea(platformVersion)
        pluginModule(implementation(project(":shared")))
        pluginModule(implementation(project(":frontend")))
        pluginModule(implementation(project(":backend")))
        testFramework(TestFrameworkType.Platform)
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_25
    targetCompatibility = JavaVersion.VERSION_25
}

kotlin {
    compilerOptions.jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_25)
}

intellijPlatform {
    projectName = distributionName
    buildSearchableOptions = false
    splitMode = true
    pluginInstallationTarget = SplitModeAware.PluginInstallationTarget.BOTH

    pluginVerification {
        // Marketplace review rejects private platform linkage. Keep those findings as hard gates
        // for every exact split build so a rejected API cannot return unnoticed.
        failureLevel.set(
            EnumSet.of(
                VerifyPluginTask.FailureLevel.COMPATIBILITY_PROBLEMS,
                VerifyPluginTask.FailureLevel.MISSING_DEPENDENCIES,
                VerifyPluginTask.FailureLevel.INVALID_PLUGIN,
                VerifyPluginTask.FailureLevel.PLUGIN_STRUCTURE_WARNINGS,
                VerifyPluginTask.FailureLevel.INTERNAL_API_USAGES,
                VerifyPluginTask.FailureLevel.OVERRIDE_ONLY_API_USAGES,
            ),
        )
        ides {
            create(IntelliJPlatformType.IntellijIdeaUltimate, platformVersion)
        }
    }
}

tasks {
    patchPluginXml {
        sinceBuild.set(compatibilitySinceBuild)
        untilBuild.set(compatibilityUntilBuild)
    }

    register("verifySplitArtifact") {
        group = "verification"
        description = "Verify that the compatibility-ranged ZIP contains all split plugin modules and range metadata"
        dependsOn("buildPlugin")
        doLast {
            val frontendSources = projectDir.resolve("frontend/src/main/kotlin")
            val internalImports = frontendSources.walkTopDown()
                .filter { it.isFile && it.extension == "kt" }
                .flatMap { source ->
                    source.readLines().asSequence()
                        .filter { line ->
                            line.startsWith("import com.intellij.openapi.fileEditor.ex.") ||
                                line.startsWith("import com.intellij.openapi.fileEditor.impl.") ||
                                line.startsWith("import com.intellij.terminal.frontend.")
                        }
                        .map { source.relativeTo(frontendSources).invariantSeparatorsPath to it }
                }
                .toList()
            check(internalImports.all { (path, _) -> path.endsWith("Exact262DetachedPresentationAdapter.kt") }) {
                "build-line-specific internal UI imports escaped the designated adapter: $internalImports"
            }
            val rejectedImports = projectDir.walkTopDown()
                .filter { it.isFile && it.extension == "kt" && "/src/main/" in it.invariantSeparatorsPath }
                .flatMap { source ->
                    source.readLines().asSequence()
                        .filter { line ->
                            line.startsWith("import com.intellij.openapi.client.") ||
                                line.startsWith("import com.intellij.openapi.fileEditor.ClientFileEditorManager") ||
                                line.startsWith("import com.jetbrains.rdserver.core.RemoteClientSessionListener") ||
                                line.startsWith("import com.intellij.openapi.fileEditor.impl.FileEditorOpenOptions")
                        }
                        .map { source.relativeTo(projectDir).invariantSeparatorsPath to it }
                }.toList()
            check(rejectedImports.isEmpty()) {
                "Marketplace-rejected private split API imports reappeared: $rejectedImports"
            }
            val zip = layout.buildDirectory.dir("distributions").get().asFile
                .listFiles()
                .orEmpty()
                .singleOrNull { it.extension == "zip" && it.name.contains(project.version.toString()) }
                ?: error("missing unique split plugin distribution")
            ZipFile(zip).use { archive ->
                val names = archive.entries().asSequence().map { it.name }.toList()
                val expectedModuleNames = setOf("agent.doc.shared", "agent.doc.frontend", "agent.doc.backend")
                val moduleDescriptors = expectedModuleNames.associateWith { module ->
                    val moduleJar = archive.entries().asSequence()
                        .singleOrNull { it.name.endsWith("/lib/modules/$module.jar") }
                        ?: error("split distribution is missing exact module JAR $module.jar")
                    JarInputStream(archive.getInputStream(moduleJar)).use { jar ->
                        var content: String? = null
                        while (true) {
                            val entry = jar.nextJarEntry ?: break
                            if (entry.name == "$module.xml") {
                                content = jar.readBytes().toString(Charsets.UTF_8)
                                break
                            }
                        }
                        content ?: error("$module.jar is missing its root $module.xml descriptor")
                    }
                }
                val rootJarEntry = archive.entries().asSequence()
                    .firstOrNull { it.name.endsWith(".jar") && !it.name.contains("/modules/") && !it.name.contains("lazily-") }
                    ?: error("split distribution is missing the root plugin jar")
                val pluginXml = JarInputStream(archive.getInputStream(rootJarEntry)).use { jar ->
                    var content: String? = null
                    while (true) {
                        val entry = jar.nextJarEntry ?: break
                        if (entry.name == "META-INF/plugin.xml") {
                            content = jar.readBytes().toString(Charsets.UTF_8)
                            break
                        }
                    }
                    content ?: error("root plugin jar is missing META-INF/plugin.xml")
                }
                val sinceBuild = compatibilitySinceBuild.get()
                val untilBuild = compatibilityUntilBuild.get()
                check(pluginXml.contains("since-build=\"$sinceBuild\"")) {
                    "$sinceBuild since-build was not patched"
                }
                check(pluginXml.contains("until-build=\"$untilBuild\"")) {
                    "$sinceBuild until-build was not clamped to $untilBuild"
                }
                check(pluginXml.contains("<id>com.github.btakita.agent-doc</id>")) {
                    "split distribution changed the existing plugin ID"
                }

                val pluginDocument = parsePluginXml(pluginXml)
                val content = pluginDocument.documentElement.directChildren("content").singleOrNull()
                    ?: error("root descriptor must contain exactly one content element")
                val actualContentModules = content.directChildren("module").associate { module ->
                    module.getAttribute("name") to ContentModuleContract(
                        loading = module.getAttribute("loading").ifBlank { null },
                        requiredIfAvailable = module.getAttribute("required-if-available").ifBlank { null },
                    )
                }
                val expectedContentModules = mapOf(
                    "agent.doc.shared" to ContentModuleContract("required", null),
                    "agent.doc.frontend" to ContentModuleContract(null, "intellij.platform.frontend"),
                    "agent.doc.backend" to ContentModuleContract(null, "intellij.platform.backend"),
                )
                check(actualContentModules == expectedContentModules) {
                    "single-distribution role selectors changed: expected=$expectedContentModules actual=$actualContentModules"
                }

                val roleMatrix = mapOf(
                    "frontend" to setOf("intellij.platform.frontend"),
                    "backend" to setOf("intellij.platform.backend"),
                    "monolithic" to setOf("intellij.platform.frontend", "intellij.platform.backend"),
                )
                val expectedSelections = mapOf(
                    "frontend" to setOf("agent.doc.shared", "agent.doc.frontend"),
                    "backend" to setOf("agent.doc.shared", "agent.doc.backend"),
                    "monolithic" to expectedModuleNames,
                )
                roleMatrix.forEach { (role, availableCapabilities) ->
                    val selected = actualContentModules.filterValues { contract ->
                        contract.requiredIfAvailable == null || contract.requiredIfAvailable in availableCapabilities
                    }.keys
                    check(selected == expectedSelections.getValue(role)) {
                        "$role process selected wrong modules: expected=${expectedSelections.getValue(role)} actual=$selected"
                    }
                }

                val descriptorDependencies = moduleDescriptors.mapValues { (_, descriptor) ->
                    val document = parsePluginXml(descriptor)
                    document.documentElement.directChildren("dependencies")
                        .singleOrNull()
                        ?.directChildren("module")
                        ?.map { it.getAttribute("name") }
                        ?.toSet()
                        .orEmpty()
                }
                check(descriptorDependencies.getValue("agent.doc.shared").isEmpty()) {
                    "shared module must remain loadable in every process: $descriptorDependencies"
                }
                check(descriptorDependencies.getValue("agent.doc.frontend").containsAll(
                    setOf("intellij.platform.frontend", "agent.doc.shared"),
                )) { "frontend descriptor lost its side/shared dependencies: $descriptorDependencies" }
                check(descriptorDependencies.getValue("agent.doc.backend").containsAll(
                    setOf("intellij.platform.backend", "agent.doc.shared"),
                )) { "backend descriptor lost its side/shared dependencies: $descriptorDependencies" }
                check("intellij.platform.backend.split" !in descriptorDependencies.getValue("agent.doc.backend")) {
                    "backend descriptor must remain loadable in monolithic IDEs: $descriptorDependencies"
                }

                val updateChannelOwners = moduleDescriptors.mapValues { (_, descriptor) ->
                    val document = parsePluginXml(descriptor)
                    document.getElementsByTagName("action").let { actions ->
                        (0 until actions.length)
                            .mapNotNull { actions.item(it) as? Element }
                            .any { it.getAttribute("id") == "AgentDoc.UpdateChannel" }
                    }
                }
                check(updateChannelOwners.getValue("agent.doc.frontend")) {
                    "modular update-channel action must execute in the JetBrains Client: $updateChannelOwners"
                }
                check(!updateChannelOwners.getValue("agent.doc.backend")) {
                    "backend-local UpdateSettings cannot update the JetBrains Client: $updateChannelOwners"
                }

                val backendJar = archive.entries().asSequence()
                    .single { it.name.endsWith("/lib/modules/agent.doc.backend.jar") }
                val backendEntries = JarInputStream(archive.getInputStream(backendJar)).use { jar ->
                    generateSequence { jar.nextJarEntry }.map { it.name }.toSet()
                }
                val embeddedLazilyClasses = setOf(
                    "io/github/lazily/ThreadSafeContext.class",
                    "io/github/lazily/MergePolicy.class",
                    "io/github/lazily/OrSet.class",
                )
                check(backendEntries.containsAll(embeddedLazilyClasses)) {
                    "backend content module must embed its private Lazily runtime: " +
                        "missing=${embeddedLazilyClasses - backendEntries}"
                }
                check(names.none { entry ->
                    entry.substringAfterLast('/').startsWith("lazily-") && entry.endsWith(".jar")
                }) {
                    "Lazily must not be packaged as a root plugin library outside the backend module"
                }
            }
        }
    }

    register("verifySplitModeSandboxes") {
        group = "verification"
        description = "Prepare and verify BOTH backend and JetBrains Client split-mode sandboxes"
        dependsOn("prepareSandbox_runIdeBackend", "prepareSandbox_runIdeFrontend")
        doLast {
            listOf("prepareSandbox_runIdeBackend", "prepareSandbox_runIdeFrontend").forEach { taskName ->
                val jars = named(taskName).get().outputs.files.files
                    .asSequence()
                    .filter { it.exists() }
                    .flatMap { output -> output.walkTopDown().asSequence() }
                    .filter { it.isFile && it.extension == "jar" }
                    .toList()
                val jarNames = jars.map { it.name }.toSet()
                val expectedModuleJars = setOf(
                    "agent.doc.shared.jar",
                    "agent.doc.frontend.jar",
                    "agent.doc.backend.jar",
                )
                check(jarNames.containsAll(expectedModuleJars)) {
                    "$taskName did not receive the complete single-distribution module set: " +
                        "missing=${expectedModuleJars - jarNames}"
                }
                val backendJars = jars.filter { it.name == "agent.doc.backend.jar" }
                check(backendJars.isNotEmpty()) { "$taskName is missing agent.doc.backend.jar" }
                backendJars.forEach { backendJar ->
                    val backendEntries = JarInputStream(backendJar.inputStream()).use { jar ->
                        generateSequence { jar.nextJarEntry }.map { it.name }.toSet()
                    }
                    check("io/github/lazily/ThreadSafeContext.class" in backendEntries) {
                        "$taskName backend module cannot resolve its embedded Lazily runtime"
                    }
                }
                check(jars.any { jarFile ->
                    JarInputStream(jarFile.inputStream()).use { jar ->
                        generateSequence { jar.nextJarEntry }
                            .firstOrNull { it.name == "META-INF/plugin.xml" }
                            ?.let { jar.readBytes().toString(Charsets.UTF_8) }
                            ?.contains("<id>com.github.btakita.agent-doc</id>") == true
                    }
                }) {
                    "$taskName did not install the Agent Doc plugin into its sandbox"
                }
            }
        }
    }
}
