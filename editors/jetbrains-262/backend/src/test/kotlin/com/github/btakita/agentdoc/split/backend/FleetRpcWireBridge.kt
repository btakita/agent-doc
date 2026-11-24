@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import fleet.rpc.RemoteApi
import fleet.rpc.RemoteApiDescriptor
import fleet.rpc.RemoteKind
import fleet.rpc.core.rpcJsonImplementationDetail
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonElement

/**
 * GH #435: a Client built against [clientApi] calling a backend that serves [backendApi] with
 * [backend], through the compiler-generated Fleet RPC descriptors of both interfaces.
 *
 * It reproduces what Fleet RPC puts on the wire (`RpcMessage.CallRequest`: the method NAME and a map
 * of parameter NAME to JSON; `CallResult`: the result JSON), encoded and decoded with Fleet's own
 * `RpcJson` (`ignoreUnknownKeys = false`). The Client side serializes with its interface's
 * signature, the backend side deserializes with its own, so a renamed method or parameter, a changed
 * parameter type, or a key the Client cannot decode fails here exactly as it would over Remote Dev.
 */
internal fun <C : RemoteApi<*>, B : RemoteApi<*>> fleetRpcWireClient(
    clientApi: RemoteApiDescriptor<C>,
    backendApi: RemoteApiDescriptor<B>,
    backend: B,
    onCall: (method: String) -> Unit = {},
): C = clientApi.clientStub { method, args ->
    onCall(method)
    val clientSignature = clientApi.getSignature(method)
    val backendSignature = backendApi.getSignature(method)
    val request: Map<String, JsonElement> = clientSignature.parameters.withIndex().associate { (index, parameter) ->
        parameter.parameterName to encode(parameter.parameterKind, args[index])
    }
    val expected = backendSignature.parameters.map { it.parameterName }
    check(request.keys.toList() == expected) {
        "$method: the Client sends parameters ${request.keys} but the backend expects $expected"
    }
    val decodedArgs = backendSignature.parameters.map { parameter ->
        decode(parameter.parameterKind, request.getValue(parameter.parameterName))
    }.toTypedArray()
    val result = backendApi.call(backend, method, decodedArgs)
    translate(backendSignature.returnType, clientSignature.returnType, result)
}

private val rpcJson = rpcJsonImplementationDetail()

@Suppress("UNCHECKED_CAST")
private fun encode(kind: RemoteKind, value: Any?): JsonElement = when (kind) {
    is RemoteKind.Data -> rpcJson.encodeToJsonElement(kind.serializer as KSerializer<Any?>, value)
    else -> error("only data parameters cross this bridge, not $kind")
}

@Suppress("UNCHECKED_CAST")
private fun decode(kind: RemoteKind, json: JsonElement): Any? = when (kind) {
    is RemoteKind.Data -> rpcJson.decodeFromJsonElement(kind.serializer as KSerializer<Any?>, json)
    else -> error("only data parameters cross this bridge, not $kind")
}

/** A backend result re-read the way the Client decodes it; a flow is translated element by element. */
@Suppress("UNCHECKED_CAST")
private fun translate(backendKind: RemoteKind, clientKind: RemoteKind, value: Any?): Any? = when {
    backendKind is RemoteKind.Data && clientKind is RemoteKind.Data -> decode(clientKind, encode(backendKind, value))
    backendKind is RemoteKind.Flow && clientKind is RemoteKind.Flow -> (value as Flow<Any?>?)?.map { element ->
        translate(backendKind.elementKind, clientKind.elementKind, element)
    }
    else -> error("the backend returns $backendKind where the Client expects $clientKind")
}
