plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm")
}

dependencies {
    implementation(project(":shared"))
}

tasks.jar {
    archiveFileName.set("agent-doc-frontend-impl-${project.version}.jar")
    manifest {
        attributes(
            "Agent-Doc-Impl-Class" to
                "com.github.btakita.agentdoc.impl.frontend.AgentDocFrontendImplementation",
            "Agent-Doc-Impl-Version" to project.version.toString(),
            "Agent-Doc-Shell-Spi-Version" to "1",
            "Implementation-Version" to project.version.toString(),
        )
    }
}
