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
val lazilyKt = file("../../../lazily-kt")
if (lazilyKt.exists()) {
    includeBuild(lazilyKt) {
        dependencySubstitution {
            substitute(module("io.github.lazily:lazily")).using(project(":"))
        }
    }
}
