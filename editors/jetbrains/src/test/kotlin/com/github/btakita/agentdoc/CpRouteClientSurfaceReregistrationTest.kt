package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import java.net.StandardProtocolFamily
import java.net.UnixDomainSocketAddress
import java.nio.channels.Channels
import java.nio.channels.ServerSocketChannel
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths
import java.util.Collections
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * `#surfaceretireformal`: a live editor frame whose endpoint the controller retired re-registers
 * above the retired generation instead of staying muted until a plugin reload (model:
 * `formal/tla/PassiveTmuxSyncRetireNet.tla`, action `EditorReregister`).
 */
class CpRouteClientSurfaceReregistrationTest {
    private val reregistration = CpRouteClient.SurfaceEndpointReregistration

    @Test
    fun `a typed retired-endpoint receipt names the generation to register above`() {
        assertEquals(
            1393798891942967L,
            reregistration.retiredGeneration(
                """{"intent":"Idle","idle":true,"outcome":null,"error":null,""" +
                    """"terminal_decision":"stale","rejected":"retired_endpoint",""" +
                    """"reregister_above_generation":1393798891942967}""",
            ),
        )
    }

    @Test
    fun `no other receipt re-registers`() {
        assertNull(
            "an accepted observation",
            reregistration.retiredGeneration("""{"intent":"Idle","idle":true}"""),
        )
        assertNull(
            "a duplicate is a benign fence, not a muted endpoint",
            reregistration.retiredGeneration("""{"idle":true,"rejected":"stale_sequence"}"""),
        )
        assertNull(
            "a pre-directive controller names no generation to register above",
            reregistration.retiredGeneration("""{"idle":true,"rejected":"retired_endpoint"}"""),
        )
        assertNull("an unparseable receipt", reregistration.retiredGeneration("controller unavailable"))
    }

    @Test
    fun `re-registration lands strictly above the retired generation and never moves backwards`() {
        assertEquals(42L, reregistration.nextGeneration(current = 41L, retiredGeneration = 41L))
        assertEquals(
            "a racing receipt already moved this client past it",
            42L,
            reregistration.nextGeneration(current = 42L, retiredGeneration = 41L),
        )
        assertEquals(90L, reregistration.nextGeneration(current = 90L, retiredGeneration = 41L))
    }

    @Test
    fun `the transport re-sends once and only while the listener owns the frame`() {
        val source =
            Files.readString(
                Paths.get("src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt")
                    .takeIf { Files.exists(it) }
                    ?: Paths.get("editors/jetbrains/src/main/kotlin/com/github/btakita/agentdoc/CpRouteClient.kt"),
            )
        val transport =
            source
                .substringAfter("private fun observeEditorSurfaceWithClient(")
                .substringBefore("fun forgetEditorSurface(")
        assertTrue(transport.contains("SurfaceEndpointReregistration.retiredGeneration(receipt)"))
        assertTrue("liveness gates the re-registration", transport.contains("if (mayReregister())"))
        assertEquals(
            "exactly one re-send per observation: muted for at most one round trip",
            1,
            Regex("""receipt = publish\(reregistered\)""").findAll(transport).count(),
        )
        assertTrue(
            "a handoff overtaking the re-registration retires it at the new generation",
            transport.contains("generation = reregistered,"),
        )
        for (lane in listOf("fun observeEditorSurface(", "fun observeEditorFocus(")) {
            val publisher = source.substringAfter(lane).substringBefore("): CpEditorRouteResult")
            assertTrue(
                "$lane must not re-register unless its caller proves the frame is live",
                publisher.contains("mayReregister: () -> Boolean = { false }"),
            )
        }
    }

    @Test
    fun `a self-retirement fence is per client and root and monotone`() {
        val fences = CpRouteClient.SelfRetiredEndpoints()
        assertNull("nothing retired yet", fences.fenceFor("focus", "/p", 5L))
        fences.record("focus", "/p/sub/../", 7L)
        fences.record("focus", "/p", 4L)
        assertEquals("the same root by another spelling, never lowered", 7L, fences.fenceFor("focus", "/p", 7L))
        assertNull("a lane already above the fence is not fenced", fences.fenceFor("focus", "/p", 8L))
        assertNull("another root is not fenced", fences.fenceFor("focus", "/q", 1L))
        assertNull("another client is not fenced", fences.fenceFor("layout", "/p", 1L))
    }

