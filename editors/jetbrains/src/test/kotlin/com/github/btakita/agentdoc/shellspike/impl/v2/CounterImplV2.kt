package com.github.btakita.agentdoc.shellspike.impl.v2

import com.github.btakita.agentdoc.shellspike.GenerationContext
import com.github.btakita.agentdoc.shellspike.ImplGeneration
import java.util.function.Consumer

/** GH #339 spike generation 2: same state schema, new behavior. */
class CounterImplV2 : ImplGeneration {
    private var events = 0
    private var imported = false

    override fun prepare(context: GenerationContext, importedState: String?) {
        val prior = importedState?.removePrefix("events=")?.toIntOrNull()
        imported = prior != null
        events = prior ?: 0
        context.subscribe("document-changed", Consumer { events++ })
    }

    override fun handle(kind: String, payload: String): String = when (kind) {
        "count" -> "v2:$events"
        "imported" -> imported.toString()
        else -> "v2:unknown:$kind"
    }

    override fun quiesce(): String = "events=$events"

    override fun resume() = Unit
}
