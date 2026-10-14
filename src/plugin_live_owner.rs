//! GH #238: which live JetBrains IDE processes can own a plugin install target.
//!
//! `agent-doc upgrade` writes a JetBrains package into one IDE data directory
//! (`target_dir`, the IDE's plugins path). Only a live IDE that loads plugins
//! from that directory can be running the generation being replaced, so only
//! such an IDE can make the write "replaced under a live IDE" and require a
//! restart. Before GH #238 every live JetBrains process on the host counted,
//! so a 263 IDE running from `IntelliJIdea2026.3` turned a write into the idle
//! `IntelliJIdea2026.2` into a "Restart IntelliJIdea2026.2" verdict and a
//! restart-required marker.
//!
//! The decision reads only process evidence, in order of strength:
//!
//! 1. an agent-doc plugin jar the process has mapped (`/proc/<pid>/maps`)
//!    under `target_dir` -- it demonstrably loads from there;
//! 2. an explicit `-Didea.plugins.path=` on its command line;
//! 3. its data directory name, from `-Didea.paths.selector=` or the IDE home's
//!    `product-info.json` `dataDirectoryName`, compared with `target_dir`'s
//!    data directory name;
//! 4. an agent-doc plugin jar mapped from some *other* plugin root.
//!
//! With no evidence the relation is [`PluginRootRelation::Unknown`], which the
//! installer keeps treating as a possible owner (the pre-GH-#238 behavior), so
//! missing evidence can never turn a real live owner into a silent plain
//! install.

use std::path::{Path, PathBuf};

/// How one live IDE process relates to the install target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRootRelation {
    /// The process loads plugins from the target; the reason names the evidence.
    Owns(String),
    /// The process provably loads plugins from somewhere else.
    Elsewhere(String),
    /// No conclusive evidence either way.
    Unknown,
}

impl PluginRootRelation {
    /// Whether the installer must treat the process as a possible owner.
    pub fn may_own(&self) -> bool {
        !matches!(self, PluginRootRelation::Elsewhere(_))
    }
}

/// What one process exposes about where it loads plugins from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdePluginRootEvidence {
    /// Every agent-doc plugin jar path the process maps (a ` (deleted)` suffix
    /// stripped: an unlinked jar still proves which root the process loaded).
    pub mapped_plugin_jars: Vec<PathBuf>,
    /// `-Didea.plugins.path=<dir>` from the command line.
    pub explicit_plugins_path: Option<PathBuf>,
    /// The IDE data directory name (`IntelliJIdea2026.3`), from
    /// `-Didea.paths.selector=` or `product-info.json`.
    pub data_directory_name: Option<String>,
}

/// The IDE data directory name `target_dir` belongs to: the directory itself
/// when it is the data root used as `idea.plugins.path`, or its parent for a
/// `<data dir>/plugins` layout. `None` for a custom plugins path that does not
/// look like a JetBrains data directory, where a name comparison proves nothing.
pub fn target_data_directory_name(target_dir: &Path) -> Option<String> {
    let name = target_dir.file_name()?.to_str()?;
    let candidate = if name == "plugins" {
        target_dir.parent()?.file_name()?.to_str()?
    } else {
        name
    };
    agent_doc_fs::jetbrains_install::is_jetbrains_ide_data_dir(candidate)
        .then(|| candidate.to_string())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left.components().eq(right.components()),
    }
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    if path.starts_with(root) {
        return true;
    }
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// Pure classifier: how a process with `evidence` relates to `target_dir`.
pub fn classify_plugin_root_relation(
    evidence: &IdePluginRootEvidence,
    target_dir: &Path,
) -> PluginRootRelation {
    if let Some(jar) = evidence
        .mapped_plugin_jars
        .iter()
        .find(|jar| path_is_under(jar, target_dir))
    {
        return PluginRootRelation::Owns(format!("maps {}", jar.display()));
    }
    if let Some(plugins_path) = &evidence.explicit_plugins_path {
        return if same_path(plugins_path, target_dir) {
            PluginRootRelation::Owns(format!("idea.plugins.path={}", plugins_path.display()))
        } else {
            PluginRootRelation::Elsewhere(format!("idea.plugins.path={}", plugins_path.display()))
        };
    }
    if let (Some(name), Some(target_name)) = (
        evidence.data_directory_name.as_deref(),
        target_data_directory_name(target_dir),
    ) {
        return if name == target_name {
            PluginRootRelation::Owns(format!("data directory {name}"))
        } else {
            PluginRootRelation::Elsewhere(format!("data directory {name}"))
        };
    }
    if let Some(jar) = evidence.mapped_plugin_jars.first() {
        return PluginRootRelation::Elsewhere(format!("maps {}", jar.display()));
    }
    PluginRootRelation::Unknown
}

