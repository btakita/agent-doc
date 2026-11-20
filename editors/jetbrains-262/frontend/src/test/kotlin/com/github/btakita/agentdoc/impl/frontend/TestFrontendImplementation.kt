package com.github.btakita.agentdoc.impl.frontend

import com.github.btakita.agentdoc.split.FrontendImplementation

class TestFrontendImplementation : FrontendImplementation {
    override fun prepare(epoch: Long, previousState: String?) {
        require(epoch > 0) { "epoch must be positive" }
    }

    override fun quiesce(): String = ""

    override fun resume() = Unit

    override fun retire() = Unit
}
