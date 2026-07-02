//! Lazily-rs dependency graph for agent-doc filesystem computations.
//!
//! Provides [`RunContext`] — a short-lived context for a single CLI invocation
//! (`preflight → plan → write → commit`) — that caches filesystem-derived
//! lookups (`project_root`, `project_config`, etc.) behind a reactive
//! dependency graph. Within a CLI run, each slot computes at most once.
//!
//! For long-lived contexts (watch daemon, supervisor), use [`ActorContext`],
//! which adds explicit invalidation via `SlotHandle::clear()` /
//! `CellHandle::clear_dependents()` on file/config change events.
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

use agent_doc_element::element::{self, Component};
use agent_doc_frontmatter::frontmatter::{self, Frontmatter};
use agent_doc_frontmatter::project_config::ProjectConfig;
use agent_doc_frontmatter_io::session::ResolvedSshContext;
use lazily::{CellHandle, Context, SlotHandle};

use agent_doc_cycle_state_io::CycleState;
use agent_doc_project_config_io as project_config_io;

pub type FilePathCell = CellHandle<PathBuf>;
pub type DocContentCell = CellHandle<String>;
pub type CanonicalPathSlot = SlotHandle<PathBuf>;
pub type ProjectRootSlot = SlotHandle<Option<PathBuf>>;
pub type ConfigPathSlot = SlotHandle<Option<PathBuf>>;
pub type ProjectConfigSlot = SlotHandle<Arc<ProjectConfig>>;
pub type SnapshotPathSlot = SlotHandle<Option<PathBuf>>;
pub type DocRelativeSlot = SlotHandle<Option<String>>;
pub type SshContextSlot = SlotHandle<Arc<ResolvedSshContext>>;
pub type FrontmatterSlot = SlotHandle<Arc<Frontmatter>>;
pub type ComponentsSlot = SlotHandle<Arc<Vec<Component>>>;
pub type DocHashSlot = SlotHandle<String>;
/// Phase 7 (#lr-cycle-7): cached per-document cycle state, loaded at most once
/// per context lifetime. `None` when no cycle-state sidecar exists yet.
pub type CycleStateSlot = SlotHandle<Option<Arc<CycleState>>>;
/// Phase 7 (#lr-cycle-7): cached snapshot content, loaded (with flock) at most
/// once per context lifetime. `None` when no snapshot exists yet.
pub type SnapshotContentSlot = SlotHandle<Option<Arc<String>>>;
/// Phase 8 (#lr-head-8): cached `git show HEAD:<doc>` content, spawned at
/// most once per context lifetime. `None` when the document is not tracked or
/// HEAD cannot provide content.
pub type HeadContentSlot = SlotHandle<Option<Arc<String>>>;
/// Phase 8 (#lr-head-8): cached comparison of snapshot content against HEAD.
pub type SnapshotCommitStatusSlot = SlotHandle<agent_doc_snapshot_io::SnapshotCommitStatus>;
/// Phase 9 (#lr-wire-9): cached harness detection. Harness env vars are
/// process-static for a CLI run, so compute once per [`RunContext`].
pub type HarnessSlot = SlotHandle<String>;
/// Phase 10 (#lr-actor-10): cached global user configuration. For CLI runs this
/// is process-static; for long-lived actors it is invalidated by config-change
/// events before the next read.
pub type GlobalConfigSlot = SlotHandle<Arc<agent_doc_config::Config>>;
/// Phase 10 (#lr-actor-10): cached session registry for the current document's
/// project root. Read-modify-write callers still load under `RegistryLock`.
pub type SessionRegistrySlot = SlotHandle<Arc<tmux_router::Registry>>;

