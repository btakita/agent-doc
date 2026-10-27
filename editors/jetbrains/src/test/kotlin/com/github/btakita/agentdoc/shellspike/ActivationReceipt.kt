package com.github.btakita.agentdoc.shellspike

/**
 * What one JVM's shell reports after an activation. It crosses process boundaries (the backend's
 * controller registration, the frontend/backend RPC) as one version-neutral text line, so a later
 * shell can still read an older receipt.
 *
 * [epoch] is assigned by the backend for each swap and echoed by every JVM that activates it;
 * [generation] is the local, per-JVM counter and is never compared across JVMs.
 */
data class ActivationReceipt(
    val role: String,
    val jvmInstanceId: String,
    val shellDigest: String,
    val spiVersion: Int,
    val implVersion: String?,
    val implSha256: String?,
    val generation: Long,
    val epoch: Long,
) {
    fun encode(): String =
        listOf(
            "role" to role,
            "jvm" to jvmInstanceId,
            "shell" to shellDigest,
            "spi" to spiVersion.toString(),
            "impl_version" to (implVersion ?: NONE),
            "impl_sha256" to (implSha256 ?: NONE),
            "generation" to generation.toString(),
            "epoch" to epoch.toString(),
        ).joinToString(" ", prefix = "$PREFIX ") { (key, value) -> "$key=$value" }

    companion object {
        const val PREFIX = "agent_doc_activation_receipt/v1"
        private const val NONE = "none"

        fun decode(line: String): ActivationReceipt {
            require(line.startsWith("$PREFIX ")) { "not an activation receipt: $line" }
            val fields = line.removePrefix("$PREFIX ").split(' ').associate { field ->
                val (key, value) = field.split('=', limit = 2).also {
                    require(it.size == 2) { "malformed receipt field: $field" }
                }
                key to value
            }
            fun field(key: String) = fields[key] ?: throw IllegalArgumentException("receipt is missing $key")
            fun optional(key: String) = field(key).takeUnless { it == NONE }
            return ActivationReceipt(
                role = field("role"),
                jvmInstanceId = field("jvm"),
                shellDigest = field("shell"),
                spiVersion = field("spi").toInt(),
                implVersion = optional("impl_version"),
                implSha256 = optional("impl_sha256"),
                generation = field("generation").toLong(),
                epoch = field("epoch").toLong(),
            )
        }
    }
}

/** How the frontend half (JetBrains Client) participates in implementation generations. */
enum class FrontendPolicy {
    /** Recommended default: the frontend runs shell code only; its changes stay restart-bound. */
    SHELL_ONLY,

    /** Operator choice: the frontend activates the same implementation jar at the same epoch. */
    SAME_IMPL,
}

/** The generation the backend asked every JVM to run. */
data class ExpectedGeneration(val shellDigest: String, val implSha256: String, val epoch: Long)

sealed class FenceDecision {
    object Accepted : FenceDecision()
    data class Hold(val reason: String) : FenceDecision()
}

/**
 * Admission for generation-dependent work (routes, surface authority) after a swap. Until both
 * receipts agree, the backend keeps the existing `frontend_authority_absent_or_generation_mismatched`
 * readiness instead of acting on a half-upgraded Remote Dev pair.
 */
object ActivationFence {
    fun decide(
        expected: ExpectedGeneration,
        backend: ActivationReceipt?,
        frontend: ActivationReceipt?,
        remoteFrontendConnected: Boolean,
        policy: FrontendPolicy = FrontendPolicy.SHELL_ONLY,
    ): FenceDecision {
        if (backend == null) return FenceDecision.Hold("backend has not reported an activation")
        if (backend.shellDigest != expected.shellDigest) {
            return FenceDecision.Hold("backend shell ${backend.shellDigest} is not ${expected.shellDigest}; restart required")
        }
        if (backend.implSha256 != expected.implSha256 || backend.epoch != expected.epoch) {
            return FenceDecision.Hold(
                "backend runs impl ${backend.implSha256} at epoch ${backend.epoch}, " +
                    "expected ${expected.implSha256} at epoch ${expected.epoch}",
            )
        }
        if (!remoteFrontendConnected) return FenceDecision.Accepted
        if (frontend == null) return FenceDecision.Hold("a remote JetBrains Client is connected but has not reported")
        if (frontend.jvmInstanceId == backend.jvmInstanceId) {
            return FenceDecision.Hold("the frontend receipt came from the backend JVM, not the JetBrains Client")
        }
        if (frontend.shellDigest != expected.shellDigest) {
            return FenceDecision.Hold(
                "JetBrains Client shell ${frontend.shellDigest} is not ${expected.shellDigest}; " +
                    "restart both sides with the same package",
            )
        }
        return when (policy) {
            FrontendPolicy.SHELL_ONLY ->
                if (frontend.implSha256 == null) {
                    FenceDecision.Accepted
                } else {
                    FenceDecision.Hold("JetBrains Client runs impl ${frontend.implSha256} under a shell-only frontend policy")
                }
            FrontendPolicy.SAME_IMPL ->
                if (frontend.implSha256 == expected.implSha256 && frontend.epoch == expected.epoch) {
                    FenceDecision.Accepted
                } else {
                    FenceDecision.Hold(
                        "JetBrains Client runs impl ${frontend.implSha256} at epoch ${frontend.epoch}, " +
                            "expected ${expected.implSha256} at epoch ${expected.epoch}",
                    )
                }
        }
    }
}
