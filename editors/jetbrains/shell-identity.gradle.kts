// GH #339 R1: build-time shell/implementation identity (editors/jetbrains/SPEC.md
// "Restart-free implementation generations"). Applied by the classic build and by the
// modular `agent.doc.shared` module, each after setting `extra["shellIdentityManifests"]`
// to the `shell-sources.txt` files that describe its package.
//
// Writes `com/github/btakita/agentdoc/shell/receipt/shell-identity.properties` into the main
// resources, read at runtime by `ShellIdentity`:
//
//   shell_digest=<sha256 over the shell file set>
//   impl_digest=<sha256 over the implementation file set>
//
// Each digest hashes every member's manifest-relative path and bytes, in path order,
// so it is stable across checkouts and changes only when a member changes.

import java.security.MessageDigest

@Suppress("UNCHECKED_CAST")
val shellIdentityManifests = (extra["shellIdentityManifests"] as List<File>)

val shellIdentityOutput = layout.buildDirectory.dir("generated/agent-doc-shell-identity")

/** Build output never belongs to either set, even under a matched directory prefix. */
val shellIdentityExcludes = listOf("**/build/**", "**/.gradle/**", "**/out/**")

fun shellIdentitySections(manifest: File): Map<String, List<String>> {
    val sections = linkedMapOf<String, MutableList<String>>()
    var current: MutableList<String>? = null
    manifest.readLines().map(String::trim).filter { it.isNotEmpty() && !it.startsWith("#") }.forEach { line ->
        if (line.startsWith("[") && line.endsWith("]")) {
            current = sections.getOrPut(line.removeSurrounding("[", "]")) { mutableListOf() }
        } else {
            (current ?: error("$manifest: entry '$line' precedes any [shell]/[impl] section")).add(line)
        }
    }
    check(sections.keys == setOf("shell", "impl")) { "$manifest must have exactly [shell] and [impl]: ${sections.keys}" }
    return sections
}

fun shellIdentityMatches(entries: List<String>, path: String): Boolean =
    entries.any { entry -> if (entry.endsWith("/")) path.startsWith(entry) else path == entry }

fun shellIdentityPattern(entry: String): String = if (entry.endsWith("/")) "$entry**" else entry

val generateShellIdentity = tasks.register("generateShellIdentity") {
    group = "build"
    description = "Digest the GH #339 shell and implementation file sets into a resource"
    val manifests = shellIdentityManifests
    manifests.forEach { manifest ->
        val sections = shellIdentitySections(manifest)
        inputs.file(manifest)
        inputs.files(
            fileTree(manifest.parentFile) {
                (sections.getValue("shell") + sections.getValue("impl")).forEach { include(shellIdentityPattern(it)) }
                exclude(shellIdentityExcludes)
            },
        )
    }
    outputs.dir(shellIdentityOutput)

    doLast {
        val shell = MessageDigest.getInstance("SHA-256")
        val impl = MessageDigest.getInstance("SHA-256")
        val counts = intArrayOf(0, 0)
        manifests.forEach { manifest ->
            val sections = shellIdentitySections(manifest)
            val base = manifest.parentFile
            val label = base.name
            val members = fileTree(base) {
                (sections.getValue("shell") + sections.getValue("impl")).forEach { include(shellIdentityPattern(it)) }
                exclude(shellIdentityExcludes)
            }.files.map { it.relativeTo(base).invariantSeparatorsPath to it }.sortedBy { it.first }
            for ((path, file) in members) {
                val digest = when {
                    shellIdentityMatches(sections.getValue("shell"), path) -> shell.also { counts[0]++ }
                    shellIdentityMatches(sections.getValue("impl"), path) -> impl.also { counts[1]++ }
                    else -> continue
                }
                digest.update("$label/$path".toByteArray())
                digest.update(0)
                digest.update(file.readBytes())
                digest.update(0)
            }
        }
        check(counts[0] > 0 && counts[1] > 0) { "shell identity matched no files: shell=${counts[0]} impl=${counts[1]}" }
        fun hex(digest: MessageDigest) = digest.digest().joinToString("") { "%02x".format(it) }
        val outputDir = shellIdentityOutput.get().asFile
        outputDir.deleteRecursively()
        val target = outputDir
            .resolve("com/github/btakita/agentdoc/shell/receipt/shell-identity.properties")
        target.parentFile.mkdirs()
        target.writeText(
            "shell_digest=${hex(shell)}\nimpl_digest=${hex(impl)}\n" +
                "shell_files=${counts[0]}\nimpl_files=${counts[1]}\n",
        )
    }
}

extensions.getByType<SourceSetContainer>().named("main") {
    resources.srcDir(shellIdentityOutput)
}

tasks.named("processResources") {
    dependsOn(generateShellIdentity)
}