pub struct RunContext {
    ctx: Context,
    file_path: FilePathCell,
    doc_content: DocContentCell,
    canonical_path: CanonicalPathSlot,
    project_root: ProjectRootSlot,
    config_path: ConfigPathSlot,
    project_config: ProjectConfigSlot,
    snapshot_path: SnapshotPathSlot,
    doc_relative: DocRelativeSlot,
    ssh_context: SshContextSlot,
    frontmatter: FrontmatterSlot,
    components: ComponentsSlot,
    doc_hash: DocHashSlot,
    cycle_state: CycleStateSlot,
    snapshot_content: SnapshotContentSlot,
    head_content: HeadContentSlot,
    snapshot_commit_status: SnapshotCommitStatusSlot,
    harness: HarnessSlot,
    global_config: GlobalConfigSlot,
    session_registry: SessionRegistrySlot,
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
                agent_doc_project_root_io::project_root_containing(&path)
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
            move |ctx: &Context| -> Arc<ResolvedSshContext> {
                let config: Arc<ProjectConfig> = ctx.get(&pc);
                let doc_rel: Option<String> = ctx.get(&dr);
                Arc::new(ResolvedSshContext {
                    config,
                    doc_relative: doc_rel.unwrap_or_default(),
                })
            }
        });

        let doc_content = ctx.cell(String::new());

        let frontmatter = ctx.slot({
            let dc = doc_content;
            let sc = ssh_context;
            move |ctx: &Context| -> Arc<Frontmatter> {
                let content: String = ctx.get_cell(&dc);
                let ssh: Arc<ResolvedSshContext> = ctx.get(&sc);
                let resolver = ssh.as_resolver_context(&ssh.doc_relative);
                let fm = frontmatter::parse_with_ssh_resolver(&content, &resolver)
                    .map(|(fm, _)| fm)
                    .unwrap_or_default();
                Arc::new(fm)
            }
        });

        let components = ctx.slot({
            let dc = doc_content;
            move |ctx: &Context| -> Arc<Vec<Component>> {
                let content: String = ctx.get_cell(&dc);
                Arc::new(element::parse(&content).unwrap_or_default())
            }
        });

        let doc_hash = ctx.slot({
            let cp = canonical_path;
            move |ctx: &Context| -> String {
                let canonical: PathBuf = ctx.get(&cp);
                agent_doc_fs::document_state_hash(&canonical).unwrap_or_else(|_| {
                    agent_doc_fs::document_state_hash_from_str(canonical.to_string_lossy().as_ref())
                })
            }
        });

        // Phase 7 (#lr-cycle-7): load the per-document cycle state once. A real
        // load error is surfaced to stderr (never swallowed); a missing sidecar
        // is the normal `None` case.
        let cycle_state = ctx.slot({
            let fp = file_path_cell;
            move |ctx: &Context| -> Option<Arc<CycleState>> {
                let path: PathBuf = ctx.get_cell(&fp);
                match agent_doc_cycle_state_io::load(&path) {
                    Ok(state) => state.map(Arc::new),
                    Err(e) => {
                        eprintln!(
                            "[graph] cycle_state load failed for {}: {}",
                            path.display(),
                            e
                        );
                        None
                    }
                }
            }
        });

        // Phase 7 (#lr-cycle-7): load the snapshot content (with flock) once.
        let snapshot_content = ctx.slot({
            let fp = file_path_cell;
            move |ctx: &Context| -> Option<Arc<String>> {
                let path: PathBuf = ctx.get_cell(&fp);
                match agent_doc_snapshot_io::load(&path) {
                    Ok(content) => content.map(Arc::new),
                    Err(e) => {
                        eprintln!("[graph] snapshot load failed for {}: {}", path.display(), e);
                        None
                    }
                }
            }
        });

        // Phase 8 (#lr-head-8): load HEAD content once per CLI context instead
        // of spawning `git show HEAD:<doc>` repeatedly across guards.
        let head_content = ctx.slot({
            let cp = canonical_path;
            let pr = project_root;
            move |ctx: &Context| -> Option<Arc<String>> {
                let canonical: PathBuf = ctx.get(&cp);
                // Register the project-root dependency even though
                // `git::show_head` performs the final submodule narrowing.
                let _root: Option<PathBuf> = ctx.get(&pr);
                match agent_doc_git_io::revision::show_head(&canonical) {
                    Ok(content) => content.map(Arc::new),
                    Err(e) => {
                        eprintln!(
                            "[graph] git show HEAD failed for {}: {}",
                            canonical.display(),
                            e
                        );
                        None
                    }
                }
            }
        });

        // Phase 8 (#lr-head-8): cache the snapshot-vs-HEAD status. Keep the
        // full enum rather than a bool so existing diagnostics retain their
        // specific NoSnapshot/NoHead/NotInGitRepo/mismatch variants.
        let snapshot_commit_status = ctx.slot({
            let fp = file_path_cell;
            let sc = snapshot_content;
            let hc = head_content;
            move |ctx: &Context| -> agent_doc_snapshot_io::SnapshotCommitStatus {
                let path: PathBuf = ctx.get_cell(&fp);
                if !agent_doc_git_io::status::is_in_git_repo(&path) {
                    return agent_doc_snapshot_io::SnapshotCommitStatus::NotInGitRepo;
                }
                let snapshot: Option<Arc<String>> = ctx.get(&sc);
                let head: Option<Arc<String>> = ctx.get(&hc);
                agent_doc_snapshot_io::snapshot_commit_status_from_contents(
                    snapshot.as_deref().map(String::as_str),
                    head.as_deref().map(String::as_str),
                )
            }
        });

        let harness = ctx.slot(|_ctx: &Context| agent_doc_model_tier::detect_harness());

        let global_config = ctx.slot(|_ctx: &Context| -> Arc<agent_doc_config::Config> {
            Arc::new(match agent_doc_config::load() {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("[graph] global config load failed: {e}");
                    agent_doc_config::Config::default()
                }
            })
        });

        let session_registry = ctx.slot({
            let pr = project_root;
            move |ctx: &Context| -> Arc<tmux_router::Registry> {
                let root: Option<PathBuf> = ctx.get(&pr);
                let loaded = match root {
                    Some(ref root) => agent_doc_session_registry_io::load_in(root).map_err(|e| {
                        anyhow::anyhow!(
                            "failed to load session registry from {}: {e}",
                            agent_doc_session_registry_io::registry_path_in(root).display()
                        )
                    }),
                    None => agent_doc_session_registry_io::load()
                        .map_err(|e| anyhow::anyhow!("failed to load session registry: {e}")),
                };

                Arc::new(match loaded {
                    Ok(registry) => registry,
                    Err(e) => {
                        eprintln!("[graph] {e}");
                        tmux_router::Registry::new()
                    }
                })
            }
        });

        Self {
            ctx,
            file_path: file_path_cell,
            doc_content,
            canonical_path,
            project_root,
            config_path,
            project_config,
            snapshot_path,
            doc_relative,
            ssh_context,
            frontmatter,
            components,
            doc_hash,
            cycle_state,
            snapshot_content,
            head_content,
            snapshot_commit_status,
            harness,
            global_config,
            session_registry,
        }
    }

    pub fn from_project_root(project_root: PathBuf) -> Self {
        Self::new(project_root.join(".agent-doc"))
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

    pub fn ssh_context(&self) -> Arc<ResolvedSshContext> {
        self.ctx.get(&self.ssh_context)
    }

    pub fn doc_content(&self) -> String {
        self.ctx.get_cell(&self.doc_content)
    }

    pub fn set_doc_content(&self, content: String) {
        self.ctx.set_cell(&self.doc_content, content);
    }

    pub fn frontmatter(&self) -> Arc<Frontmatter> {
        self.ctx.get(&self.frontmatter)
    }

    pub fn components(&self) -> Arc<Vec<Component>> {
        self.ctx.get(&self.components)
    }

    pub fn doc_hash(&self) -> String {
        self.ctx.get(&self.doc_hash)
    }

    /// Phase 7 (#lr-cycle-7): cached cycle state for this document (loaded once).
    pub fn cycle_state(&self) -> Option<Arc<CycleState>> {
        self.ctx.get(&self.cycle_state)
    }

    /// Phase 7 (#lr-cycle-7): cached snapshot content for this document.
    pub fn snapshot_content(&self) -> Option<Arc<String>> {
        self.ctx.get(&self.snapshot_content)
    }

    /// Phase 8 (#lr-head-8): cached HEAD content for this document.
    pub fn head_content(&self) -> Option<Arc<String>> {
        self.ctx.get(&self.head_content)
    }

    /// Phase 8 (#lr-head-8): cached snapshot-vs-HEAD comparison.
    pub fn snapshot_commit_status(&self) -> agent_doc_snapshot_io::SnapshotCommitStatus {
        self.ctx.get(&self.snapshot_commit_status)
    }

    /// Phase 9 (#lr-wire-9): cached harness detection for this CLI run.
    pub fn harness(&self) -> String {
        self.ctx.get(&self.harness)
    }

    /// Phase 10 (#lr-actor-10): cached global user configuration.
    pub fn global_config(&self) -> Arc<agent_doc_config::Config> {
        self.ctx.get(&self.global_config)
    }

    /// Phase 10 (#lr-actor-10): cached session registry for this document's
    /// project root. This is for read-only point-in-time queries; mutation paths
    /// must continue to load while holding `RegistryLock`.
    pub fn session_registry(&self) -> Arc<tmux_router::Registry> {
        self.ctx.get(&self.session_registry)
    }

    /// Invalidate the cached cycle state after a save/mutation so the next read
    /// reloads it (Phase 7).
    pub fn invalidate_cycle_state(&self) {
        self.cycle_state.clear(&self.ctx);
    }

    /// Invalidate the cached snapshot content after a save/delete (Phase 7).
    pub fn invalidate_snapshot_content(&self) {
        self.snapshot_content.clear(&self.ctx);
    }

    /// Invalidate the cached HEAD content after `git::commit`.
    pub fn invalidate_head_content(&self) {
        self.head_content.clear(&self.ctx);
    }

    /// Invalidate cached global config after a config-file change.
    pub fn invalidate_global_config(&self) {
        self.global_config.clear(&self.ctx);
    }

    /// Invalidate cached session registry after registry mutation or a watcher
    /// event. Registry mutation paths generally create a fresh `RunContext`;
    /// long-lived `ActorContext`s call this before the next read.
    pub fn invalidate_session_registry(&self) {
        self.session_registry.clear(&self.ctx);
    }

    pub fn is_cycle_state_cached(&self) -> bool {
        self.ctx.is_set(&self.cycle_state)
    }

    pub fn is_snapshot_content_cached(&self) -> bool {
        self.ctx.is_set(&self.snapshot_content)
    }

    pub fn is_head_content_cached(&self) -> bool {
        self.ctx.is_set(&self.head_content)
    }

    pub fn is_snapshot_commit_status_cached(&self) -> bool {
        self.ctx.is_set(&self.snapshot_commit_status)
    }

    pub fn is_harness_cached(&self) -> bool {
        self.ctx.is_set(&self.harness)
    }

    pub fn is_global_config_cached(&self) -> bool {
        self.ctx.is_set(&self.global_config)
    }

    pub fn is_session_registry_cached(&self) -> bool {
        self.ctx.is_set(&self.session_registry)
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

    pub fn is_frontmatter_cached(&self) -> bool {
        self.ctx.is_set(&self.frontmatter)
    }

    pub fn is_components_cached(&self) -> bool {
        self.ctx.is_set(&self.components)
    }

    pub fn is_doc_hash_cached(&self) -> bool {
        self.ctx.is_set(&self.doc_hash)
    }

    pub fn invalidate_doc_content(&self) {
        self.doc_content.clear_dependents(&self.ctx);
    }

    pub fn snapshot_path_for(&self) -> Option<PathBuf> {
        self.project_root()?;
        agent_doc_fs::snapshot_path_for(&self.canonical_path()).ok()
    }

    pub fn lock_path_for(&self) -> Option<PathBuf> {
        self.project_root()?;
        agent_doc_fs::state_lock_path_for(&self.canonical_path()).ok()
    }

    pub fn baseline_path_for(&self) -> Option<PathBuf> {
        self.project_root()?;
        agent_doc_fs::baseline_path_for(&self.canonical_path()).ok()
    }

    pub fn pending_path_for(&self) -> Option<PathBuf> {
        self.project_root()?;
        agent_doc_fs::pending_response_path_for(&self.canonical_path()).ok()
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

    pub fn for_project_root(project_root: PathBuf) -> Self {
        Self {
            inner: RunContext::from_project_root(project_root),
        }
    }

    pub fn on_file_change(&self, new_path: PathBuf) {
        self.inner.set_file_path(new_path);
    }

    pub fn on_config_change(&self) {
        self.inner.invalidate_project_root();
        self.inner.invalidate_global_config();
    }

    pub fn on_session_registry_change(&self) {
        self.inner.invalidate_session_registry();
    }

    pub fn invalidate_all(&self) {
        self.inner.invalidate_project_root();
        self.inner.invalidate_global_config();
        self.inner.invalidate_session_registry();
    }

    pub fn context(&self) -> &RunContext {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;
    use std::sync::{Mutex, MutexGuard};
    use tempfile::TempDir;

    static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn setup_project(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/snapshots")).unwrap();
        dir.join(".agent-doc").join("config.toml")
    }

    fn init_git_repo(root: &Path) {
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();
    }

    fn commit_path(root: &Path, rel: &str, message: &str) {
        Command::new("git")
            .current_dir(root)
            .args(["add", rel])
            .output()
            .unwrap();
        let output = Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", message, "--no-verify"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct EnvVarRestore {
        key: &'static str,
        old: Option<std::ffi::OsString>,
    }

    impl EnvVarRestore {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let old = std::env::var_os(key);
            unsafe { std::env::set_var(key, value) };
            Self { key, old }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            unsafe {
                match &self.old {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn registry_entry(pane: &str, session_id: &str, file: &Path) -> tmux_router::RegistryEntry {
        tmux_router::RegistryEntry {
            pane: pane.to_string(),
            pid: 12345,
            cwd: file
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_string_lossy()
                .to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: session_id.to_string(),
            file: file.to_string_lossy().to_string(),
            window: String::new(),
            supervisor_instance_id: String::new(),
        }
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
        let val = ResolvedSshContext {
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

    #[test]
    fn set_doc_content_enables_frontmatter_and_components() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(
            &doc,
            "---\nagent: claude\n---\n\n<!-- agent:exchange -->\nhello\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let rc = RunContext::new(doc.clone());
        rc.set_doc_content(std::fs::read_to_string(&doc).unwrap());

        let fm = rc.frontmatter();
        assert_eq!(fm.agent.as_deref(), Some("claude"));

        let comps = rc.components();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "exchange");
    }

    #[test]
    fn frontmatter_returns_default_when_no_frontmatter() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "no frontmatter here\n").unwrap();

        let rc = RunContext::new(doc);
        rc.set_doc_content("no frontmatter here\n".to_string());

        let fm = rc.frontmatter();
        assert!(fm.agent.is_none());
        assert!(fm.session.is_none());
    }

    #[test]
    fn components_returns_empty_for_no_markers() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "plain text\n").unwrap();

        let rc = RunContext::new(doc);
        rc.set_doc_content("plain text\n".to_string());

        let comps = rc.components();
        assert!(comps.is_empty());
    }

    #[test]
    fn doc_hash_is_deterministic() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        let hash1 = rc.doc_hash();
        let hash2 = rc.doc_hash();
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn doc_hash_matches_snapshot_doc_hash() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc.clone());

        let graph_hash = rc.doc_hash();
        let canonical = doc.canonicalize().unwrap();
        let snap_hash = agent_doc_fs::document_state_hash(&canonical).unwrap();
        assert_eq!(graph_hash, snap_hash);
    }

    #[test]
    fn snapshot_path_for_matches_agent_doc_fs() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc.clone());

        assert_eq!(
            rc.snapshot_path_for(),
            agent_doc_fs::snapshot_path_for(&doc).ok()
        );
    }

    #[test]
    fn lock_path_for_matches_agent_doc_fs() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc.clone());

        assert_eq!(
            rc.lock_path_for(),
            agent_doc_fs::state_lock_path_for(&doc).ok()
        );
    }

    #[test]
    fn baseline_path_for_matches_agent_doc_fs() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc.clone());

        assert_eq!(
            rc.baseline_path_for(),
            agent_doc_fs::baseline_path_for(&doc).ok()
        );
    }

    #[test]
    fn pending_path_for_matches_agent_doc_fs() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc.clone());

        assert_eq!(
            rc.pending_path_for(),
            agent_doc_fs::pending_response_path_for(&doc).ok()
        );
    }

    #[test]
    fn invalidate_doc_content_clears_frontmatter_and_components() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(
            &doc,
            "---\nagent: claude\n---\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let rc = RunContext::new(doc.clone());
        rc.set_doc_content(std::fs::read_to_string(&doc).unwrap());

        let _fm = rc.frontmatter();
        let _comps = rc.components();
        assert!(rc.is_frontmatter_cached());
        assert!(rc.is_components_cached());

        rc.invalidate_doc_content();

        assert!(!rc.is_frontmatter_cached());
        assert!(!rc.is_components_cached());
    }

    #[test]
    fn caching_status_flags_new_slots() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = RunContext::new(doc);

        assert!(!rc.is_doc_hash_cached());
        let _hash = rc.doc_hash();
        assert!(rc.is_doc_hash_cached());

        assert!(!rc.is_frontmatter_cached());
        assert!(!rc.is_components_cached());
    }

    // ---- Phase 7 (#lr-cycle-7) ----

    #[test]
    fn phase7_snapshot_content_slot_loads_and_caches() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "hello").unwrap();
        agent_doc_snapshot_io::save(&doc, "snapshot body", agent_doc_ops_log_io::log_op).unwrap();

        let rc = RunContext::new(doc);
        assert!(!rc.is_snapshot_content_cached());
        let content = rc.snapshot_content().expect("snapshot present");
        assert_eq!(content.as_str(), "snapshot body");
        assert!(
            rc.is_snapshot_content_cached(),
            "snapshot loaded once and cached for the context lifetime"
        );
    }

    #[test]
    fn phase7_snapshot_content_none_when_absent() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "hello").unwrap();

        let rc = RunContext::new(doc);
        assert!(rc.snapshot_content().is_none());
        assert!(rc.is_snapshot_content_cached(), "the None result is cached");
    }

    #[test]
    fn phase7_cycle_state_slot_loads_and_invalidates() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "hello").unwrap();

        let rc = RunContext::new(doc.clone());
        // No sidecar yet → None, and the None is cached.
        assert!(rc.cycle_state().is_none());
        assert!(rc.is_cycle_state_cached());

        // Create a cycle-state sidecar, then prove invalidation reloads it.
        agent_doc_cycle_state_io::start_preflight(&doc, Some("hello"), Some("hello")).unwrap();
        assert!(
            rc.cycle_state().is_none(),
            "still cached as None until invalidated"
        );
        rc.invalidate_cycle_state();
        let state = rc
            .cycle_state()
            .expect("cycle state now present after reload");
        assert!(!state.cycle_id.is_empty());
        assert!(rc.is_cycle_state_cached());
    }

    // ---- Phase 8 (#lr-head-8) ----

    #[test]
    fn phase8_head_content_slot_loads_caches_and_invalidates() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        init_git_repo(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "first\n").unwrap();
        commit_path(dir.path(), "file.md", "first");

        let rc = RunContext::new(doc.clone());
        assert!(!rc.is_head_content_cached());
        let first = rc.head_content().expect("tracked file has HEAD content");
        assert_eq!(first.as_str(), "first\n");
        assert!(rc.is_head_content_cached());

        std::fs::write(&doc, "second\n").unwrap();
        commit_path(dir.path(), "file.md", "second");
        assert_eq!(
            rc.head_content()
                .expect("cached HEAD remains present")
                .as_str(),
            "first\n",
            "HEAD content stays cached until explicitly invalidated"
        );

        rc.invalidate_head_content();
        assert!(!rc.is_head_content_cached());
        assert_eq!(
            rc.head_content()
                .expect("reloaded HEAD remains present")
                .as_str(),
            "second\n"
        );
    }

    #[test]
    fn phase8_snapshot_commit_status_uses_cached_snapshot_and_head() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        init_git_repo(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "committed\n").unwrap();
        agent_doc_snapshot_io::save(&doc, "committed\n", agent_doc_ops_log_io::log_op).unwrap();
        commit_path(dir.path(), "file.md", "add doc");

        let rc = RunContext::new(doc.clone());
        assert!(!rc.is_snapshot_commit_status_cached());
        assert_eq!(
            rc.snapshot_commit_status(),
            agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        );
        assert!(rc.is_snapshot_commit_status_cached());

        agent_doc_snapshot_io::save(&doc, "snapshot drift\n", agent_doc_ops_log_io::log_op)
            .unwrap();
        assert_eq!(
            rc.snapshot_commit_status(),
            agent_doc_snapshot_io::SnapshotCommitStatus::Committed,
            "status stays cached until the snapshot slot is invalidated"
        );

        rc.invalidate_snapshot_content();
        match rc.snapshot_commit_status() {
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead {
                snapshot_len,
                head_len,
            } => {
                assert_eq!(snapshot_len, "snapshot drift".len());
                assert_eq!(head_len, "committed".len());
            }
            other => panic!("expected snapshot/head drift after invalidation, got {other:?}"),
        }
    }

    // ---- Phase 9 (#lr-wire-9) ----

    #[test]
    fn phase9_harness_slot_caches_detected_harness() {
        let _env_guard = env_lock();
        for key in [
            "CLAUDE_CODE_SESSION",
            "CLAUDE_CODE",
            "CLAUDECODE",
            "CODEX_SESSION",
            "CODEX_THREAD_ID",
            "CODEX_CLI",
            "CODEX",
            "OPENCODE_CLIENT",
            "OPENCODE",
        ] {
            unsafe { std::env::remove_var(key) };
        }
        unsafe { std::env::set_var("CODEX_THREAD_ID", "thread-1") };

        let rc = RunContext::new(PathBuf::from("doc.md"));
        assert!(!rc.is_harness_cached());
        assert_eq!(rc.harness(), "codex");
        assert!(rc.is_harness_cached());

        unsafe {
            std::env::remove_var("CODEX_THREAD_ID");
            std::env::set_var("OPENCODE", "1");
        }
        assert_eq!(
            rc.harness(),
            "codex",
            "cached harness should not change during one RunContext"
        );

        unsafe { std::env::remove_var("OPENCODE") };
    }

    // ---- Phase 10 (#lr-actor-10) ----

    #[test]
    fn phase10_global_config_slot_loads_caches_and_invalidates() {
        let _env_guard = env_lock();
        let config_root = TempDir::new().unwrap();
        let config_dir = config_root.path().join("agent-doc");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("config.toml"),
            "default_agent = \"claude\"\nagent_args = \"--first\"\n",
        )
        .unwrap();
        let _xdg = EnvVarRestore::set("XDG_CONFIG_HOME", config_root.path());

        let rc = RunContext::new(PathBuf::from("doc.md"));
        assert!(!rc.is_global_config_cached());
        let first = rc.global_config();
        assert_eq!(first.default_agent.as_deref(), Some("claude"));
        assert_eq!(first.agent_args.as_deref(), Some("--first"));
        assert!(rc.is_global_config_cached());

        std::fs::write(
            config_dir.join("config.toml"),
            "default_agent = \"codex\"\nagent_args = \"--second\"\n",
        )
        .unwrap();
        assert_eq!(
            rc.global_config().default_agent.as_deref(),
            Some("claude"),
            "global config remains cached until invalidated"
        );

        rc.invalidate_global_config();
        let second = rc.global_config();
        assert_eq!(second.default_agent.as_deref(), Some("codex"));
        assert_eq!(second.agent_args.as_deref(), Some("--second"));
    }

    #[test]
    fn phase10_session_registry_slot_loads_caches_and_invalidates() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let mut first_registry = tmux_router::Registry::new();
        first_registry.insert("first".to_string(), registry_entry("%1", "first", &doc));
        agent_doc_session_registry_io::save_in(dir.path(), &first_registry).unwrap();

        let rc = RunContext::new(doc.clone());
        assert!(!rc.is_session_registry_cached());
        let first = rc.session_registry();
        assert!(first.values().any(|entry| entry.pane == "%1"));
        assert!(rc.is_session_registry_cached());

        let mut second_registry = tmux_router::Registry::new();
        second_registry.insert("second".to_string(), registry_entry("%2", "second", &doc));
        agent_doc_session_registry_io::save_in(dir.path(), &second_registry).unwrap();

        assert!(
            rc.session_registry()
                .values()
                .any(|entry| entry.pane == "%1"),
            "session registry remains cached until invalidated"
        );
        rc.invalidate_session_registry();
        let second = rc.session_registry();
        assert!(second.values().any(|entry| entry.pane == "%2"));
        assert!(!second.values().any(|entry| entry.pane == "%1"));
    }

    #[test]
    fn phase10_actor_context_invalidates_session_registry() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let ac = ActorContext::new(doc.clone());
        assert!(ac.context().session_registry().is_empty());
        assert!(ac.context().is_session_registry_cached());

        let mut registry = tmux_router::Registry::new();
        registry.insert("live".to_string(), registry_entry("%9", "live", &doc));
        agent_doc_session_registry_io::save_in(dir.path(), &registry).unwrap();

        assert!(
            ac.context().session_registry().is_empty(),
            "actor registry stays cached until an actor event invalidates it"
        );
        ac.on_session_registry_change();
        assert!(
            ac.context()
                .session_registry()
                .values()
                .any(|entry| entry.pane == "%9")
        );
    }
}
