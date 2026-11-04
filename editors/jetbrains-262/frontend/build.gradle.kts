import org.jetbrains.intellij.platform.gradle.TestFrameworkType

dependencies {
    intellijPlatform {
        bundledModule("intellij.platform.frontend")
        bundledModule("intellij.terminal.frontend")
        bundledPlugin("org.jetbrains.plugins.terminal")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-core-jvm:1.9.0")
        compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json-jvm:1.9.0")
        // GH #391: MarkdownBracketWrapStrategyPlatformTest drives the bundled Markdown wrap
        // strategy in a light editor. Test-only; the module descriptor stays Markdown-free.
        testFramework(TestFrameworkType.Platform)
        testBundledPlugin("org.intellij.plugins.markdown")
    }

    implementation(project(":shared"))
    testImplementation(kotlin("test"))
    testImplementation("junit:junit:4.13.2")
}
