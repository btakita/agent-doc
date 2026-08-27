plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "0.9.0"
}

rootProject.name = "agent-doc-jetbrains"

// #lzpkgwire / S5: consume the sibling lazily-kt reactive core as a composite
// build when it is present (agent-loop monorepo checkout) so local edits to the
// canonical StateGraphMirror are exercised without a republish. The plugin is a
// public standalone repo, so this is strictly conditional: when the sibling is
// absent (standalone checkout) the `io.github.lazily:lazily` dependency resolves
// from mavenLocal / GitHub Packages instead.
//
// `#lzktemptydir`: the guard must prove the sibling is a REAL Gradle build, not
// merely that the directory exists. An uninitialized git submodule is an
// existing EMPTY directory, so `exists()` alone returns true, `includeBuild`
// substitutes `io.github.lazily:lazily` with a build that has no projects, and
// Gradle fails with the opaque "No matching variant of project :lazily-kt was
// found ... No variants exist." A standalone checkout has no directory at all,
// which is why only a PARTIAL monorepo checkout hit this — observed in the
// agent-loop superproject CI, whose submodule init listed every sibling except
// `src/lazily-kt`. Keying on the settings file makes the fallback identical for
// "absent" and "present but uninitialized".
val lazilyKt = file("../../../lazily-kt")
val lazilyKtIsBuild = lazilyKt.resolve("settings.gradle.kts").isFile ||
    lazilyKt.resolve("settings.gradle").isFile
if (lazilyKtIsBuild) {
    val lazilySpecProto = file("../../../lazily-spec/proto")
    check(lazilySpecProto.isDirectory) {
        "lazily-kt development build requires sibling lazily-spec/proto at ${lazilySpecProto.path}; " +
            "initialize the lazily-spec sibling or remove the partial lazily-kt checkout"
    }
    includeBuild(lazilyKt) {
        dependencySubstitution {
            substitute(module("io.github.lazily:lazily")).using(project(":"))
        }
    }
}
