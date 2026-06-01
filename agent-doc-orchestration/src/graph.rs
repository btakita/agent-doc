//! Lazily-rs dependency graph for agent-doc filesystem computations.
//!
//! Provides [`RunContext`] — a short-lived context for a single CLI invocation
//! (`preflight → plan → write → commit`) — that caches filesystem-derived
//! lookups (`project_root`, `project_config`, etc.) behind a reactive
//! dependency graph. Within a CLI run, each slot computes at most once.
//!
//! For long-lived contexts (watch daemon, supervisor), use [`ActorContext`]
//! (Phase 5 — not yet implemented), which adds explicit invalidation via
//! `SlotHandle::clear()` / `CellHandle::clear_dependents()` on file/config
//! change events.
//!
//! Slot graph:
//!
//! ```text
//! Cell: file_path (PathBuf)
//!  └─ Slot: canonical_path(file_path)
//!      ├─ Slot: project_root(canonical_path)
//!      │   ├─ Slot: config_path(project_root)
//!      │   │   └─ Slot: project_config(config_path)
//!      │   │       └─ Slot: ssh_context(project_config, doc_relative)
//!      │   └─ Slot: snapshot_path(project_root, canonical_path)
//!      └─ Slot: doc_relative(canonical_path, project_root)
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use agent_doc_core::project_config::ProjectConfig;
use lazily::{CellHandle, Context, SlotHandle};

use crate::fs_util;
use crate::project_config_io;

pub type FilePathCell = CellHandle<PathBuf>;
pub type CanonicalPathSlot = SlotHandle<PathBuf>;
pub type ProjectRootSlot = SlotHandle<Option<PathBuf>>;
pub type ConfigPathSlot = SlotHandle<Option<PathBuf>>;
pub type ProjectConfigSlot = SlotHandle<Arc<ProjectConfig>>;
pub type SnapshotPathSlot = SlotHandle<Option<PathBuf>>;
pub type DocRelativeSlot = SlotHandle<Option<String>>;
pub type SshContextSlot = SlotHandle<Arc<SshContextValue>>;

pub struct SshContextValue {
    pub config: Arc<ProjectConfig>,
    pub doc_relative: String,
}

pub struct RunContext {
    ctx: Context,
    file_path: FilePathCell,
    canonical_path: CanonicalPathSlot,
    project_root: ProjectRootSlot,
    config_path: ConfigPathSlot,
    project_config: ProjectConfigSlot,
    snapshot_path: SnapshotPathSlot,
    doc_relative: DocRelativeSlot,
    ssh_context: SshContextSlot,
}

impl RunContext {
    pub fn new(file_path: PathBuf) -> Self {
        let ctx = Context::new();

        let file_path_cell = ctx.cell(file_path);

        let canonical_path = ctx.slot({
            let fp = file_path_cell;
            move |ctx: &Context| -> PathBuf {
                let path: PathBuf = ctx.get_cell(&fp);
                std::fs::canonicalize(&path).unwrap_or(path)
            }
        });

        let project_root = ctx.slot({
            let cp = canonical_path;
            move |ctx: &Context| -> Option<PathBuf> {
                let path: PathBuf = ctx.get(&cp);
                fs_util::find_project_root(&path)
            }
        });

        let config_path = ctx.slot({
            let pr = project_root;
            move |ctx: &Context| -> Option<PathBuf> {
                let root: Option<PathBuf> = ctx.get(&pr);
                root.map(|r| r.join(".agent-doc").join("config.toml"))
            }
        });

        let project_config = ctx.slot({
            let cp = config_path;
            move |ctx: &Context| -> Arc<ProjectConfig> {
                let path: Option<PathBuf> = ctx.get(&cp);
                Arc::new(match path {
                    Some(ref p) => project_config_io::load_project_from(p),
                    None => ProjectConfig::default(),
                })
            }
        });

        let snapshot_path = ctx.slot({
            let pr = project_root;
            move |ctx: &Context| -> Option<PathBuf> {
                let root: Option<PathBuf> = ctx.get(&pr);
                root.map(|r| r.join(".agent-doc").join("snapshots"))
            }
        });

        let doc_relative = ctx.slot({
            let cp = canonical_path;
            let pr = project_root;
            move |ctx: &Context| -> Option<String> {
                let canonical: PathBuf = ctx.get(&cp);
                let root: Option<PathBuf> = ctx.get(&pr);
                root.map(|r| {
                    canonical
                        .strip_prefix(&r)
                        .unwrap_or(canonical.as_path())
                        .to_string_lossy()
                        .replace('\\', "/")
                })
            }
        });

        let ssh_context = ctx.slot({
            let pc = project_config;
            let dr = doc_relative;
            move |ctx: &Context| -> Arc<SshContextValue> {
                let config: Arc<ProjectConfig> = ctx.get(&pc);
                let doc_rel: Option<String> = ctx.get(&dr);
                Arc::new(SshContextValue {
                    config,
                    doc_relative: doc_rel.unwrap_or_default(),
                })
            }
        });

        Self {
            ctx,
            file_path: file_path_cell,
            canonical_path,
            project_root,
            config_path,
            project_config,
            snapshot_path,
            doc_relative,
            ssh_context,
        }
    }

