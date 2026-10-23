plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "0.9.0"
}

rootProject.name = "agent-doc-jetbrains"

// #lzpkgwire / S5: ordinary builds resolve the version pinned in build.gradle.kts.
// A checkout that merely happens to sit beside lazily-kt must not silently replace
// that release: an old or dirty sibling made the 0.46.0 consumer suite exercise
// unrelated source and fail its generation-fence proof. Development against local
// Lazily source is therefore an explicit opt-in, matching the modular plugin build.
// The override also lets dependency-update verification exercise an exact source tag
// without changing the operator's ordinary sibling checkout.
System.getenv("AGENT_DOC_LAZILY_KT_DIR")?.let { sourceDir ->
    val lazilyKt = file(sourceDir)
    check(lazilyKt.resolve("settings.gradle.kts").isFile || lazilyKt.resolve("settings.gradle").isFile) {
        "AGENT_DOC_LAZILY_KT_DIR must name a lazily-kt Gradle build: ${lazilyKt.path}"
    }
    val lazilySpecProto = lazilyKt.parentFile.resolve("lazily-spec/proto")
    check(lazilySpecProto.isDirectory) {
        "AGENT_DOC_LAZILY_KT_DIR requires sibling lazily-spec/proto at ${lazilySpecProto.path}"
    }
    includeBuild(lazilyKt) {
        dependencySubstitution {
            substitute(module("io.github.lazily:lazily")).using(project(":"))
        }
    }
}
