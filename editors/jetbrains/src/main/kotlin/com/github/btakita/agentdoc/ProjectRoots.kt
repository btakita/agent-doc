package com.github.btakita.agentdoc

import java.io.File
import java.io.IOException

/** Local discovery for editor paths that avoid a native call. */
internal object ProjectRoots {
    internal val processPolicy by lazy {
        Policy(
            home = listOfNotNull(System.getenv("HOME"), System.getenv("USERPROFILE"), System.getProperty("user.home"))
                .map(::File).firstOrNull(File::isAbsolute),
            temp = listOfNotNull(System.getenv("TMPDIR"), System.getProperty("java.io.tmpdir"))
                .map(::File).filter(File::isAbsolute) +
                if (File.separatorChar == '/') listOf(File("/tmp"), File("/var/tmp")) else emptyList(),
            ceilings = System.getenv("AGENT_DOC_ROOT_CEILING_DIRECTORIES").orEmpty()
                .split(File.pathSeparatorChar).map(::File).filter(File::isAbsolute),
            allowHome = System.getenv("AGENT_DOC_ALLOW_HOME_ROOT") == "1",
        )
    }

    internal class Policy(home: File?, temp: List<File>, ceilings: List<File>, private val allowHome: Boolean) {
        private val home = home?.let(::canonical)
        private val temp = temp.map(::canonical).toSet()
        private val ceilings = ceilings.flatMap { listOf(it.absoluteFile.normalize(), canonical(it)) }.toSet()

        fun allows(dir: File): Boolean {
            val path = canonical(dir)
            return path.parentFile != null && path !in temp &&
                (path != home || allowHome || File(path, ".git").exists())
        }

        fun find(start: File): File? {
            val first = start.absoluteFile.normalize()
            var current: File? = first
            while (current != null) {
                val atCeiling = ceilings.isNotEmpty() && (current in ceilings || canonical(current) in ceilings)
                if (atCeiling && current != first) break
                if (File(current, ".agent-doc").isDirectory && allows(current)) return current
                if (atCeiling) break
                current = current.parentFile
            }
            return null
        }

        fun allowsFallback(start: File, candidate: File): Boolean {
            if (!allows(candidate)) return false
            if (ceilings.isEmpty()) return true
            val first = canonical(start)
            val root = canonical(candidate)
            val ceiling = first.toPath().let { path ->
                ceilings.filter { path.startsWith(it.toPath()) }.maxByOrNull { it.toPath().nameCount }
            } ?: return true
            return root.toPath().startsWith(ceiling.toPath()) && (root != ceiling || first == ceiling)
        }
    }

    private fun canonical(file: File): File = try {
        file.canonicalFile
    } catch (_: IOException) {
        file.absoluteFile.normalize()
    }
}
