package com.github.btakita.agentdoc.split

/*
 * GH #441: lease metadata the frontend sends inside `SurfaceSnapshotRpcApi.openIngress`'s
 * `frontendInstanceId`, without a new RPC method or a new snapshot key.
 *
 * A schema-1 snapshot means the backend's schema answer did not reach the frontend, so the frontend
 * cannot know which keys that backend decodes (Fleet RPC decodes with `ignoreUnknownKeys = false`).
 * The instance id is already an opaque string every backend only compares for equality, so a
 * backend that predates this metadata keeps working unchanged, and a current backend can tell a
 * provisional schema 1 from its own plugin (which asks again after a refusal) from a schema 1 that
 * names another plugin, or none.
 */

/** Separates the frontend's per-JVM instance UUID from the Agent Doc plugin version it runs. */
const val FRONTEND_LEASE_PLUGIN_SEPARATOR: String = ";plugin="

/** The `frontendInstanceId` of one frontend publisher that runs Agent Doc [pluginVersion]. */
fun frontendLeaseInstanceId(instance: String, pluginVersion: String?): String {
    val version = pluginVersion?.trim()?.takeIf { it.isNotEmpty() && it != "unknown" }
        ?: return instance
    return "$instance$FRONTEND_LEASE_PLUGIN_SEPARATOR$version"
}

/**
 * The Agent Doc plugin version a frontend named in its `frontendInstanceId`, or null when it named
 * none: every frontend before GH #441 (0.2.1052 and earlier) sends a bare UUID.
 */
fun frontendLeasePluginVersion(frontendInstanceId: String): String? =
    frontendInstanceId.substringAfter(FRONTEND_LEASE_PLUGIN_SEPARATOR, missingDelimiterValue = "")
        .trim()
        .takeIf(String::isNotEmpty)
