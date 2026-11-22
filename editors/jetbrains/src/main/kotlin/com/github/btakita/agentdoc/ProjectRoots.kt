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
            // `#strayskillroot`: mirrors `agent_doc_fs::project_root_allowed` — an
            // installed harness skill tree is never a project, by either spelling.
            if (isWithinHarnessSkillInstallDir(dir)) return false
            val path = canonical(dir)
            if (isWithinHarnessSkillInstallDir(path)) return false
            return path.parentFile != null && path !in temp &&
                (path != home || allowHome || File(path, ".git").exists())
        }

        /**
         * Mirrors `agent_doc_fs::find_project_root`: the nearest *marked* project
         * ([isMarkedProjectRoot]) wins over a nearer state-only `.agent-doc/`; a
         * state-only directory is the root only when no marked project encloses it.
         */
        fun find(start: File): File? {
            val first = start.absoluteFile.normalize()
            var current: File? = first
            var nearestStateOnly: File? = null
            while (current != null) {
                val atCeiling = ceilings.isNotEmpty() && (current in ceilings || canonical(current) in ceilings)
                if (atCeiling && current != first) break
                if (File(current, ".agent-doc").isDirectory && allows(current)) {
                    if (isMarkedProjectRoot(current)) return current
                    if (nearestStateOnly == null) nearestStateOnly = current
                }
                if (atCeiling) break
                current = current.parentFile
            }
            return nearestStateOnly
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

    /** Mirrors `agent_doc_fs::HARNESS_SKILL_INSTALL_DIRS`. */
    internal val HARNESS_SKILL_INSTALL_DIRS = listOf(
        ".claude" to "skills",
        ".agents" to "skills",
        ".codex" to "skills",
        ".grok" to "skills",
        ".opencode" to "skills",
        ".cursor" to "rules",
    )

    /** Mirrors `agent_doc_fs::PROJECT_MARKER_FILES`. */
    internal val PROJECT_MARKER_FILES = listOf("config.toml", "project.md", "sessions.json")

    /** Whether [path] is, or lies inside, a harness skill/rule install directory. Lexical. */
    internal fun isWithinHarnessSkillInstallDir(path: File): Boolean {
        val names = try {
            path.toPath().normalize().map { it.toString() }
        } catch (_: Exception) {
            return false
        }
        return names.zipWithNext().any { pair -> pair in HARNESS_SKILL_INSTALL_DIRS }
    }

    /** Mirrors `agent_doc_fs::is_marked_project_root`. */
    internal fun isMarkedProjectRoot(dir: File): Boolean {
        val state = File(dir, ".agent-doc")
        return PROJECT_MARKER_FILES.any { File(state, it).isFile } || File(dir, ".git").exists()
    }

    private fun canonical(file: File): File = try {
        file.canonicalFile
    } catch (_: IOException) {
        file.absoluteFile.normalize()
    }
}
