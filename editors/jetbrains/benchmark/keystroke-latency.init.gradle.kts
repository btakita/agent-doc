// `#jbshrinkrestarts`: the JetBrains per-keystroke native-boundary latency gate.
//
// An init script, not a build.gradle.kts task: the benchmark is not part of the plugin, so
// changing it must not change the plugin's source digest (`pluginSourceDigest`) and force a
// plugin generation, which on the modular package is an IDE restart. Run it with
// `make jetbrains-keystroke-bench`, or:
//
//   AGENT_DOC_BENCH_BIN_DIR=<dir with agent-doc and libagent_doc> \
//     ./gradlew -I benchmark/keystroke-latency.init.gradle.kts runKeystrokeLatencyBenchmark
//
// KeystrokeLatencyBenchmarkMain.kt documents the cases and the AGENT_DOC_BENCH_* knobs.
rootProject {
    afterEvaluate {
        val testSourceSet =
            extensions.getByType(org.gradle.api.tasks.SourceSetContainer::class.java).getByName("test")
        val platformTestClasspath = configurations.getByName("intellijPlatformTestClasspath")
        tasks.register("runKeystrokeLatencyBenchmark", JavaExec::class.java) {
            group = "verification"
            description = "Measure the JetBrains plugin's per-keystroke native-boundary latency"
            dependsOn("testClasses")
            classpath = testSourceSet.runtimeClasspath + platformTestClasspath
            mainClass.set("com.github.btakita.agentdoc.KeystrokeLatencyBenchmarkMainKt")
            doFirst {
                // IntelliJ's launcher normally provides this property; locate the IDE-bundled
                // JNA dispatch library exactly as runCrossEditorHarness does.
                val platformLoader = platformTestClasspath.files.first { it.name == "platform-loader.jar" }
                val jnaDispatch =
                    platformLoader.parentFile
                        .resolve("jna")
                        .walkTopDown()
                        .first { it.name.startsWith("libjnidispatch") || it.name == "jnidispatch.dll" }
                systemProperty("jna.boot.library.path", jnaDispatch.parentFile.absolutePath)
                System.getenv("AGENT_DOC_BENCH_BIN_DIR")?.takeIf { it.isNotBlank() }?.let { binDir ->
                    environment("PATH", "$binDir${java.io.File.pathSeparator}${System.getenv("PATH")}")
                }
            }
        }
    }
}
