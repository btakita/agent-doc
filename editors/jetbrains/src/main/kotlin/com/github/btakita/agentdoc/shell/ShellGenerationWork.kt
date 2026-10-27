package com.github.btakita.agentdoc.shell

import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ModalityState
import com.intellij.openapi.util.Condition
import com.intellij.util.Alarm
import com.intellij.util.concurrency.AppExecutorUtil
import java.util.Collections
import java.util.WeakHashMap
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.ScheduledExecutorService
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.ThreadFactory
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineName
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

/**
 * GH #339 R1, EDT and pooled work (editors/jetbrains/SPEC.md "Who owns IntelliJ registrations
 * and callbacks"): every task, executor and thread an implementation generation starts goes
 * through the work owner the shell made for that generation.
 *
 * - EDT work is a [FencedTask]: the platform's event queue holds the shell task, never the
 *   implementation runnable, and the task's `expired` condition is the fence.
 * - Pooled and scheduled work runs on the application pools, counted while in flight.
 * - Named executors and threads are created here and tracked weakly, so retirement can report
 *   what the generation left running.
 * - One [CoroutineScope] per generation, cancelled at retirement.
 *
 * Retirement takes a [Retirement] policy. A swap (R3) retires with [Retirement.CANCEL]: queued
 * EDT tasks expire and drop their implementation reference before they run. R1's only retirement
 * is plugin unload, which retires with [Retirement.DRAIN]: queued EDT tasks still run, exactly as
 * they did before R1, because unload teardown itself queues EDT work (an AWT listener detached
 * from a pooled dispose path). Either way a task drops its implementation reference once it ran.
 */
class ShellGenerationWork internal constructor(
    private val generation: Long,
    private val edt: (FencedTask, Condition<Any?>) -> Unit = { task, expired ->
        ApplicationManager.getApplication().invokeLater(task, ModalityState.defaultModalityState(), expired)
    },
) {
    /** How retirement treats EDT tasks that are still queued. */
    enum class Retirement { DRAIN, CANCEL }

    /** What retirement found, for the `[shell]` retirement log line. */
    data class Report(
        val pendingEdtTasks: Int,
        val pooledInFlight: Int,
        val liveExecutors: Int,
        val liveThreads: Int,
    ) {
        fun encode(): String =
            "pending_edt=$pendingEdtTasks pooled_in_flight=$pooledInFlight " +
                "live_executors=$liveExecutors live_threads=$liveThreads"
    }

    /** A queued EDT task: the platform holds this, never the implementation runnable. */
    class FencedTask internal constructor(task: Runnable, private val owner: ShellGenerationWork) : Runnable {
        @Volatile private var task: Runnable? = task

        /** The `expired` condition the platform evaluates before it runs this task. */
        internal val expired = Condition<Any?> { owner.cancelled }

        override fun run() {
            val runnable = synchronized(this) { task.also { task = null } } ?: return
            owner.pending.remove(this)
            if (owner.cancelled) return
            runnable.run()
        }

        /** Drop the implementation reference; a later [run] is inert. */
        internal fun clear() {
            synchronized(this) { task = null }
        }

        internal val cleared: Boolean get() = task == null
    }

    @Volatile private var cancelled = false

    internal val pending: MutableSet<FencedTask> = Collections.newSetFromMap(java.util.concurrent.ConcurrentHashMap())
    private val pooledInFlight = AtomicInteger()
    private val executors: MutableSet<ExecutorService> = Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))
    private val threads: MutableSet<Thread> = Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))

    /** This generation's coroutine scope: pooled dispatch by default, `Dispatchers.EDT` on request. */
    val coroutineScope: CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.Default + CoroutineName("agent-doc generation $generation"))

    /** Queue [task] on the EDT with the default modality state, as `Application.invokeLater` did. */
    fun invokeLater(task: Runnable) {
        val fenced = FencedTask(task, this)
        pending.add(fenced)
        edt(fenced, fenced.expired)
    }

    fun executeOnPooledThread(task: Runnable): Future<*> =
        ApplicationManager.getApplication().executeOnPooledThread(counted(task))

    fun schedule(task: Runnable, delay: Long, unit: TimeUnit): ScheduledFuture<*> =
        AppExecutorUtil.getAppScheduledExecutorService().schedule(counted(task), delay, unit)

    fun scheduleWithFixedDelay(task: Runnable, initialDelay: Long, delay: Long, unit: TimeUnit): ScheduledFuture<*> =
        AppExecutorUtil.getAppScheduledExecutorService().scheduleWithFixedDelay(counted(task), initialDelay, delay, unit)

    fun submit(task: Runnable): Future<*> = AppExecutorUtil.getAppExecutorService().submit(counted(task))

    /** A single daemon thread named [threadName], as `Executors.newSingleThreadScheduledExecutor`. */
    fun newSingleThreadScheduledExecutor(threadName: String): ScheduledExecutorService =
        Executors.newSingleThreadScheduledExecutor(threadFactory { threadName }).also(executors::add)

    /** A single daemon thread named [threadName], as `Executors.newSingleThreadExecutor`. */
    fun newSingleThreadExecutor(threadName: String): ExecutorService =
        Executors.newSingleThreadExecutor(threadFactory { threadName }).also(executors::add)

    /** A daemon thread factory whose threads take [threadName]'s name at creation. */
    fun threadFactory(threadName: () -> String): ThreadFactory = ThreadFactory { runnable ->
        newThread(runnable, threadName()).apply { isDaemon = true }
    }

    /** An unstarted thread, configured exactly as `Thread(task)` (the caller starts it). */
    fun newThread(task: Runnable): Thread = Thread(task).also(threads::add)

    /** An unstarted thread, configured exactly as `Thread(task, threadName)` (the caller starts it). */
    fun newThread(task: Runnable, threadName: String): Thread = Thread(task, threadName).also(threads::add)

    /** A pooled-thread `Alarm` released with [parent], as `Alarm(POOLED_THREAD, parent)`. */
    fun newPooledAlarm(parent: Disposable): Alarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, parent)

    /** Retire this generation's work; idempotent. */
    fun retire(policy: Retirement): Report {
        if (policy == Retirement.CANCEL) {
            cancelled = true
            pending.toList().forEach { it.clear() }
            pending.clear()
        }
        coroutineScope.cancel()
        return report()
    }

    fun report(): Report = Report(
        pendingEdtTasks = pending.size,
        pooledInFlight = pooledInFlight.get(),
        liveExecutors = synchronized(executors) { executors.count { !it.isShutdown } },
        liveThreads = synchronized(threads) { threads.count { it.isAlive } },
    )

    /** Counts [task] as in flight while it runs (a periodic task counts once per run). */
    private fun counted(task: Runnable): Runnable = Runnable {
        pooledInFlight.incrementAndGet()
        try {
            task.run()
        } finally {
            pooledInFlight.decrementAndGet()
        }
    }
}