    /**
     * `#tmuxsyncselfretire`, observed 2026-11-22: the spanning surface published to the superproject
     * root retired the submodule root's focus lane; the next tab switch to the submodule document
     * was rejected `retired_endpoint`, and when its receipt was late (the controller answered after
     * the 1 s focus budget) tmux never followed. The lane must re-register before that first send.
     */
    @Test
    fun `a focus lane publishing into a root it retired itself is accepted on the first send`() {
        FakeSurfaceController().use { controller ->
            CpRouteClient.forgetEditorFocus(controller.root)
            val receipt =
                CpRouteClient.observeEditorFocus(
                    projectRoot = controller.root,
                    surfaceJson = """{"focused":"${controller.root}/tasks/a.md"}""",
                    surfaceId = "root",
                    mayReregister = { true },
                )
            assertEquals(0, receipt.exitCode)
            assertEquals(
                "one send, accepted: no rejected round trip for a fence the editor set itself",
                listOf("accepted"),
                controller.observations,
            )
        }
    }

    @Test
    fun `a frame the listener does not own keeps its own retirement fence`() {
        FakeSurfaceController().use { controller ->
            CpRouteClient.forgetEditorFocus(controller.root)
            CpRouteClient.observeEditorFocus(
                projectRoot = controller.root,
                surfaceJson = """{"focused":"${controller.root}/tasks/a.md"}""",
                surfaceId = "root",
                mayReregister = { false },
            )
            assertEquals(listOf("retired"), controller.observations)
        }
    }

    /**
     * Minimal `editor_surface_forget` / `editor_surface_observe` peer with the controller's fence:
     * a forget retires the caller at its generation, and an observation at a generation `<=` that
     * fence is the typed `retired_endpoint` receipt.
     */
    private class FakeSurfaceController : AutoCloseable {
        private val dir: Path = Files.createTempDirectory("adsr")
        val root: String = dir.toString()
        val observations: MutableList<String> = Collections.synchronizedList(mutableListOf())
        private val fences = mutableMapOf<String, Long>()
        private val server =
            ServerSocketChannel.open(StandardProtocolFamily.UNIX).also {
                Files.createDirectories(dir.resolve(".agent-doc"))
                it.bind(UnixDomainSocketAddress.of(dir.resolve(".agent-doc/controller.sock")))
            }
        private val thread =
            Thread {
                while (true) {
                    val channel = runCatching { server.accept() }.getOrNull() ?: break
                    channel.use {
                        val line = Channels.newReader(it, Charsets.UTF_8).buffered().readLine() ?: return@use
                        val reply = respond(JsonParser.parseString(line).asJsonObject)
                        val writer = Channels.newWriter(it, Charsets.UTF_8)
                        writer.write(reply.toString() + "\n")
                        writer.flush()
                    }
                }
            }.apply {
                isDaemon = true
                start()
            }

        private fun respond(request: JsonObject): JsonObject {
            val caller = request.get("caller").asString
            val generation = request.get("generation").asLong
            val data = JsonObject()
            when (request.get("command").asString) {
                "editor_surface_forget" -> {
                    fences[caller] = maxOf(fences[caller] ?: Long.MIN_VALUE, generation)
                    data.addProperty("forgotten", true)
                }
                "editor_surface_observe" -> {
                    val fence = fences[caller]
                    if (fence != null && generation <= fence) {
                        observations.add("retired")
                        data.addProperty("idle", true)
                        data.addProperty("rejected", "retired_endpoint")
                        data.addProperty("reregister_above_generation", fence)
                    } else {
                        observations.add("accepted")
                        data.addProperty("idle", false)
                    }
                }
            }
            return JsonObject().also {
                it.addProperty("ok", true)
                it.add("data", data)
            }
        }

        override fun close() {
            server.close()
            thread.join(1_000)
            dir.toFile().deleteRecursively()
        }
    }
}
