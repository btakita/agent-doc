@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split

import fleet.rpc.RemoteApiDescriptor
import fleet.rpc.RemoteKind
import fleet.rpc.remoteApiDescriptor
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.util.TreeMap
import kotlin.io.path.isRegularFile
import kotlin.io.path.readText
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.fail
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.SerialKind
import kotlinx.serialization.descriptors.StructureKind

/**
 * GH #435: the Fleet RPC wire contract between the JetBrains Client frontend and the backend is
 * frozen. A Client's frontend is shell code that only a Client plugin update replaces, so every
 * backend must keep serving every Client shell already shipped. 89b1df09af (0.2.1052) added a
 * method to the shared [SurfaceSnapshotRpcApi]; no test noticed, and a 0.2.1052 backend stranded a
 * 0.2.1046 Client on Coder.
 *
 * The test renders each shared `@Rpc` API as Fleet RPC sees it (method names, parameter names, and
 * the serial shape of every message an older peer decodes with `ignoreUnknownKeys = false`) and
 * requires it to equal `shared-rpc-contract.baseline`. A change must be declared there: bump
 * `revision`, name the `shell_bump` plugin version that ships it, and state the
 * `compatibility_plan` for Clients and backends already in the field. Prefer a new, additive
 * `RemoteApi` over changing a shipped one.
 */
class SharedRpcContractGuardTest {
    private val apis: List<Pair<String, RemoteApiDescriptor<*>>> = listOf(
        "com.github.btakita.agentdoc.split.FrontendReleaseRpcApi" to remoteApiDescriptor<FrontendReleaseRpcApi>(),
        "com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi" to remoteApiDescriptor<SurfaceSnapshotRpcApi>(),
    )

    @Test
    fun theSharedRpcContractEqualsItsDeclaredBaseline() {
        val baseline = Baseline.read()
        val actual = render()
        if (actual != baseline.fingerprint) {
            val out = moduleRoot().resolve("shared/build/shared-rpc-contract.actual")
            Files.createDirectories(out.parent)
            Files.writeString(out, actual)
            fail(
                "The shared Fleet RPC contract changed (GH #435). Shipped JetBrains Clients cannot decode " +
                    "a changed message or call a renamed method; add a new RemoteApi instead. If the change is " +
                    "intended and compatible, replace the fingerprint in " +
                    "shared/src/test/resources/shared-rpc-contract.baseline with $out, bump `revision`, set " +
                    "`shell_bump` to the plugin version that ships it, and state the `compatibility_plan`.\n" +
                    unifiedDiff(baseline.fingerprint, actual),
            )
        }
    }

    @Test
    fun theBaselineDeclaresItsShellBumpAndCompatibilityPlan() {
        val baseline = Baseline.read()
        assertTrue(baseline.revision > 0, "revision")
        assertTrue(baseline.compatibilityPlan.length >= 40, "compatibility_plan must state the plan")
        val pluginVersion = Files.readAllLines(moduleRoot().resolve("gradle.properties"))
            .first { it.startsWith("pluginVersion") }.substringAfter('=').trim()
        val order = compareAgentDocPluginVersions(baseline.shellBump, pluginVersion)
        assertTrue(order != null && order <= 0, "shell_bump ${baseline.shellBump} must be a shipped or this build's version $pluginVersion")
    }

    /** Every `@Rpc` interface in the shared module is covered, so a new one cannot skip the guard. */
    @Test
    fun everySharedRpcApiIsGuarded() {
        val sources = moduleRoot().resolve("shared/src/main/kotlin")
        val declared = Files.walk(sources).use { paths ->
            paths.filter { it.isRegularFile() && it.toString().endsWith(".kt") }.toList()
        }.flatMap { file ->
            val code = file.readText()
            val pkg = Regex("""^package ([\w.]+)""", RegexOption.MULTILINE).find(code)!!.groupValues[1]
            Regex("""@Rpc\s+interface (\w+)""").findAll(code).map { "$pkg.${it.groupValues[1]}" }.toList()
        }.toSortedSet()
        assertEquals(declared, apis.map { it.first }.toSortedSet())
    }

