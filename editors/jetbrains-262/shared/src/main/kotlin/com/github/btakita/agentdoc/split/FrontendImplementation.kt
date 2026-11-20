package com.github.btakita.agentdoc.split

/** Stable shell-owned SPI for a restart-free frontend implementation generation. */
interface FrontendImplementation {
    fun prepare(epoch: Long, previousState: String?)
    fun quiesce(): String
    fun resume()
    fun retire()
}