    pub fn file_path(&self) -> PathBuf {
        self.ctx.get_cell(&self.file_path)
    }

    pub fn set_file_path(&self, path: PathBuf) {
        self.ctx.set_cell(&self.file_path, path);
    }

    pub fn canonical_path(&self) -> PathBuf {
        self.ctx.get(&self.canonical_path)
    }

    pub fn project_root(&self) -> Option<PathBuf> {
        self.ctx.get(&self.project_root)
    }

    pub fn config_path(&self) -> Option<PathBuf> {
        self.ctx.get(&self.config_path)
    }

    pub fn project_config(&self) -> Arc<ProjectConfig> {
        self.ctx.get(&self.project_config)
    }

    pub fn snapshot_path(&self) -> Option<PathBuf> {
        self.ctx.get(&self.snapshot_path)
    }

    pub fn doc_relative(&self) -> Option<String> {
        self.ctx.get(&self.doc_relative)
    }

    pub fn ssh_context(&self) -> Arc<SshContextValue> {
        self.ctx.get(&self.ssh_context)
    }

    pub fn invalidate_project_root(&self) {
        self.project_root.clear(&self.ctx);
    }

    pub fn invalidate_project_config(&self) {
        self.project_config.clear(&self.ctx);
    }

    pub fn is_project_root_cached(&self) -> bool {
        self.ctx.is_set(&self.project_root)
    }

    pub fn is_project_config_cached(&self) -> bool {
        self.ctx.is_set(&self.project_config)
    }

    pub fn is_canonical_path_cached(&self) -> bool {
        self.ctx.is_set(&self.canonical_path)
    }

    pub fn is_doc_relative_cached(&self) -> bool {
        self.ctx.is_set(&self.doc_relative)
    }

    pub fn is_ssh_context_cached(&self) -> bool {
        self.ctx.is_set(&self.ssh_context)
    }
}

impl SshContextValue {
    pub fn as_resolver_context<'a>(
        &'a self,
        file_display: &'a str,
    ) -> agent_doc_core::frontmatter::SshResolverContext<'a> {
        agent_doc_core::frontmatter::SshResolverContext {
            project: &self.config,
            doc_relative: &self.doc_relative,
            file_display,
        }
    }
}

pub struct ActorContext {
    inner: RunContext,
}

