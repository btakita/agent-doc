@file:Suppress("UnstableApiUsage")

package com.github.btakita.agentdoc.split.backend

import com.github.btakita.agentdoc.split.FrontendReleaseRpcApi
import com.github.btakita.agentdoc.split.SurfaceSnapshotRpcApi
import com.intellij.platform.rpc.backend.RemoteApiProvider
import fleet.rpc.remoteApiDescriptor

internal class SurfaceSnapshotRemoteApiProvider : RemoteApiProvider {
    override fun RemoteApiProvider.Sink.remoteApis() {
        remoteApi(remoteApiDescriptor<SurfaceSnapshotRpcApi>()) {
            BackendSurfaceSnapshotRpcApi()
        }
        // GH #435: additive, so an older Client never sees it and the frozen API above stays exact.
        remoteApi(remoteApiDescriptor<FrontendReleaseRpcApi>()) {
            BackendFrontendReleaseRpcApi()
        }
    }
}
