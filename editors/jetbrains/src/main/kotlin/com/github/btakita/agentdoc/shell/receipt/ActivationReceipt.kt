package com.github.btakita.agentdoc.shell.receipt

import java.util.Properties
import java.util.UUID

/*
 * GH #339 R1: the activation receipt. This file is JDK-only on purpose: the classic plugin compiles
 * it into its jar, and the modular (exact-262/263) build compiles it into `agent.doc.shared`, so
 * the backend and frontend content modules read one class and one shell identity. It has its own
 * package so no package is split between the shared and backend content modules.
 */

/**
 * What one JVM's shell reports after an activation, as one version-neutral text line:
 *
 * ```text
 * agent_doc_activation_receipt/v1 role=<backend|frontend> jvm=<instance-uuid> shell=<pluginShellDigest> spi=<n> impl_version=<v|none> impl_sha256=<hex|none> generation=<local-counter> epoch=<backend-assigned>
 * ```
 *
 * [epoch] is assigned by the backend for each swap and echoed by every JVM that activates it;
 * [generation] is local to one JVM and is never compared across JVMs. In R1 no separate
 * implementation jar exists, so a backend reports `impl_sha256=none` and the frontend, which is
 * shell-only, reports `impl_version=none` as well.
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
        ).joinToString(" ", prefix = "$PREFIX ") { (key, value) -> "$key=${token(value)}" }

    companion object {
        const val PREFIX = "agent_doc_activation_receipt/v1"
        const val ROLE_BACKEND = "backend"
        const val ROLE_FRONTEND = "frontend"
        private const val NONE = "none"

        /** A value never carries a separator, so one receipt always stays one parseable line. */
        private fun token(value: String): String =
            value.ifEmpty { NONE }.map { if (it.isWhitespace() || it == '=') '_' else it }.joinToString("")

        /** The receipt a shell-only JVM (the JetBrains Client frontend) reports: no generation. */
        fun shellOnly(role: String): ActivationReceipt =
            ActivationReceipt(
                role = role,
                jvmInstanceId = ShellIdentity.jvmInstanceId,
                shellDigest = ShellIdentity.shellDigest,
                spiVersion = SHELL_SPI_VERSION,
                implVersion = null,
                implSha256 = null,
                generation = 0,
                epoch = 0,
            )

        fun decode(line: String): ActivationReceipt {
            require(line.startsWith("$PREFIX ")) { "not an activation receipt: $line" }
            val fields = line.removePrefix("$PREFIX ").split(' ').associate { field ->
                val parts = field.split('=', limit = 2)
                require(parts.size == 2) { "malformed receipt field: $field" }
                parts[0] to parts[1]
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

        /** Decode, or null for anything that is not a v1 receipt (an older or newer peer). */
        fun decodeOrNull(line: String?): ActivationReceipt? =
            line?.let { runCatching { decode(it) }.getOrNull() }
    }
}

/**
 * Bumped only by a shell release. An implementation built against another SPI version is refused.
 * It lives in this JDK-only file so the modular `agent.doc.shared` module carries the one copy.
 */
const val SHELL_SPI_VERSION: Int = 1

/**
 * This package's shell identity, generated at build time from `editors/jetbrains/shell-sources.txt`
 * (`generateShellIdentity`): `shell_digest` covers the shell file set, `impl_digest` the rest of
 * the plugin sources. A build without the resource (an IDE run configuration that skipped the
 * task) reports `unknown`, which no expected digest ever matches.
 */
object ShellIdentity {
    /** Beside this class, in this package: `com/github/btakita/agentdoc/shell/receipt/`. */
    const val RESOURCE = "shell-identity.properties"
    const val UNKNOWN = "unknown"

    /** Shared by every classloader in this JVM, so a dynamic plugin reload keeps it. */
    private const val JVM_INSTANCE_PROPERTY = "agent.doc.shell.jvm.instance"

    private val properties: Properties by lazy {
        Properties().also { loaded ->
            ShellIdentity::class.java.getResourceAsStream(RESOURCE)?.use(loaded::load)
        }
    }

    val shellDigest: String get() = properties.getProperty("shell_digest") ?: UNKNOWN

    val implDigest: String get() = properties.getProperty("impl_digest") ?: UNKNOWN

    val jvmInstanceId: String by lazy {
        val system = System.getProperties()
        (system.putIfAbsent(JVM_INSTANCE_PROPERTY, UUID.randomUUID().toString()) as? String)
            ?: system.getProperty(JVM_INSTANCE_PROPERTY)
    }
}
