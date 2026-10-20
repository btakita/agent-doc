dependencies {
    intellijPlatform {
        bundledModule("intellij.platform.rpc")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-core-jvm:1.9.0")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json-jvm:1.9.0")
    }

    testImplementation(kotlin("test"))
    testImplementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.9.0")
}
