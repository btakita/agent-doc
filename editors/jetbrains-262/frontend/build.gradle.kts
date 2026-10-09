dependencies {
    intellijPlatform {
        bundledModule("intellij.platform.frontend")
        bundledModule("intellij.terminal.frontend")
        bundledPlugin("org.jetbrains.plugins.terminal")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-core-jvm:1.9.0")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json-jvm:1.9.0")
    }

    implementation(project(":shared"))
    testImplementation(kotlin("test"))
}
