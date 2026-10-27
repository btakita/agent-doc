package com.github.btakita.agentdoc.shell

import com.intellij.openapi.util.Condition
import kotlinx.coroutines.isActive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * GH #339 R1, EDT and pooled work: the event queue holds shell [ShellGenerationWork.FencedTask]s,
 * whose fence is the generation's retirement policy. A fake event queue stands in for the EDT and
 * keeps every queued task and its `expired` condition, as the platform would.
 */
class ShellGenerationWorkTest {
    private class FakeEdt {
        val queued = mutableListOf<Pair<ShellGenerationWork.FencedTask, Condition<Any?>>>()

        fun dispatch(task: ShellGenerationWork.FencedTask, expired: Condition<Any?>) {
            queued += task to expired
        }

        /** Run every queued task the way the platform does: skip it when it has expired. */
        fun flush() {
            queued.toList().forEach { (task, expired) -> if (!expired.value(null)) task.run() }
            queued.clear()
        }
    }

    private val edt = FakeEdt()
    private val work = ShellGenerationWork(generation = 7, edt = edt::dispatch)

    @Test
    fun `an EDT task runs once and then drops the implementation runnable`() {
        var runs = 0
        work.invokeLater { runs++ }
        assertEquals(1, work.report().pendingEdtTasks)
        val (task, _) = edt.queued.single()
        edt.flush()
        task.run()
        assertEquals(1, runs)
        assertTrue(task.cleared)
        assertEquals(0, work.report().pendingEdtTasks)
    }

    @Test
    fun `drain retirement still runs queued EDT work, as plugin unload always did`() {
        val ran = mutableListOf<String>()
        work.invokeLater { ran += "before" }
        val report = work.retire(ShellGenerationWork.Retirement.DRAIN)
        assertEquals(1, report.pendingEdtTasks)
        // Unload teardown queues EDT work of its own after retirement began.
        work.invokeLater { ran += "teardown" }
        edt.flush()
        assertEquals(listOf("before", "teardown"), ran)
        assertFalse(work.coroutineScope.isActive)
    }

    @Test
    fun `cancel retirement expires queued EDT work and releases its runnable`() {
        var ran = false
        work.invokeLater { ran = true }
        val (task, expired) = edt.queued.single()

        work.retire(ShellGenerationWork.Retirement.CANCEL)

        assertTrue("the platform must see the task as expired", expired.value(null))
        assertTrue("retirement must clear the implementation reference", task.cleared)
        edt.flush()
        task.run()
        assertFalse(ran)
        assertEquals(0, work.report().pendingEdtTasks)
    }

    @Test
    fun `executors and threads made for a generation are reported until they stop`() {
        val executor = work.newSingleThreadScheduledExecutor("agent-doc-test-executor")
        val gate = java.util.concurrent.CountDownLatch(1)
        val thread = work.newThread({ gate.await() }, "agent-doc-test-thread").apply { isDaemon = true }
        thread.start()
        val worker = executor.submit<Thread> { Thread.currentThread() }.get()
        assertEquals("agent-doc-test-executor", worker.name)
        assertTrue("executor threads are daemon threads", worker.isDaemon)
        assertEquals(1, work.report().liveExecutors)
        assertEquals("the executor's worker and the raw thread", 2, work.report().liveThreads)
        executor.shutdownNow()
        gate.countDown()
        worker.join(5_000)
        thread.join(5_000)
        assertEquals(0, work.report().liveExecutors)
        assertEquals(0, work.report().liveThreads)
        val daemon = work.threadFactory { "agent-doc-test-factory" }.newThread {}
        assertTrue(daemon.isDaemon)
        assertEquals("agent-doc-test-factory", daemon.name)
    }

    @Test
    fun `the retirement report is one log-safe line`() {
        assertEquals(
            "pending_edt=0 pooled_in_flight=0 live_executors=0 live_threads=0",
            work.retire(ShellGenerationWork.Retirement.DRAIN).encode(),
        )
    }
}
