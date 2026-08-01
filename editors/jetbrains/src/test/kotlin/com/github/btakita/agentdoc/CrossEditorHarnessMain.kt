package com.github.btakita.agentdoc

import com.google.gson.JsonObject
import com.google.gson.JsonParser

private const val CROSS_EDITOR_NATIVE_HARNESS_CAPABILITY = "cross_editor_native_harness_v1"

/**
 * Headless executable endpoint for the cross-editor SimWorld suite.
 *
 * This is deliberately a composition root only: every operation runs through
 * the shipped [CrdtReplicaForwarder], [CpSocketReplicaTransport], and
 * [NativeReplicaNode]. The harness owns no CRDT or controller protocol logic.
 */
fun main() {
    val projectRoot =
        System.getenv("AGENT_DOC_HARNESS_PROJECT_ROOT")
            ?: error("AGENT_DOC_HARNESS_PROJECT_ROOT is required")
    val filePath =
        System.getenv("AGENT_DOC_HARNESS_FILE")
            ?: error("AGENT_DOC_HARNESS_FILE is required")
    val identity =
        System.getenv("AGENT_DOC_HARNESS_IDENTITY") ?: "intellij:native-harness"

    var forwarder: CrdtReplicaForwarder? = null
    var retained: ReplicaResumeState? = null

    fun newForwarder(resumeState: ReplicaResumeState?): CrdtReplicaForwarder =
        CrdtReplicaForwarder(
            filePath = filePath,
            identity = identity,
            node = NativeReplicaNode(),
            transport = CpSocketReplicaTransport(projectRoot),
            resumeState = resumeState,
        )

    fun reply(block: JsonObject.() -> Unit) {
        val response =
            JsonObject().apply {
                addProperty("harness", "jetbrains")
                block()
            }
        println(response.toString())
        System.out.flush()
    }

    generateSequence(::readLine).forEach { line ->
        if (line.isBlank()) return@forEach
        try {
            val command = JsonParser.parseString(line).asJsonObject
            when (command.get("command")?.asString) {
                "attach" -> {
                    forwarder = newForwarder(null)
                    val registered = forwarder!!.register()
                    reply {
                        addProperty("ok", registered)
                        addProperty("text", forwarder!!.replicaText())
                    }
                }
                "edit" -> {
                    val active = forwarder ?: error("harness is not attached")
                    active.forwardLocalDelta(
                        command.get("offset")?.asInt ?: 0,
                        command.get("deleteLen")?.asInt ?: 0,
                        command.get("insert")?.asString ?: "",
                    )
                    reply {
                        addProperty("ok", true)
                        addProperty("text", active.replicaText())
                    }
                }
                "pull" -> {
                    val active = forwarder ?: error("harness is not attached")
                    val updates = active.pullRemoteUpdates()
                    var allAcked = true
                    updates.forEach { update ->
                        val text = active.applyRemoteUpdate(update.update)
                        allAcked =
                            (text?.let { active.projectVisibleState(it) } == true) && allAcked
                    }
                    reply {
                        addProperty("ok", allAcked)
                        addProperty("applied", updates.size)
                        addProperty("text", active.replicaText())
                    }
                }
                "disconnect" -> {
                    val active = forwarder ?: error("harness is not attached")
                    retained = active.captureResumeState()
                    active.deregister()
                    forwarder = null
                    reply { addProperty("ok", retained != null) }
                }
                "reconnect" -> {
                    val resume = retained ?: error("harness has no retained replica state")
                    forwarder = newForwarder(resume)
                    val registered = forwarder!!.register()
                    reply {
                        addProperty("ok", registered)
                        addProperty("text", forwarder!!.replicaText())
                    }
                }
                "text" -> {
                    reply {
                        addProperty("ok", forwarder != null)
                        addProperty("text", forwarder?.replicaText())
                    }
                }
                "shutdown" -> {
                    forwarder?.deregister()
                    forwarder = null
                    reply { addProperty("ok", true) }
                    return
                }
                else -> error("unsupported harness command: $line")
            }
        } catch (error: Throwable) {
            error.printStackTrace(System.err)
            reply {
                addProperty("ok", false)
                addProperty("error", error.stackTraceToString())
            }
        }
    }
}
