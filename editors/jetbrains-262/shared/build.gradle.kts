dependencies {
    intellijPlatform {
        bundledModule("intellij.platform.rpc")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-core-jvm:1.9.0")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json-jvm:1.9.0")
    }

    testImplementation(kotlin("test"))
    testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
}

// GH #339 R1: the activation receipt and shell identity are one JDK-only classic source file.
// The shared module compiles it so the frontend and backend content modules read one class
// (and one JVM instance id); the backend excludes its classic copy.
val sharedShellSources = layout.buildDirectory.dir("generated/agent-doc-shared-shell-sources")
val copySharedShellSources = tasks.register<Sync>("copySharedShellSources") {
    from("../../jetbrains/src/main/kotlin") {
        include("com/github/btakita/agentdoc/shell/receipt/**")
    }
    into(sharedShellSources)
}
sourceSets.main {
    kotlin.srcDir(sharedShellSources)
}
tasks.named("compileKotlin") {
    dependsOn(copySharedShellSources)
}

extra["shellIdentityManifests"] = listOf(
    rootProject.layout.projectDirectory.file("../jetbrains/shell-sources.txt").asFile,
    rootProject.layout.projectDirectory.file("shell-sources.txt").asFile,
)
apply(from = "../../jetbrains/shell-identity.gradle.kts")
