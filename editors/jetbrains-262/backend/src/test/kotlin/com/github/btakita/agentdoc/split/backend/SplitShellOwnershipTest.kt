package com.github.btakita.agentdoc.split.backend

import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import kotlin.io.path.extension
import kotlin.io.path.invariantSeparatorsPathString
import kotlin.io.path.name
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * GH #339 R1, modular extensions and Fleet RPC: the backend's platform-held shell files (the RPC
 * provider, the RPC implementation, the ingress project service and its SPI) name no backend
 * implementation type. They reach the ingress only through `SurfaceIngressFactory`, which the
 * active implementation generation serves.
 */
class SplitShellOwnershipTest {
    private val moduleRoot: Path =
        listOf(Paths.get("."), Paths.get("editors/jetbrains-262"), Paths.get(".."))
            .map { it.toAbsolutePath().normalize() }
            .first { Files.exists(it.resolve("shell-sources.txt")) && Files.exists(it.resolve("backend")) }

    private val backendSources: Path =
        moduleRoot.resolve("backend/src/main/kotlin/com/github/btakita/agentdoc/split/backend")

    /** The `[shell]` entries of `shell-sources.txt` under the backend source tree. */
    private fun shellFiles(): Set<String> =
        Files.readAllLines(moduleRoot.resolve("shell-sources.txt"))
            .map(String::trim)
            .filter { it.startsWith("backend/src/main/") && it.endsWith(".kt") }
            .map { it.substringAfterLast('/') }
            .toSet()

    private fun sources(): List<Path> = Files.list(backendSources).use { stream ->
        stream.filter { it.extension == "kt" }.sorted().toList()
    }

    @Test
    fun `backend shell files name no backend implementation type`() {
        val shell = shellFiles()
        assertEquals(
            setOf(
                "SurfaceSnapshotRemoteApiProvider.kt",
                "BackendSurfaceSnapshotRpcApi.kt",
                "AuthenticatedSurfaceIngressService.kt",
                "SurfaceIngressSpi.kt",
            ),
            shell,
        )
        val declaration = Regex(
            """^(?:internal |private )?(?:abstract |open |data |sealed |enum |fun )*(?:class|object|interface) (\w+)""",
            RegexOption.MULTILINE,
        )
        val implTypes = sources().filter { it.name !in shell }
            .flatMap { file -> declaration.findAll(file.readText()).map { it.groupValues[1] }.toList() }
            .toSet()
        assertTrue("AuthenticatedSurfaceIngressState" in implTypes, "the lease state machine is implementation")
        val references = sources().filter { it.name in shell }.flatMap { file ->
            val code = file.readText().lines()
                .filterNot { it.trimStart().startsWith("*") || it.trimStart().startsWith("/*") || it.trimStart().startsWith("//") }
                .joinToString("\n")
                .replace(Regex(""""[^"\n]*""""), "\"\"")
            implTypes.filter { Regex("""\b$it\b""").containsMatchIn(code) }
                .map { "${file.invariantSeparatorsPathString.substringAfterLast('/')} -> $it" }
        }
        assertEquals(emptyList(), references, "backend shell references to implementation types")
    }

    @Test
    fun `the ingress service delegates and declares nothing else`() {
        val service = backendSources.resolve("AuthenticatedSurfaceIngressService.kt").readText()
        assertTrue(service.contains("moduleService(SurfaceIngressFactory::class.java)"))
        val declared = Regex("""^(?:internal |private )?(?:data |enum )?(?:class|object|interface) (\w+)""", RegexOption.MULTILINE)
            .findAll(service).map { it.groupValues[1] }.toList()
        assertEquals(listOf("AuthenticatedSurfaceIngressService"), declared)
    }
}
