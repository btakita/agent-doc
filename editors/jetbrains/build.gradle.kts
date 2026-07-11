plugins {
    id("java")
    // Kotlin 2.0 (K2) to consume lazily-kt's Kotlin 2.0 metadata (#lzpkgwire).
    // Aligns with the Kotlin 2.0 runtime bundled from IntelliJ 2024.2 onward.
    id("org.jetbrains.kotlin.jvm") version "2.0.21"
    id("org.jetbrains.intellij.platform") version "2.11.0"
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

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
    implementation("io.github.lazily:lazily:0.19.0") {
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

intellijPlatform {
    buildSearchableOptions = false
}

tasks {
    patchPluginXml {
        sinceBuild.set("242")
        untilBuild.set(provider { null })
        changeNotes.set("""
            <ul>
                <li>Initial release</li>
                <li>Submit current markdown file to agent-doc via terminal hotkey</li>
            </ul>
        """.trimIndent())
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