/// The pathname column of `/proc/<pid>/maps` lines that name an agent-doc
/// JetBrains plugin jar. The pathname is everything after the fifth field and
/// may itself contain spaces.
pub fn mapped_plugin_jars_from_maps(maps: &str) -> Vec<PathBuf> {
    let mut jars = maps
        .lines()
        .filter_map(|line| {
            let mut rest = line.trim_start();
            for _ in 0..5 {
                let (_, tail) = rest.split_once(char::is_whitespace)?;
                rest = tail.trim_start();
            }
            let path = rest.trim_end();
            let path = path.strip_suffix(" (deleted)").unwrap_or(path);
            if !path.starts_with('/') {
                return None;
            }
            let name = Path::new(path).file_name()?.to_str()?;
            agent_doc_fs::plugin_jar::plugin_jar_version(
                name,
                agent_doc_fs::plugin_jar::JETBRAINS_PLUGIN_JAR_STEMS,
            )?;
            Some(PathBuf::from(path))
        })
        .collect::<Vec<_>>();
    jars.sort();
    jars.dedup();
    jars
}

fn system_property(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("-D{name}=");
    args.iter()
        .rev()
        .find_map(|arg| arg.strip_prefix(&prefix))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn product_info_data_directory_name(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join("product-info.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("dataDirectoryName")?
        .as_str()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// Candidate IDE home directories for a process: `-Didea.home.path=`, the
/// bundled runtime's `<home>/jbr/bin/java`, then a `<home>/bin/<launcher>`
/// argv[0].
fn ide_home_candidates(args: &[String], exe: Option<&Path>) -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(home) = system_property(args, "idea.home.path") {
        homes.push(PathBuf::from(home));
    }
    if let Some(exe) = exe
        && let Some(bin) = exe.parent()
        && bin.file_name().is_some_and(|name| name == "bin")
        && let Some(jbr) = bin.parent()
        && jbr.file_name().is_some_and(|name| name == "jbr")
        && let Some(home) = jbr.parent()
    {
        homes.push(home.to_path_buf());
    }
    if let Some(argv0) = args.first().map(Path::new)
        && argv0.is_absolute()
        && let Some(bin) = argv0.parent()
        && bin.file_name().is_some_and(|name| name == "bin")
        && let Some(home) = bin.parent()
    {
        homes.push(home.to_path_buf());
    }
    homes
}

/// Read one process's plugin-root evidence from `proc_root` (`/proc` in
/// production, a fixture tree in tests). Every unreadable source is simply
/// absent evidence.
pub fn ide_plugin_root_evidence(proc_root: &Path, pid: u32) -> IdePluginRootEvidence {
    let dir = proc_root.join(pid.to_string());
    let args = std::fs::read(dir.join("cmdline"))
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .split('\0')
                .filter(|arg| !arg.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mapped_plugin_jars = std::fs::read_to_string(dir.join("maps"))
        .map(|maps| mapped_plugin_jars_from_maps(&maps))
        .unwrap_or_default();
    let exe = std::fs::read_link(dir.join("exe")).ok();
    let data_directory_name = system_property(&args, "idea.paths.selector").or_else(|| {
        ide_home_candidates(&args, exe.as_deref())
            .iter()
            .find_map(|home| product_info_data_directory_name(home))
    });
    IdePluginRootEvidence {
        mapped_plugin_jars,
        explicit_plugins_path: system_property(&args, "idea.plugins.path").map(PathBuf::from),
        data_directory_name,
    }
}

/// Split `pids` into the processes that may own `target_dir` and the ones that
/// provably load plugins elsewhere (with the evidence for each).
pub fn partition_owner_candidates(
    proc_root: &Path,
    pids: Vec<u32>,
    target_dir: &Path,
) -> (Vec<u32>, Vec<(u32, String)>) {
    let mut candidates = Vec::new();
    let mut elsewhere = Vec::new();
    for pid in pids {
        let relation =
            classify_plugin_root_relation(&ide_plugin_root_evidence(proc_root, pid), target_dir);
        if relation.may_own() {
            candidates.push(pid);
        } else if let PluginRootRelation::Elsewhere(reason) = relation {
            elsewhere.push((pid, reason));
        }
    }
    (candidates, elsewhere)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake host: a `/proc` tree plus IDE homes and JetBrains data dirs.
    struct FakeHost {
        root: tempfile::TempDir,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                root: tempfile::tempdir().unwrap(),
            }
        }

        fn proc_root(&self) -> PathBuf {
            self.root.path().join("proc")
        }

        fn data_dir(&self, name: &str) -> PathBuf {
            let dir = self.root.path().join("share/JetBrains").join(name);
            std::fs::create_dir_all(dir.join("agent-doc-jetbrains/lib")).unwrap();
            dir
        }

        /// An IDE install at `home_name` whose product-info names `data_dir_name`.
        fn ide_home(&self, home_name: &str, data_dir_name: &str) -> PathBuf {
            let home = self.root.path().join("dist").join(home_name);
            std::fs::create_dir_all(home.join("bin")).unwrap();
            std::fs::create_dir_all(home.join("jbr/bin")).unwrap();
            std::fs::write(home.join("jbr/bin/java"), "").unwrap();
            std::fs::write(
                home.join("product-info.json"),
                format!(r#"{{"buildNumber":"263.6259.32","dataDirectoryName":"{data_dir_name}"}}"#),
            )
            .unwrap();
            home
        }

        fn process(&self, pid: u32, args: &[String], exe: Option<&Path>, mapped: &[PathBuf]) {
            let dir = self.proc_root().join(pid.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            let mut cmdline = args.join("\0");
            cmdline.push('\0');
            std::fs::write(dir.join("cmdline"), cmdline).unwrap();
            let maps = mapped
                .iter()
                .enumerate()
                .map(|(index, jar)| {
                    format!(
                        "7f00{index:04x}000-7f00{index:04x}fff r--s 00000000 103:02 {} {}\n",
                        1000 + index,
                        jar.display()
                    )
                })
                .collect::<String>();
            std::fs::write(
                dir.join("maps"),
                format!("55d0000-55d1000 r-xp 00000000 103:02 42 /usr/bin/java\n{maps}"),
            )
            .unwrap();
            if let Some(exe) = exe {
                std::os::unix::fs::symlink(exe, dir.join("exe")).unwrap();
            }
        }
    }

    fn jar(data_dir: &Path, name: &str) -> PathBuf {
        data_dir.join("agent-doc-jetbrains/lib").join(name)
    }

    /// GH #238, the reported host: one RemoteDev backend whose dist directory
    /// says 262 but whose product-info names `IntelliJIdea2026.3`, mapping the
    /// plugin from 2026.3. A write into the idle 2026.2 data dir has no owner.
    #[test]
    fn gh238_live_ide_on_other_data_dir_does_not_own_target() {
        let host = FakeHost::new();
        let idle = host.data_dir("IntelliJIdea2026.2");
        let live = host.data_dir("IntelliJIdea2026.3");
        let home = host.ide_home("IU-262.9437.185", "IntelliJIdea2026.3");
        host.process(
            3274346,
            &[
                home.join("bin/idea").display().to_string(),
                "serverMode".to_string(),
            ],
            Some(&home.join("jbr/bin/java")),
            &[jar(&live, "agent-doc-jetbrains-0.2.508.jar")],
        );

        let (candidates, elsewhere) =
            partition_owner_candidates(&host.proc_root(), vec![3274346], &idle);
        assert!(candidates.is_empty(), "{candidates:?}");
        assert_eq!(elsewhere.len(), 1);
        assert!(
            elsewhere[0].1.contains("IntelliJIdea2026.3"),
            "{elsewhere:?}"
        );

        // The same IDE is a real owner of its own data dir.
        let (candidates, _) = partition_owner_candidates(&host.proc_root(), vec![3274346], &live);
        assert_eq!(candidates, vec![3274346]);
    }

    /// Product-info alone (no agent-doc jar mapped yet) still decides.
    #[test]
    fn product_info_data_directory_decides_without_mapped_jar() {
        let host = FakeHost::new();
        let idle = host.data_dir("IntelliJIdea2026.2");
        let same = host.data_dir("IntelliJIdea2026.3");
        let home = host.ide_home("idea-IU-263", "IntelliJIdea2026.3");
        host.process(
            7,
            &[
                "/usr/lib/jvm/java".to_string(),
                "com.intellij.idea.Main".to_string(),
            ],
            Some(&home.join("jbr/bin/java")),
            &[],
        );
        let evidence = ide_plugin_root_evidence(&host.proc_root(), 7);
        assert_eq!(
            evidence.data_directory_name.as_deref(),
            Some("IntelliJIdea2026.3")
        );
        assert!(!classify_plugin_root_relation(&evidence, &idle).may_own());
        assert!(matches!(
            classify_plugin_root_relation(&evidence, &same),
            PluginRootRelation::Owns(_)
        ));
        // `<data dir>/plugins` layout names the same data dir.
        assert!(matches!(
            classify_plugin_root_relation(&evidence, &same.join("plugins")),
            PluginRootRelation::Owns(_)
        ));
    }

    /// True positive: an IDE mapping the plugin from the target owns it, even
    /// when the mapping is an unlinked (replaced) jar.
    #[test]
    fn mapped_jar_under_target_owns_it() {
        let host = FakeHost::new();
        let target = host.data_dir("IntelliJIdea2026.2");
        let deleted = PathBuf::from(format!(
            "{} (deleted)",
            jar(&target, "agent.doc-0.2.526.jar").display()
        ));
        host.process(9, &["java".to_string()], None, &[deleted]);
        let relation =
            classify_plugin_root_relation(&ide_plugin_root_evidence(&host.proc_root(), 9), &target);
        assert!(
            matches!(relation, PluginRootRelation::Owns(_)),
            "{relation:?}"
        );
    }

    #[test]
    fn explicit_plugins_path_outranks_data_directory_name() {
        let host = FakeHost::new();
        let target = host.data_dir("IntelliJIdea2026.2");
        let custom = host.root.path().join("custom-plugins");
        let home = host.ide_home("idea", "IntelliJIdea2026.2");
        host.process(
            11,
            &[
                "java".to_string(),
                format!("-Didea.plugins.path={}", custom.display()),
            ],
            Some(&home.join("jbr/bin/java")),
            &[],
        );
        assert!(
            !classify_plugin_root_relation(
                &ide_plugin_root_evidence(&host.proc_root(), 11),
                &target
            )
            .may_own()
        );
        assert!(matches!(
            classify_plugin_root_relation(
                &ide_plugin_root_evidence(&host.proc_root(), 11),
                &custom
            ),
            PluginRootRelation::Owns(_)
        ));
    }

    #[test]
    fn paths_selector_names_the_data_directory() {
        let host = FakeHost::new();
        let target = host.data_dir("IntelliJIdea2026.2");
        host.process(
            13,
            &[
                "java".to_string(),
                "-Didea.paths.selector=IntelliJIdea2026.2".to_string(),
            ],
            None,
            &[],
        );
        assert!(matches!(
            classify_plugin_root_relation(
                &ide_plugin_root_evidence(&host.proc_root(), 13),
                &target
            ),
            PluginRootRelation::Owns(_)
        ));
    }

    /// Missing evidence never manufactures a non-owner: the installer keeps the
    /// pre-GH-#238 conservative verdict for such a process.
    #[test]
    fn no_evidence_is_unknown_and_kept_as_candidate() {
        let host = FakeHost::new();
        let target = host.data_dir("IntelliJIdea2026.2");
        host.process(17, &["java".to_string()], None, &[]);
        let (candidates, elsewhere) =
            partition_owner_candidates(&host.proc_root(), vec![17, 18], &target);
        assert_eq!(candidates, vec![17, 18]);
        assert!(elsewhere.is_empty());
    }

    #[test]
    fn mapped_jar_from_other_root_is_elsewhere_for_custom_target() {
        let host = FakeHost::new();
        let other = host.data_dir("IntelliJIdea2026.3");
        let custom = host.root.path().join("custom-plugins");
        host.process(
            19,
            &["java".to_string()],
            None,
            &[jar(&other, "agent.doc-0.2.530.jar")],
        );
        assert!(
            !classify_plugin_root_relation(
                &ide_plugin_root_evidence(&host.proc_root(), 19),
                &custom
            )
            .may_own()
        );
    }

    #[test]
    fn maps_parser_keeps_paths_with_spaces_and_ignores_other_jars() {
        let maps = "\
7f00-7f01 r--s 00000000 103:02 5 /opt/JetBrains Rider-2026.2/plugins/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.500.jar
7f02-7f03 r--s 00000000 103:02 6 /opt/idea/lib/app.jar
7f04-7f05 rw-p 00000000 00:00 0
7f06-7f07 r--s 00000000 103:02 7 /d/agent-doc-jetbrains/lib/agent.doc-0.2.530.jar (deleted)
7f08-7f09 r--s 00000000 103:02 8 /d/agent-doc-jetbrains-263/lib/agent-doc-jetbrains-263-0.2.531.jar
";
        assert_eq!(
            mapped_plugin_jars_from_maps(maps),
            vec![
                PathBuf::from("/d/agent-doc-jetbrains/lib/agent.doc-0.2.530.jar"),
                PathBuf::from("/d/agent-doc-jetbrains-263/lib/agent-doc-jetbrains-263-0.2.531.jar"),
                PathBuf::from(
                    "/opt/JetBrains Rider-2026.2/plugins/agent-doc-jetbrains/lib/agent-doc-jetbrains-0.2.500.jar"
                ),
            ]
        );
    }
}
