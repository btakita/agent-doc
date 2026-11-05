package com.github.btakita.agentdoc

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/** `#starttimeoutload`: progress-aware bootstrap wait. No assertion depends on elapsed time. */
class ProgressAwareCommandTest {
    private val budget = ProgressWaitBudget(idleTimeoutMs = 10_000L, overallCapMs = 60_000L)

    @Test
    fun `parses only well-formed progress lines`() {
        assertEquals("registry_lookup", ProgressAwareCommand.parseProgressPhase("[progress] phase=registry_lookup"))
        assertEquals("reused", ProgressAwareCommand.parseProgressPhase("  [progress] phase=reused \r"))
        assertNull(ProgressAwareCommand.parseProgressPhase("[progress] phase="))
        assertNull(ProgressAwareCommand.parseProgressPhase("[progress] phase=two words"))
        assertNull(ProgressAwareCommand.parseProgressPhase("[start] doc.md already has a live supervisor"))
        assertEquals(
            "b",
            ProgressAwareCommand.lastProgressPhase(
                listOf("[progress] phase=a", "noise", "[progress] phase=b", "[start] done"),
            ),
        )
        assertNull(ProgressAwareCommand.lastProgressPhase(listOf("Error: boom")))
        assertEquals(
            "[start] done\nError: boom",
            ProgressAwareCommand.diagnosticText(listOf("[progress] phase=a", "[start] done", "Error: boom")),
        )
    }

    @Test
    fun `progress extends the wait until the idle bound or the cap`() {
        // No phase yet: the idle bound runs from launch, which is the old fixed kill.
        assertEquals(ProgressWaitDecision.Wait(10_000L), ProgressAwareCommand.decide(0L, 0L, 0L, budget))
        assertEquals(
            ProgressWaitDecision.Expire(ProgressExpiry.NO_PROGRESS),
            ProgressAwareCommand.decide(10_000L, 0L, 0L, budget),
        )
        // A phase at 9 s moves the idle deadline to 19 s: 12.3 s no longer expires.
        assertEquals(ProgressWaitDecision.Wait(6_700L), ProgressAwareCommand.decide(12_300L, 0L, 9_000L, budget))
        assertEquals(
            ProgressWaitDecision.Expire(ProgressExpiry.NO_PROGRESS),
            ProgressAwareCommand.decide(19_000L, 0L, 9_000L, budget),
        )
        // Steady progress never outlives the cap, and the wait never sleeps past it.
        assertEquals(ProgressWaitDecision.Wait(1_000L), ProgressAwareCommand.decide(59_000L, 0L, 58_500L, budget))
        assertEquals(
            ProgressWaitDecision.Expire(ProgressExpiry.OVERALL_CAP),
            ProgressAwareCommand.decide(60_000L, 0L, 59_999L, budget),
        )
    }

    @Test
    fun `timeout message names the last phase instead of a bare exit 124`() {
        assertEquals(
            "agent-doc tmux start timed out in phase 'registry_lookup' (no progress for 10s; 12.3s elapsed)",
            ProgressAwareCommand.timeoutMessage(
                "agent-doc tmux start",
                "registry_lookup",
                ProgressExpiry.NO_PROGRESS,
                12_300L,
                budget,
            ),
        )
        assertEquals(
            "agent-doc tmux start timed out before it reported any phase (still running at the 60s cap; 60s elapsed)",
            ProgressAwareCommand.timeoutMessage("agent-doc tmux start", null, ProgressExpiry.OVERALL_CAP, 60_000L, budget),
        )
    }

    @Test
    fun `failure detail covers success, failure, and timeout`() {
        fun result(exit: Int, out: String = "", err: String = "", phase: String? = null, expiry: ProgressExpiry? = null) =
            ProgressCommandResult(exit, out, err, phase, expiry, 12_300L)

        assertNull(ProgressAwareCommand.failureDetail("start", result(0), budget))
        assertEquals("Error: boom", ProgressAwareCommand.failureDetail("start", result(1, err = "Error: boom"), budget))
        assertEquals("start failed (exit 2)", ProgressAwareCommand.failureDetail("start", result(2), budget))
        assertEquals(
            "start timed out in phase 'terminal_policy' (no progress for 10s; 12.3s elapsed): [registry] waiting",
            ProgressAwareCommand.failureDetail(
                "start",
                result(124, err = "[registry] waiting", phase = "terminal_policy", expiry = ProgressExpiry.NO_PROGRESS),
                budget,
            ),
        )
    }

    @Test
    fun `run asks for progress and separates it from diagnostics`() {
        val result = ProgressAwareCommand.run(
            listOf(
                "sh",
                "-c",
                "printf '[progress] phase=terminal_host\\n' >&2; " +
                    "printf '[start] reused\\n' >&2; printf '%s' \"\$AGENT_DOC_PROGRESS\"",
            ),
            File(".").absolutePath,
            budget,
        )

        assertEquals(0, result.exitCode)
        assertEquals("1", result.output)
        assertEquals("[start] reused", result.errorOutput)
        assertEquals("terminal_host", result.lastPhase)
        assertNull(result.expiry)
    }

    @Test
    fun `a stalled command is killed and reports the phase it stalled in`() {
        val dir = kotlin.io.path.createTempDirectory("progress-stall").toFile()
        val marker = File(dir, "phases-written")
        // Time stands still until the script has written its phases (they are then in
        // the pipe, drained after the kill); after that every reading is 1 s later than
        // the last. So the expiry is decided by the phases, never by scheduling.
        var calls = 0L
        val clock = {
            synchronized(this) {
                if (marker.exists()) {
                    calls += 1
                    calls * 1_000L
                } else {
                    0L
                }
            }
        }
        try {
            val result = ProgressAwareCommand.run(
                listOf(
                    "sh",
                    "-c",
                    "printf '[progress] phase=config\\n' >&2; printf '[progress] phase=registry_lookup\\n' >&2; " +
                        "printf 'waiting on the registry lock\\n' >&2; : > '${marker.absolutePath}'; exec sleep 30",
                ),
                File(".").absolutePath,
                // The cap is far above the idle bound in clock readings: reaching it would
                // mean progress was misread as continuing.
                ProgressWaitBudget(idleTimeoutMs = 200L, overallCapMs = 25_000L),
                clock,
            )

            assertEquals(ProgressAwareCommand.TIMEOUT_EXIT_CODE, result.exitCode)
            assertEquals(ProgressExpiry.NO_PROGRESS, result.expiry)
            assertEquals("registry_lookup", result.lastPhase)
            assertEquals("waiting on the registry lock", result.errorOutput)
            assertTrue(
                ProgressAwareCommand.failureDetail("agent-doc tmux start", result, budget)!!
                    .startsWith("agent-doc tmux start timed out in phase 'registry_lookup'"),
            )
        } finally {
            dir.deleteRecursively()
        }
    }
}
