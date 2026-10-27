import java.util.EnumSet
import java.util.jar.JarInputStream
import java.util.zip.ZipFile
import org.jetbrains.intellij.platform.gradle.tasks.VerifyPluginTask
import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType

plugins {
    id("java")
    // Kotlin 2.0 (K2) to consume lazily-kt's Kotlin 2.0 metadata (#lzpkgwire).
    // Aligns with the Kotlin 2.0 runtime bundled from IntelliJ 2024.2 onward.
    id("org.jetbrains.kotlin.jvm") version "2.0.21"
    id("org.jetbrains.intellij.platform") version "2.11.0"
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

val compatibilitySinceBuild = providers.gradleProperty("compatibilitySinceBuild").orElse("242")
val compatibilityUntilBuild = providers.gradleProperty("compatibilityUntilBuild").orElse("261.*")
val packageName = providers.gradleProperty("pluginName").orElse("agent-doc-jetbrains")

repositories {
    // mavenLocal first so a locally-published lazily-kt (publishToMavenLocal) or the
    // composite-build substitution resolves before Maven Central / GitHub Packages
    // in a standalone plugin checkout (#lzpkgwire / S5).
    mavenLocal()
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdeaCommunity(providers.gradleProperty("platformVersion").get())
        bundledPlugin("org.jetbrains.plugins.terminal")
    }
    // S5: the canonical lazily reactive StateGraphMirror. In the monorepo the
    // composite build in settings.gradle.kts substitutes this with the sibling
    // source build; standalone it resolves from mavenLocal / GitHub Packages.
    //
    // Exclude every transitive the IntelliJ platform already provides at runtime
    // so ONLY lazily-kt-<v>.jar lands in the plugin zip's lib/. JetBrains
    // explicitly forbids bundling kotlinx-coroutines (it breaks the IDE coroutine
    // dispatcher); kotlin-stdlib, kotlinx-serialization, JetBrains annotations,
    // and jna are all platform-provided and were NOT bundled pre-change, so keep
    // them out of the zip to avoid classloader conflicts and regressions.
    implementation("io.github.lazily:lazily:0.46.0") {
        exclude(group = "org.jetbrains.kotlinx")
        exclude(group = "org.jetbrains.kotlin")
        exclude(group = "org.jetbrains", module = "annotations")
        exclude(group = "net.java.dev.jna")
    }
    // lazily-kt's @Serializable wire classes (WireSnapshot etc., constructed by
    // StateGraphMirror.applyMessage) need the kotlinx-serialization runtime to
    // class-load. The IDE provides it at runtime (so it is compile/test-only, never
    // bundled); unit tests run OUTSIDE the IDE so they need it on the test classpath.
    compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    // The IDE supplies Gson in production; the standalone native-plugin harness
    // needs the same API on its non-IDE JavaExec runtime classpath.
    testRuntimeOnly("com.google.code.gson:gson:2.11.0")
    testImplementation("junit:junit:4.13.2")
}

java {
    // JBR 21 (IntelliJ 2024.2+); matches lazily-kt's JVM 21 bytecode (#lzpkgwire).
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21)
    }
}

// GH #339 R1: the shell/implementation digests the activation receipt reports.
extra["shellIdentityManifests"] = listOf(layout.projectDirectory.file("shell-sources.txt").asFile)
apply(from = "shell-identity.gradle.kts")

intellijPlatform {
    buildSearchableOptions = false
    // One source tree produces the classic 242-261 package and the separately
    // versioned 263 package. The exact-262 split package remains isolated in
    // editors/jetbrains-262 because it uses build-line-specific internal APIs.
    projectName = packageName
    pluginVerification {
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
            create(IntelliJPlatformType.IntellijIdeaCommunity, providers.gradleProperty("platformVersion"))
        }
    }
}

