package com.github.btakita.agentdoc

import java.nio.file.Files
import java.nio.file.Paths
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
}