    private fun render(): String {
        val types = TreeMap<String, String>()
        val lines = mutableListOf<String>()
        for ((fqn, descriptor) in apis) {
            lines += "api $fqn"
            val methods = Class.forName(fqn).declaredMethods.filterNot { it.isSynthetic }.map { it.name }.toSortedSet()
            for (method in methods) {
                val signature = descriptor.getSignature(method)
                val parameters = signature.parameters.joinToString(", ") { parameter ->
                    "${parameter.parameterName}: ${kind(parameter.parameterKind, types)}"
                }
                lines += "  $method($parameters): ${kind(signature.returnType, types)}"
            }
        }
        types.values.forEach { lines += it }
        return lines.joinToString("\n", postfix = "\n")
    }

    private fun kind(kind: RemoteKind, types: MutableMap<String, String>): String = when (kind) {
        is RemoteKind.Data -> reference(kind.serializer.descriptor, types)
        is RemoteKind.Flow -> "Flow<${kind(kind.elementKind, types)}>" + if (kind.nullable) "?" else ""
        is RemoteKind.Deferred -> "Deferred<${kind(kind.elementKind, types)}>" + if (kind.nullable) "?" else ""
        is RemoteKind.ReceiveChannel -> "ReceiveChannel<${kind(kind.elementKind, types)}>" + if (kind.nullable) "?" else ""
        is RemoteKind.SendChannel -> "SendChannel<${kind(kind.elementKind, types)}>" + if (kind.nullable) "?" else ""
        else -> kind.javaClass.simpleName
    }

    /** [descriptor]'s name, recording the serial shape of every Agent Doc type it reaches. */
    @OptIn(ExperimentalSerializationApi::class)
    private fun reference(descriptor: SerialDescriptor, types: MutableMap<String, String>): String {
        val name = descriptor.serialName.removeSuffix("?")
        val shown = name + if (descriptor.isNullable) "?" else ""
        when (descriptor.kind) {
            StructureKind.LIST, StructureKind.MAP -> {
                val elements = (0 until descriptor.elementsCount).joinToString(", ") {
                    reference(descriptor.getElementDescriptor(it), types)
                }
                return "${descriptor.kind}<$elements>" + if (descriptor.isNullable) "?" else ""
            }
            else -> Unit
        }
        if (!name.startsWith("com.github.btakita.") || name in types) return shown
        types[name] = "" // recursion guard
        val body = mutableListOf("type $name ${descriptor.kind}")
        for (index in 0 until descriptor.elementsCount) {
            val element = descriptor.getElementName(index)
            if (descriptor.kind == SerialKind.ENUM) {
                body += "  $element"
                continue
            }
            // `@EncodeDefault` is not carried by the descriptor; `SurfaceSnapshotContractTest` pins
            // that a never-encoded key stays absent from an older schema's JSON.
            val optional = if (descriptor.isElementOptional(index)) " optional" else ""
            body += "  $element: ${reference(descriptor.getElementDescriptor(index), types)}$optional"
        }
        types[name] = body.joinToString("\n")
        return shown
    }

    private data class Baseline(
        val revision: Int,
        val shellBump: String,
        val compatibilityPlan: String,
        val fingerprint: String,
    ) {
        companion object {
            fun read(): Baseline {
                val text = checkNotNull(SharedRpcContractGuardTest::class.java.getResource("/shared-rpc-contract.baseline")) {
                    "shared-rpc-contract.baseline is missing from shared/src/test/resources"
                }.readText()
                val (header, fingerprint) = text.split("\n---\n", limit = 2).also {
                    check(it.size == 2) { "the baseline needs a `---` line before the fingerprint" }
                }
                val fields = header.lines().filterNot { it.startsWith("#") || it.isBlank() }.associate { line ->
                    line.substringBefore(':').trim() to line.substringAfter(':').trim()
                }
                return Baseline(
                    revision = fields["revision"]?.toIntOrNull() ?: error("baseline revision"),
                    shellBump = fields["shell_bump"] ?: error("baseline shell_bump"),
                    compatibilityPlan = fields["compatibility_plan"] ?: error("baseline compatibility_plan"),
                    fingerprint = fingerprint,
                )
            }
        }
    }

    private fun unifiedDiff(expected: String, actual: String): String {
        val removed = expected.lines() - actual.lines().toSet()
        val added = actual.lines() - expected.lines().toSet()
        return (removed.map { "- $it" } + added.map { "+ $it" }).joinToString("\n")
    }

    private fun moduleRoot(): Path =
        listOf(Paths.get(".."), Paths.get("."), Paths.get("editors/jetbrains-262"))
            .map { it.toAbsolutePath().normalize() }
            .first { Files.exists(it.resolve("shell-sources.txt")) && Files.exists(it.resolve("shared")) }
}