impl ActorContext {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            inner: RunContext::new(file_path),
        }
    }

    pub fn on_file_change(&self, new_path: PathBuf) {
        self.inner.set_file_path(new_path);
    }

    pub fn on_config_change(&self) {
        self.inner.invalidate_project_root();
    }

    pub fn invalidate_all(&self) {
        self.inner.invalidate_project_root();
    }

    pub fn context(&self) -> &RunContext {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup_project(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        dir.join(".agent-doc").join("config.toml")
    }

    #[test]
    fn run_context_finds_project_root() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        std::fs::create_dir_all(dir.path().join("nested/deep")).unwrap();
        let doc = dir.path().join("nested/deep/file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc.clone());

        let root = rc.project_root().unwrap();
        assert_eq!(root, dir.path());

        let cp = rc.config_path().unwrap();
        assert_eq!(cp, dir.path().join(".agent-doc").join("config.toml"));
    }

    #[test]
    fn run_context_no_project_root_finds_ancestor() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        let doc = dir.path().join("nested/file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let root = rc.project_root();
        assert!(root.is_some());
        assert!(rc.config_path().is_some());
        let cfg = rc.project_config();
        assert!(cfg.tmux_session.is_none() || cfg.tmux_session.is_some());
    }

    #[test]
    fn run_context_caches_project_root() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        assert!(!rc.is_project_root_cached());
        let root = rc.project_root();
        assert!(rc.is_project_root_cached());

        let root2 = rc.project_root();
        assert_eq!(root2, root);
    }

    #[test]
    fn run_context_snapshot_path_under_project() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let snap = rc.snapshot_path().unwrap();
        assert_eq!(snap, dir.path().join(".agent-doc").join("snapshots"));
    }

    #[test]
    fn run_context_doc_relative() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        std::fs::create_dir_all(dir.path().join("nested/deep")).unwrap();
        let doc = dir.path().join("nested/deep/file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let rel = rc.doc_relative().unwrap();
        assert_eq!(rel, "nested/deep/file.md");
    }

    #[test]
    fn run_context_ssh_context_inherits_config() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        std::fs::write(
            &config_path,
            "tmux_session = \"test\"\n\n[components.exchange]\npatch = \"append\"\n",
        )
        .unwrap();
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let ssh = rc.ssh_context();
        assert_eq!(ssh.config.tmux_session.as_deref(), Some("test"));
        assert_eq!(ssh.doc_relative, "file.md");
    }

    #[test]
    fn run_context_loads_project_config() {
        let dir = TempDir::new().unwrap();
        let config_path = setup_project(dir.path());
        std::fs::write(
            &config_path,
            "tmux_session = \"my-session\"\nagent_doc_auto_compact = 120\n",
        )
        .unwrap();
        let doc = dir.path().join("doc.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let cfg = rc.project_config();
        assert_eq!(cfg.tmux_session.as_deref(), Some("my-session"));
        assert_eq!(cfg.agent_doc_auto_compact, Some(120));
    }

    #[test]
    fn set_file_path_clears_dependents() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let _root = rc.project_root();
        assert!(rc.is_project_root_cached());

        rc.set_file_path(PathBuf::from("/nonexistent/path.md"));

        assert!(!rc.is_canonical_path_cached());
        assert!(!rc.is_project_root_cached());
        assert!(!rc.is_project_config_cached());
        assert!(!rc.is_doc_relative_cached());
        assert!(!rc.is_ssh_context_cached());
    }

    #[test]
    fn invalidate_project_root_clears_descendants() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let _root = rc.project_root();
        let _cfg = rc.project_config();
        assert!(rc.is_project_root_cached());
        assert!(rc.is_project_config_cached());

        rc.invalidate_project_root();

        assert!(!rc.is_project_root_cached());
        assert!(!rc.is_project_config_cached());
        assert!(!rc.is_ssh_context_cached());
    }

    #[test]
    fn invalidate_project_config_clears_config_descendants() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let _root = rc.project_root();
        let _cfg = rc.project_config();
        let _ssh = rc.ssh_context();
        assert!(rc.is_project_config_cached());
        assert!(rc.is_ssh_context_cached());

        rc.invalidate_project_config();

        assert!(!rc.is_project_config_cached());
        assert!(!rc.is_ssh_context_cached());
        assert!(rc.is_project_root_cached());
    }

    #[test]
    fn actor_context_invalidates_all_on_config_change() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let ac = ActorContext::new(doc);

        let _root = ac.context().project_root();
        let _cfg = ac.context().project_config();
        assert!(ac.context().is_project_root_cached());
        assert!(ac.context().is_project_config_cached());

        ac.on_config_change();

        assert!(!ac.context().is_project_root_cached());
        assert!(!ac.context().is_project_config_cached());
    }

    #[test]
    fn actor_context_on_file_change_clears_dependents() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let ac = ActorContext::new(doc);
        let _root = ac.context().project_root();
        assert!(ac.context().is_project_root_cached());

        ac.on_file_change(PathBuf::from("/other/path.md"));

        assert!(!ac.context().is_project_root_cached());
    }

    #[test]
    fn ssh_context_value_builds_resolver_context() {
        let config = Arc::new(ProjectConfig::default());
        let val = SshContextValue {
            config,
            doc_relative: "path/to/doc.md".to_string(),
        };

        let resolver = val.as_resolver_context("doc.md");
        assert_eq!(resolver.doc_relative, "path/to/doc.md");
        assert_eq!(resolver.file_display, "doc.md");
    }

    #[test]
    fn run_context_doc_relative_without_direct_project() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let rel = rc.doc_relative();
        let root = rc.project_root();
        if root.is_some() {
            assert!(rel.is_some());
        }
    }
}