tasks {
    withType<JavaCompile>().configureEach {
        options.compilerArgs.addAll(listOf("--add-modules", "jdk.attach"))
    }

    jar {
        archiveBaseName.set(packageName)
        manifest {
            attributes(
                "Main-Class" to "com.github.btakita.agentdoc.JetBrainsPluginUpgradeBootstrap",
                "Agent-Class" to "com.github.btakita.agentdoc.JetBrainsPluginUpgradeBootstrap",
            )
        }
    }

    // The IntelliJ Platform plugin replaces the base Jar with `composedJar`
    // after bytecode instrumentation. Carry the attach-agent entry points onto
    // that final artifact too: this is the Jar shipped in buildPlugin's ZIP.
    named<org.gradle.jvm.tasks.Jar>("composedJar") {
        archiveBaseName.set(packageName)
        manifest {
            attributes(
                "Main-Class" to "com.github.btakita.agentdoc.JetBrainsPluginUpgradeBootstrap",
                "Agent-Class" to "com.github.btakita.agentdoc.JetBrainsPluginUpgradeBootstrap",
            )
        }
    }

    register<JavaExec>("runCrossEditorHarness") {
        group = "verification"
        description = "Run the production JetBrains CRDT/native endpoint for cross-editor SimWorld"
        dependsOn(testClasses)
        classpath =
            sourceSets["test"].runtimeClasspath +
            configurations["intellijPlatformTestClasspath"]
        mainClass.set("com.github.btakita.agentdoc.CrossEditorHarnessMainKt")
        standardInput = System.`in`
        doFirst {
            // IntelliJ's launcher normally provides this property. The headless
            // harness still uses the IDE-bundled JNA classes and native dispatch
            // library; locate that exact library from the resolved platform.
            val platformLoader =
                configurations["intellijPlatformTestClasspath"].files.first {
                    it.name == "platform-loader.jar"
                }
            val jnaDispatch =
                platformLoader.parentFile
                    .resolve("jna")
                    .walkTopDown()
                    .first { it.name.startsWith("libjnidispatch") || it.name == "jnidispatch.dll" }
            systemProperty("jna.boot.library.path", jnaDispatch.parentFile.absolutePath)
        }
    }

    patchPluginXml {
        sinceBuild.set(compatibilitySinceBuild)
        untilBuild.set(compatibilityUntilBuild)
        changeNotes.set("""
            <ul>
                <li>Initial release</li>
                <li>Submit current markdown file to agent-doc via terminal hotkey</li>
            </ul>
        """.trimIndent())
    }

    register("verifyClassicArtifact") {
        group = "verification"
        description = "Verify the selected classic-source compatibility-ranged ZIP"
        dependsOn("buildPlugin")
        doLast {
            val forbiddenImports = projectDir.resolve("src/main").walkTopDown()
                .filter { it.isFile && (it.extension == "kt" || it.extension == "java") }
                .flatMap { source ->
                    source.readLines().asSequence()
                        .filter { line ->
                            line.startsWith("import com.intellij.codeInsight.daemon.impl.EditorTracker") ||
                                line.startsWith("import com.intellij.openapi.client.ClientKind") ||
                                line.startsWith("import com.intellij.openapi.fileEditor.ClientFileEditorManager") ||
                                line.startsWith("import com.intellij.openapi.wm.RegisterToolWindowTask") ||
                                line.startsWith("import com.intellij.openapi.wm.impl.ToolWindowManagerImpl") ||
                                line.startsWith("import com.intellij.toolWindow.ToolWindowPane") ||
                                line.startsWith("import com.intellij.ide.plugins.DynamicPlugins") ||
                                line.startsWith("import com.intellij.ide.plugins.IdeaPluginDescriptorImpl") ||
                                line.startsWith("import com.intellij.ide.plugins.PluginInstaller")
                        }
                        .map { source.relativeTo(projectDir).invariantSeparatorsPath to it }
                }.toList()
            check(forbiddenImports.isEmpty()) {
                "Marketplace-rejected private IntelliJ API imports reappeared: $forbiddenImports"
            }
            val zip = layout.buildDirectory.dir("distributions").get().asFile
                .listFiles()
                .orEmpty()
                .singleOrNull {
                    it.name == "${packageName.get()}-${project.version}.zip"
                }
                ?: error("missing exact classic plugin distribution for ${project.version}")
            ZipFile(zip).use { archive ->
                val rootJar = archive.entries().asSequence()
                    .singleOrNull {
                        it.name.endsWith(".jar") &&
                            it.name.substringAfterLast('/').startsWith("agent-doc-jetbrains-")
                    }
                    ?: error("classic distribution is missing its root plugin JAR")
                val pluginXml = JarInputStream(archive.getInputStream(rootJar)).use { jar ->
                    var content: String? = null
                    while (true) {
                        val entry = jar.nextJarEntry ?: break
                        if (entry.name == "META-INF/plugin.xml") {
                            content = jar.readBytes().toString(Charsets.UTF_8)
                            break
                        }
                    }
                    content ?: error("classic root plugin JAR is missing META-INF/plugin.xml")
                }
                check(pluginXml.contains("since-build=\"${compatibilitySinceBuild.get()}\"")) {
                    "package since-build was not patched"
                }
                check(pluginXml.contains("until-build=\"${compatibilityUntilBuild.get()}\"")) {
                    "package until-build was not patched"
                }
                check(pluginXml.contains("<id>com.github.btakita.agent-doc</id>")) {
                    "classic distribution changed the shared Marketplace plugin ID"
                }
            }
        }
    }

    signPlugin {
        val certDir = layout.projectDirectory.dir("certificate")
        certificateChain.set(providers.environmentVariable("CERTIFICATE_CHAIN")
            .orElse(providers.fileContents(certDir.file("chain.crt")).asText))
        privateKey.set(providers.environmentVariable("PRIVATE_KEY")
            .orElse(providers.fileContents(certDir.file("private.pem")).asText))
        password.set(providers.environmentVariable("PRIVATE_KEY_PASSWORD")
            .orElse(provider { "" }))
    }

    // Always sign after building
    named("signPlugin") {
        dependsOn("buildPlugin")
    }

    publishPlugin {
        token.set(providers.environmentVariable("PUBLISH_TOKEN"))
    }
}
