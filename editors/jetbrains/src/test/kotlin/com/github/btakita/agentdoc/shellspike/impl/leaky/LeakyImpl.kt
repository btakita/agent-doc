package com.github.btakita.agentdoc.shellspike.impl.leaky

import com.github.btakita.agentdoc.shellspike.FakePlatform
import com.github.btakita.agentdoc.shellspike.GenerationContext
import com.github.btakita.agentdoc.shellspike.ImplGeneration
import java.util.function.Consumer

/**
 * Negative control: subscribes to the platform directly instead of through its context, the way
 * an implementation calling `project.messageBus.connect().subscribe(...)` would. The platform then
 * retains this generation's lambda, and so its loader, forever.
 */
class LeakyImpl : ImplGeneration {
    private var events = 0

    override fun prepare(context: GenerationContext, importedState: String?) {
        FakePlatform.current!!.subscribe("document-changed", Consumer { events++ })
    }

    override fun handle(kind: String, payload: String): String = "leaky:$events"

    override fun quiesce(): String = "events=$events"

    override fun resume() = Unit
}
