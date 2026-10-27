package com.github.btakita.agentdoc.shellspike

import java.lang.ref.WeakReference
import java.nio.file.Files
import java.nio.file.Path
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicReference
import java.util.function.Consumer

/** Result of asking the shell to activate one implementation jar. */
sealed class SwapOutcome {
    data class Activated(
        val generation: Long,
        val implVersion: String,
        val implSha256: String,
        val importedState: Boolean,
    ) : SwapOutcome()

    /** Nothing was loaded or the jar was unusable; the active generation never stopped. */
    data class Refused(val reason: String) : SwapOutcome()

    /** The replacement failed after the active generation quiesced; that generation resumed. */
    data class RolledBack(val reason: String) : SwapOutcome()

    /** Only an IDE restart can apply this change; the restart prompt is the fallback. */
    data class RestartRequired(val reason: String) : SwapOutcome()
}

/** A retired generation whose loader the leak check watches. */
data class RetiredGeneration(
    val generation: Long,
    val implVersion: String,
    val loader: WeakReference<ClassLoader>,
)

/**
 * A shell-owned stand-in for an implementation throwable. The original throwable (an
 * implementation class whose `getClass()` pins its loader) is never stored: only strings and a
 * fresh copy of its stack frames survive, so the platform error pool can keep this forever.
 */
class ImplFailure private constructor(message: String) : RuntimeException(message, null, false, true) {
    companion object {
        fun from(error: Throwable, generation: Long): ImplFailure {
            val chain = generateSequence(error) { it.cause }.take(8).joinToString(" <- ") {
                "${it.javaClass.name}: ${it.message}"
            }
            return ImplFailure("implementation generation $generation failed: $chain").also { failure ->
                failure.stackTrace = error.stackTrace.map { frame ->
                    StackTraceElement(
                        frame.classLoaderName,
                        frame.moduleName,
                        frame.moduleVersion,
                        frame.className,
                        frame.methodName,
                        frame.fileName,
                        frame.lineNumber,
                    )
                }.toTypedArray()
            }
        }
    }
}

internal enum class SlotState { PREPARED, ACTIVE, QUIESCED, RETIRED }

/** Shell-side bookkeeping for one generation; after [retire] it references nothing it loaded. */
internal class GenerationSlot(
    val generation: Long,
    val implVersion: String,
    val implSha256: String,
    impl: ImplGeneration,
    loader: ClassLoader,
) {
    @Volatile var impl: ImplGeneration? = impl
    @Volatile var loader: ClassLoader? = loader
    val loaderRef = WeakReference(loader)
    val loaderName: String = loader.name
    val handlers = ConcurrentHashMap<String, Consumer<String>>()
    val pendingTasks: MutableSet<FencedTask> = ConcurrentHashMap.newKeySet()

    @Volatile var state = SlotState.PREPARED

    fun retire() {
        state = SlotState.RETIRED
        impl = null
        loader = null
        handlers.clear()
        pendingTasks.toList().forEach(FencedTask::cancel)
        pendingTasks.clear()
    }
}

/**
 * What the platform's event queue retains for an implementation `invokeLater`. The queue may hold
 * it long after its generation retires; [cancel] drops both the implementation runnable and the
 * slot, so a retained task pins nothing.
 */
internal class FencedTask(slot: GenerationSlot, task: Runnable, private val host: GenerationHost) : Runnable {
    private val slot = AtomicReference<GenerationSlot?>(slot)
    private val task = AtomicReference<Runnable?>(task)

    fun cancel() {
        task.set(null)
        slot.set(null)
    }

    override fun run() {
        val owner = slot.getAndSet(null) ?: return
        val work = task.getAndSet(null) ?: return
        owner.pendingTasks.remove(this)
        if (owner.state != SlotState.ACTIVE) return
        host.guarded(owner.generation) { work.run() }
    }
}

/** The single subscriber the shell registers with the platform for one topic. */
internal class TopicForwarder(private val host: GenerationHost, private val topic: String) : Consumer<String> {
    override fun accept(payload: String) = host.deliver(topic, payload)
}

