@file:Suppress("UnstableApiUsage")

import org.jetbrains.intellij.platform.gradle.extensions.intellijPlatform

pluginManagement {
    repositories {
        mavenCentral()
        gradlePluginPortal()
        maven("https://packages.jetbrains.team/maven/p/ij/intellij-dependencies/")
    }
    plugins {
        id("rpc") version "2.4.0-RC-0.1"
        id("org.jetbrains.kotlin.jvm") version "2.4.0"
        id("org.jetbrains.kotlin.plugin.serialization") version "2.4.0"
    }
}

plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
    id("org.jetbrains.intellij.platform.settings") version "2.19.0"
}

// The Gradle plugin derives module JAR names as <rootProject.name>.<subproject>.
// This must match each Plugin Model v2 module/descriptor name exactly.
rootProject.name = "agent.doc"

dependencyResolutionManagement {
    repositories {
        mavenLocal()
        mavenCentral()
        intellijPlatform {
            defaultRepositories()
        }
    }
}

include("shared")
include("frontend")
include("backend")

// CI normally materializes the pinned release in mavenLocal. An explicit
// source checkout provides the same exact-version verification seam without a
// machine-local publication side effect.
System.getenv("AGENT_DOC_LAZILY_KT_DIR")?.let { sourceDir ->
    val lazilyKt = file(sourceDir)
    check(lazilyKt.resolve("settings.gradle.kts").isFile) {
        "AGENT_DOC_LAZILY_KT_DIR must name a lazily-kt Gradle build: ${lazilyKt.path}"
    }
    includeBuild(lazilyKt) {
        dependencySubstitution {
            substitute(module("io.github.lazily:lazily")).using(project(":"))
        }
    }
}
