package com.github.btakita.agentdoc.shellspike.impl.broken

import com.github.btakita.agentdoc.shellspike.GenerationContext
import com.github.btakita.agentdoc.shellspike.ImplGeneration

/** A replacement generation that fails after the active generation has already quiesced. */
class BrokenPrepareImpl : ImplGeneration {
    override fun prepare(context: GenerationContext, importedState: String?) {
        throw IllegalStateException("cannot import $importedState")
    }

    override fun handle(kind: String, payload: String): String = "broken"

    override fun quiesce(): String = ""

    override fun resume() = Unit
}