private class SlotContext(private val host: GenerationHost, private val slot: GenerationSlot) : GenerationContext {
    override val generation: Long get() = slot.generation
    override val implVersion: String get() = slot.implVersion

    override fun subscribe(topic: String, handler: Consumer<String>) {
        when (slot.state) {
            SlotState.PREPARED -> slot.handlers[topic] = handler
            SlotState.ACTIVE -> {
                slot.handlers[topic] = handler
                host.ensureForwarder(topic)
            }
            // A stale generation that is still running code cannot re-register itself.
            SlotState.QUIESCED, SlotState.RETIRED -> Unit
        }
    }

    override fun invokeLater(task: Runnable) {
        if (slot.state == SlotState.RETIRED) return
        val fenced = FencedTask(slot, task, host)
        slot.pendingTasks += fenced
        host.platform.invokeLater(fenced)
    }
}

/**
 * The stable shell's generation host: one per JVM (a Remote Dev backend and its JetBrains Client
 * each run their own). It owns every platform registration, lends each implementation generation
 * a [GenerationContext], and swaps generations with prepare, quiesce, import, activate, retire.
 */
class GenerationHost(
    internal val platform: FakePlatform,
    val role: String,
    val shellDigest: String,
    private val spiLoader: ClassLoader = ImplGeneration::class.java.classLoader,
) {
    val jvmInstanceId: String = UUID.randomUUID().toString()

    private val lock = Any()
    private var nextGeneration = 1L
    private var epoch = 0L

    @Volatile private var active: GenerationSlot? = null
    private var swapping = false
    private val buffered = ArrayDeque<Pair<String, String>>()
    private val forwardedTopics = LinkedHashSet<String>()
    private val retired = mutableListOf<RetiredGeneration>()

    /** False once a retired generation survived the leak check: further swaps would accumulate. */
    @Volatile var hotSwapAllowed: Boolean = true
        private set

    @Volatile private var leakReason: String? = null

    fun installFromPath(jar: Path, expectedSha256: String, epoch: Long): SwapOutcome =
        install(Files.readAllBytes(jar), expectedSha256, epoch)

    fun install(jarBytes: ByteArray, expectedSha256: String, epoch: Long): SwapOutcome {
        if (!hotSwapAllowed) return SwapOutcome.RestartRequired(leakReason ?: "hot swap disabled")
        val jar = try {
            VerifiedImplJar.verify(jarBytes, expectedSha256)
        } catch (refused: ImplJarRefused) {
            return SwapOutcome.Refused(refused.message ?: "jar refused")
        }
        if (jar.spiVersion != SHELL_SPI_VERSION) {
            return SwapOutcome.RestartRequired(
                "jar ${jar.implVersion} targets shell SPI v${jar.spiVersion}; this shell is " +
                    "v$SHELL_SPI_VERSION, and a shell change needs an IDE restart",
            )
        }
        return synchronized(lock) { swapLocked(jar, epoch) }
    }

    private fun swapLocked(jar: VerifiedImplJar, newEpoch: Long): SwapOutcome {
        val generation = nextGeneration++
        val slot = try {
            val loader = ImplClassLoader(jar, spiLoader, generation)
            val instance = loader.loadClass(jar.implClass).getDeclaredConstructor().newInstance()
            val impl = instance as? ImplGeneration
                ?: return SwapOutcome.Refused("${jar.implClass} does not implement the shell SPI")
            GenerationSlot(generation, jar.implVersion, jar.sha256, impl, loader)
        } catch (error: Throwable) {
            return SwapOutcome.Refused("could not instantiate ${jar.implClass}: ${error.javaClass.name}: ${error.message}")
        }

        swapping = true
        try {
            val previous = active
            val exported = previous?.let { old ->
                try {
                    old.impl?.quiesce().also { old.state = SlotState.QUIESCED }
                } catch (error: Throwable) {
                    retireAndWatch(slot)
                    return SwapOutcome.Refused("active generation ${old.generation} could not quiesce: ${error.message}")
                }
            }
            try {
                slot.impl!!.prepare(SlotContext(this, slot), exported)
            } catch (error: Throwable) {
                retireAndWatch(slot)
                previous?.let { old ->
                    old.impl?.resume()
                    old.state = SlotState.ACTIVE
                }
                return SwapOutcome.RolledBack(
                    "generation $generation (${jar.implVersion}) failed to prepare: " +
                        "${error.javaClass.name}: ${error.message}",
                )
            }
            slot.handlers.keys.forEach(::ensureForwarder)
            slot.state = SlotState.ACTIVE
            active = slot
            epoch = newEpoch
            previous?.let(::retireAndWatch)
            return SwapOutcome.Activated(generation, jar.implVersion, jar.sha256, importedState = exported != null)
        } finally {
            swapping = false
            flushBuffered()
        }
    }

    private fun retireAndWatch(slot: GenerationSlot) {
        slot.retire()
        retired += RetiredGeneration(slot.generation, slot.implVersion, slot.loaderRef)
    }

    /** Register the shell's one forwarder for [topic]; a later generation reuses it. */
    internal fun ensureForwarder(topic: String) {
        val added = synchronized(forwardedTopics) { forwardedTopics.add(topic) }
        if (added) platform.subscribe(topic, TopicForwarder(this, topic))
    }

    internal fun deliver(topic: String, payload: String) {
        synchronized(buffered) {
            if (swapping) {
                buffered.addLast(topic to payload)
                return
            }
        }
        val slot = active ?: return
        if (slot.state != SlotState.ACTIVE) return
        val handler = slot.handlers[topic] ?: return
        guarded(slot.generation) { handler.accept(payload) }
    }

    private fun flushBuffered() {
        val pending = synchronized(buffered) { buffered.toList().also { buffered.clear() } }
        pending.forEach { (topic, payload) -> deliver(topic, payload) }
    }

    /** Run implementation code; a failure reaches the platform only as a shell [ImplFailure]. */
    internal fun guarded(generation: Long, work: () -> Unit) {
        try {
            work()
        } catch (error: Throwable) {
            platform.reportError(ImplFailure.from(error, generation))
        }
    }

    /** A shell-forwarded request (an action, a service call) to the active generation. */
    fun request(kind: String, payload: String = ""): String {
        val slot = active ?: throw IllegalStateException("no active implementation generation")
        val impl = slot.impl ?: throw IllegalStateException("generation ${slot.generation} is retired")
        return try {
            impl.handle(kind, payload)
        } catch (error: Throwable) {
            throw ImplFailure.from(error, slot.generation)
        }
    }

    fun activeLoaderName(): String? = active?.loaderName

    fun receipt(): ActivationReceipt {
        val slot = active
        return ActivationReceipt(
            role = role,
            jvmInstanceId = jvmInstanceId,
            shellDigest = shellDigest,
            spiVersion = SHELL_SPI_VERSION,
            implVersion = slot?.implVersion,
            implSha256 = slot?.implSha256,
            generation = slot?.generation ?: 0,
            epoch = if (slot == null) 0 else epoch,
        )
    }

    fun retiredGenerations(): List<RetiredGeneration> = synchronized(lock) { retired.toList() }

    /**
     * Force collections until every retired loader is gone, or report the survivors. A survivor
     * means something outside the shell still reaches that generation; the host then refuses
     * further hot swaps, so the next upgrade falls back to the restart prompt instead of piling
     * up retained generations.
     */
    fun checkRetiredCollected(maxCollections: Int = 40): List<RetiredGeneration> {
        val watched = retiredGenerations()
        repeat(maxCollections) {
            if (watched.all { it.loader.get() == null }) return emptyList()
            System.gc()
            @Suppress("UNUSED_VARIABLE")
            val pressure = ByteArray(1 shl 20)
        }
        val survivors = watched.filter { it.loader.get() != null }
        if (survivors.isNotEmpty()) {
            leakReason = "retired implementation generation(s) " +
                survivors.joinToString { "${it.generation} (${it.implVersion})" } +
                " are still reachable after retirement; restart the IDE to release them"
            hotSwapAllowed = false
        }
        return survivors
    }
}
