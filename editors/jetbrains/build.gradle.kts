plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "1.9.25"
    id("org.jetbrains.intellij.platform") version "2.11.0"
}

group = providers.gradleProperty("pluginGroup").get()
version = providers.gradleProperty("pluginVersion").get()

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdeaCommunity(providers.gradleProperty("platformVersion").get())
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

intellijPlatform {
    buildSearchableOptions = false
}

tasks {
    patchPluginXml {
        sinceBuild.set("241")
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
