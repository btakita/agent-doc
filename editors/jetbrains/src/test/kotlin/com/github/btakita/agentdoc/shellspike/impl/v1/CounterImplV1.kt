package com.github.btakita.agentdoc.shellspike.impl.v1

import com.github.btakita.agentdoc.shellspike.GenerationContext
import com.github.btakita.agentdoc.shellspike.ImplGeneration
import java.util.function.Consumer

/** Implementation-defined failure type: its `Class` lives in this generation's loader. */
class V1Failure(message: String) : RuntimeException(message)

/**
 * GH #339 spike generation 1. Loaded only by an `ImplClassLoader` from a packed jar; the shell
 * never names this class.
 */
class CounterImplV1 : ImplGeneration {
    private var context: GenerationContext? = null
    private var events = 0
    private var running = false

    override fun prepare(context: GenerationContext, importedState: String?) {
        this.context = context
        events = importedState?.removePrefix("events=")?.toIntOrNull() ?: 0
        context.subscribe("document-changed", Consumer { events++ })
        running = true
    }

    override fun handle(kind: String, payload: String): String = when (kind) {
        "count" -> "v1:$events"
        "schedule" -> {
            // Queued on the platform's event queue through the shell's fenced forwarder; the
            // runnable captures this generation.
            context!!.invokeLater { events += 100 }
            "scheduled"
        }
        "boom" -> throw V1Failure("v1 refused $payload")
        else -> "v1:unknown:$kind"
    }

    override fun quiesce(): String {
        running = false
        return "events=$events"
    }

    override fun resume() {
        running = true
    }
}
