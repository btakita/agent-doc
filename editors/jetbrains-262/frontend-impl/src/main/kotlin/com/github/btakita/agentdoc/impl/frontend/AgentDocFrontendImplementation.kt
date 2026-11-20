package com.github.btakita.agentdoc.impl.frontend

import com.github.btakita.agentdoc.split.FrontendImplementation

/** Initial implementation generation; behavior migrates here without changing the shell. */
class AgentDocFrontendImplementation : FrontendImplementation {
    override fun prepare(epoch: Long, previousState: String?) {
        require(epoch > 0) { "epoch must be positive" }
    }

    override fun quiesce(): String = "{\"schema\":1}"

    override fun resume() = Unit

    override fun retire() = Unit
}
