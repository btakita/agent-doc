import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType
import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.tasks.aware.SplitModeAware
import org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask
import java.util.EnumSet
import java.util.jar.JarInputStream
import java.util.zip.ZipFile

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
    projectName = "agent-doc-jetbrains-262"
    buildSearchableOptions = false
    splitMode = true
    pluginInstallationTarget = SplitModeAware.PluginInstallationTarget.BOTH

    pluginVerification {
        // The integrated classic backend intentionally preserves its existing implementation and
        // verifier warnings. Keep compatibility, dependency, structure, and validity failures as
        // hard gates while reporting (but not reclassifying) inherited internal/override warnings.
        failureLevel.set(
            EnumSet.of(
                VerifyPluginTask.FailureLevel.COMPATIBILITY_PROBLEMS,
                VerifyPluginTask.FailureLevel.MISSING_DEPENDENCIES,
                VerifyPluginTask.FailureLevel.INVALID_PLUGIN,
                VerifyPluginTask.FailureLevel.PLUGIN_STRUCTURE_WARNINGS,
            ),
        )
        ides {
            create(IntelliJPlatformType.IntellijIdeaUltimate, platformVersion)
        }
    }
}

tasks {
    patchPluginXml {
        sinceBuild.set("262")
        untilBuild.set("262.*")
    }

    register("verifySplitArtifact") {
        group = "verification"
        description = "Verify that the 262 ZIP contains all split plugin modules and range metadata"
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
                "exact-262 internal UI imports escaped the designated adapter: $internalImports"
            }
            val zip = layout.buildDirectory.dir("distributions").get().asFile
                .listFiles()
                .orEmpty()
                .singleOrNull { it.extension == "zip" && it.name.contains(project.version.toString()) }
                ?: error("missing unique split plugin distribution")
            ZipFile(zip).use { archive ->
                val names = archive.entries().asSequence().map { it.name }.toList()
                listOf("agent.doc.shared", "agent.doc.frontend", "agent.doc.backend").forEach { module ->
                    val moduleJar = archive.entries().asSequence()
                        .singleOrNull { it.name.endsWith("/lib/modules/$module.jar") }
                        ?: error("split distribution is missing exact module JAR $module.jar")
                    val hasDescriptor = JarInputStream(archive.getInputStream(moduleJar)).use { jar ->
                        generateSequence { jar.nextJarEntry }.any { it.name == "$module.xml" }
                    }
                    check(hasDescriptor) { "$module.jar is missing its root $module.xml descriptor" }
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
                check(pluginXml.contains("since-build=\"262\"")) { "262 since-build was not patched" }
                check(pluginXml.contains("until-build=\"262.*\"")) { "262 until-build was not clamped" }
                check(pluginXml.contains("<id>com.github.btakita.agent-doc</id>")) {
                    "split distribution changed the existing plugin ID"
                }
                check(pluginXml.contains("agent.doc.shared")) { "shared content module missing" }
                check(pluginXml.contains("agent.doc.frontend")) { "frontend content module missing" }
                check(pluginXml.contains("agent.doc.backend")) { "backend content module missing" }
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
