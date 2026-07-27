//! Lazily-rs dependency graph for agent-doc filesystem computations.
//!
//! Provides [`CycleContext`] — a short-lived context for a single CLI invocation
//! (`preflight → plan → write → commit`) — that caches filesystem-derived
//! lookups (`project_root`, `project_config`, etc.) behind a reactive
//! dependency graph. Within a CLI run, each slot computes at most once.
//!
//! For long-lived contexts (watch daemon, supervisor), use [`ActorContext`],
//! which uses the same typed document schema with an actor-specific lazily
//! context family and explicit invalidation on file/config change events.
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
use lazily::{TypedCellHandle, TypedContext, TypedFactoryContext, TypedSlotHandle};

use agent_doc_cycle_state_io::CycleState;
use agent_doc_document_realtime::{CurrentDocument, reconcile_current_doc};
use agent_doc_project_config_io as project_config_io;

lazily::define_schema!(pub CycleContextSchema);
lazily::define_schema!(pub ActorContextSchema);

pub type CycleContext = TypedContext<CycleContextSchema>;
pub type ActorContext = TypedContext<ActorContextSchema>;

pub type FilePathCell<Schema> = TypedCellHandle<Schema, PathBuf>;
pub type CurrentDocumentCell<Schema> = TypedCellHandle<Schema, Option<CurrentDocument>>;
pub type CanonicalPathSlot<Schema> = TypedSlotHandle<Schema, PathBuf>;
pub type ProjectRootSlot<Schema> = TypedSlotHandle<Schema, Option<PathBuf>>;
pub type ConfigPathSlot<Schema> = TypedSlotHandle<Schema, Option<PathBuf>>;
pub type ProjectConfigSlot<Schema> = TypedSlotHandle<Schema, Arc<ProjectConfig>>;
pub type SnapshotPathSlot<Schema> = TypedSlotHandle<Schema, Option<PathBuf>>;
pub type DocRelativeSlot<Schema> = TypedSlotHandle<Schema, Option<String>>;
pub type SshContextSlot<Schema> = TypedSlotHandle<Schema, Arc<ResolvedSshContext>>;
pub type FrontmatterSlot<Schema> = TypedSlotHandle<Schema, Arc<Frontmatter>>;
pub type ComponentsSlot<Schema> = TypedSlotHandle<Schema, Arc<Vec<Component>>>;
pub type DocHashSlot<Schema> = TypedSlotHandle<Schema, String>;
/// Phase 7 (#lr-cycle-7): cached per-document cycle state, loaded at most once
/// per context lifetime. `None` when no durable closeout state exists yet.
pub type CycleStateSlot<Schema> = TypedSlotHandle<Schema, Option<Arc<CycleState>>>;
/// Phase 7 (#lr-cycle-7): cached snapshot content, loaded (with flock) at most
/// once per context lifetime. `None` when no snapshot exists yet.
pub type SnapshotContentSlot<Schema> = TypedSlotHandle<Schema, Option<Arc<String>>>;
/// Phase 8 (#lr-head-8): cached `git show HEAD:<doc>` content, spawned at
/// most once per context lifetime. `None` when the document is not tracked or
/// HEAD cannot provide content.
pub type HeadContentSlot<Schema> = TypedSlotHandle<Schema, Option<Arc<String>>>;
/// Phase 8 (#lr-head-8): cached comparison of snapshot content against HEAD.
pub type SnapshotCommitStatusSlot<Schema> =
    TypedSlotHandle<Schema, agent_doc_snapshot_io::SnapshotCommitStatus>;
/// Phase 9 (#lr-wire-9): cached harness detection. Harness env vars are
/// process-static for a CLI run, so compute once per [`CycleContext`].
pub type HarnessSlot<Schema> = TypedSlotHandle<Schema, String>;
/// Phase 10 (#lr-actor-10): cached global user configuration. For CLI runs this
/// is process-static; for long-lived actors it is invalidated by config-change
/// events before the next read.
pub type GlobalConfigSlot<Schema> = TypedSlotHandle<Schema, Arc<agent_doc_config::Config>>;
/// Phase 10 (#lr-actor-10): cached session registry for the current document's
/// project root. Read-modify-write callers still load under `RegistryLock`.
pub type SessionRegistrySlot<Schema> = TypedSlotHandle<Schema, Arc<tmux_router::Registry>>;

struct FilePathKey;
struct CurrentDocumentKey;
struct CanonicalPathKey;
struct ProjectRootKey;
struct ConfigPathKey;
struct ProjectConfigKey;
struct SnapshotPathKey;
struct DocRelativeKey;
struct SshContextKey;
struct FrontmatterKey;
struct ComponentsKey;
struct DocHashKey;
struct CycleStateKey;
struct SnapshotContentKey;
struct HeadContentKey;
struct SnapshotCommitStatusKey;
struct HarnessKey;
struct GlobalConfigKey;
struct SessionRegistryKey;

fn file_path_cell<C>(ctx: &C) -> FilePathCell<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_cell::<FilePathKey, _, _>(|_| PathBuf::new())
}

fn current_document_cell<C>(ctx: &C) -> CurrentDocumentCell<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_cell::<CurrentDocumentKey, _, _>(|_| None::<CurrentDocument>)
}

fn canonical_path_slot<C>(ctx: &C) -> CanonicalPathSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<CanonicalPathKey, _, _>(|ctx| {
        let path: PathBuf = ctx.get(file_path_cell(ctx));
        std::fs::canonicalize(&path).unwrap_or(path)
    })
}

fn project_root_slot<C>(ctx: &C) -> ProjectRootSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<ProjectRootKey, _, _>(|ctx| {
        let path: PathBuf = ctx.get(canonical_path_slot(ctx));
        agent_doc_project_root_io::project_root_containing(&path)
    })
}

fn config_path_slot<C>(ctx: &C) -> ConfigPathSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<ConfigPathKey, _, _>(|ctx| {
        let root: Option<PathBuf> = ctx.get(project_root_slot(ctx));
        root.map(|r| r.join(".agent-doc").join("config.toml"))
    })
}

fn project_config_slot<C>(ctx: &C) -> ProjectConfigSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<ProjectConfigKey, _, _>(|ctx| {
        let path: Option<PathBuf> = ctx.get(config_path_slot(ctx));
        Arc::new(match path {
            Some(ref p) => project_config_io::load_project_from(p),
            None => ProjectConfig::default(),
        })
    })
}

fn snapshot_path_slot<C>(ctx: &C) -> SnapshotPathSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<SnapshotPathKey, _, _>(|ctx| {
        let root: Option<PathBuf> = ctx.get(project_root_slot(ctx));
        root.map(|r| r.join(".agent-doc").join("snapshots"))
    })
}

fn doc_relative_slot<C>(ctx: &C) -> DocRelativeSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<DocRelativeKey, _, _>(|ctx| {
        let canonical: PathBuf = ctx.get(canonical_path_slot(ctx));
        let root: Option<PathBuf> = ctx.get(project_root_slot(ctx));
        root.map(|r| {
            canonical
                .strip_prefix(&r)
                .unwrap_or(canonical.as_path())
                .to_string_lossy()
                .replace('\\', "/")
        })
    })
}

fn ssh_context_slot<C>(ctx: &C) -> SshContextSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<SshContextKey, _, _>(|ctx| {
        let config: Arc<ProjectConfig> = ctx.get(project_config_slot(ctx));
        let doc_rel: Option<String> = ctx.get(doc_relative_slot(ctx));
        Arc::new(ResolvedSshContext {
            config,
            doc_relative: doc_rel.unwrap_or_default(),
        })
    })
}

fn frontmatter_slot<C>(ctx: &C) -> FrontmatterSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<FrontmatterKey, _, _>(|ctx| {
        let content = ctx
            .get(current_document_cell(ctx))
            .map(|doc| doc.into_content())
            .unwrap_or_default();
        let ssh: Arc<ResolvedSshContext> = ctx.get(ssh_context_slot(ctx));
        let resolver = ssh.as_resolver_context(&ssh.doc_relative);
        let fm = frontmatter::parse_with_ssh_resolver(&content, &resolver)
            .map(|(fm, _)| fm)
            .unwrap_or_default();
        Arc::new(fm)
    })
}

/// Report a context read that is slow enough to matter (`#sessioncheckprofile`).
///
/// The slots behind these accessors are memoized, so each cost is paid once per
/// context — but "once" is still the whole cost when a sweep touches a dozen of
/// them for the first time. Attribution has to happen here, because a caller
/// only ever sees a hit or a miss, never which value it paid for.
fn timed_read<T>(label: &'static str, read: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let out = read();
    let elapsed = started.elapsed();
    if elapsed >= std::time::Duration::from_millis(25) {
        eprintln!(
            "[perf] run_context.read label={label} elapsed_ms={}",
            elapsed.as_millis()
        );
    }
    out
}

fn components_slot<C>(ctx: &C) -> ComponentsSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<ComponentsKey, _, _>(|ctx| {
        let content = ctx
            .get(current_document_cell(ctx))
            .map(|doc| doc.into_content())
            .unwrap_or_default();
        Arc::new(element::parse(&content).unwrap_or_default())
    })
}

fn doc_hash_slot<C>(ctx: &C) -> DocHashSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<DocHashKey, _, _>(|ctx| {
        let canonical: PathBuf = ctx.get(canonical_path_slot(ctx));
        agent_doc_fs::document_state_hash(&canonical).unwrap_or_else(|_| {
            agent_doc_fs::document_state_hash_from_str(canonical.to_string_lossy().as_ref())
        })
    })
}

fn cycle_state_slot<C>(ctx: &C) -> CycleStateSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    // Phase 7 (#lr-cycle-7): load the per-document cycle state once. A real
    // load error is surfaced to stderr (never swallowed); a missing sidecar is
    // the normal `None` case.
    ctx.memoized_slot::<CycleStateKey, _, _>(|ctx| {
        let path: PathBuf = ctx.get(file_path_cell(ctx));
        match agent_doc_cycle_state_io::load_with_closeout_projection(&path) {
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
    })
}

fn snapshot_content_slot<C>(ctx: &C) -> SnapshotContentSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    // Phase 7 (#lr-cycle-7): load the snapshot content (with flock) once.
    ctx.memoized_slot::<SnapshotContentKey, _, _>(|ctx| {
        let path: PathBuf = ctx.get(file_path_cell(ctx));
        match agent_doc_snapshot_io::load_document_baseline(&path) {
            Ok(content) => content.map(Arc::new),
            Err(e) => {
                eprintln!("[graph] snapshot load failed for {}: {}", path.display(), e);
                None
            }
        }
    })
}

fn head_content_slot<C>(ctx: &C) -> HeadContentSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    // Phase 8 (#lr-head-8): load HEAD content once per CLI context instead of
    // spawning `git show HEAD:<doc>` repeatedly across guards.
    ctx.memoized_slot::<HeadContentKey, _, _>(|ctx| {
        let canonical: PathBuf = ctx.get(canonical_path_slot(ctx));
        // Register the project-root dependency even though `git::show_head`
        // performs the final submodule narrowing.
        let _root: Option<PathBuf> = ctx.get(project_root_slot(ctx));
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
    })
}

fn snapshot_commit_status_slot<C>(ctx: &C) -> SnapshotCommitStatusSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    // Phase 8 (#lr-head-8): cache the snapshot-vs-HEAD status. Keep the full
    // enum rather than a bool so diagnostics retain their specific variants.
    ctx.memoized_slot::<SnapshotCommitStatusKey, _, _>(|ctx| {
        let path: PathBuf = ctx.get(file_path_cell(ctx));
        if !agent_doc_git_io::status::is_in_git_repo(&path) {
            return agent_doc_snapshot_io::SnapshotCommitStatus::NotInGitRepo;
        }
        let snapshot: Option<Arc<String>> = ctx.get(snapshot_content_slot(ctx));
        let head: Option<Arc<String>> = ctx.get(head_content_slot(ctx));
        agent_doc_snapshot_io::snapshot_commit_status_from_contents(
            snapshot.as_deref().map(String::as_str),
            head.as_deref().map(String::as_str),
        )
    })
}

fn harness_slot<C>(ctx: &C) -> HarnessSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<HarnessKey, _, _>(|_| agent_doc_model_tier::detect_harness())
}

fn global_config_slot<C>(ctx: &C) -> GlobalConfigSlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<GlobalConfigKey, _, _>(|_| {
        Arc::new(match agent_doc_config::load() {
            Ok(config) => config,
            Err(e) => {
                eprintln!("[graph] global config load failed: {e}");
                agent_doc_config::Config::default()
            }
        })
    })
}

fn session_registry_slot<C>(ctx: &C) -> SessionRegistrySlot<C::Schema>
where
    C: TypedFactoryContext + ?Sized,
{
    ctx.memoized_slot::<SessionRegistryKey, _, _>(|ctx| {
        let root: Option<PathBuf> = ctx.get(project_root_slot(ctx));
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
    })
}

pub fn cycle_context(file_path: PathBuf) -> CycleContext {
    // #stategraphjoin-allow: this IS a scope factory — the context is the returned
    // value's own graph, not a private island hidden inside a longer-lived owner.
    let ctx = CycleContext::new();
    ctx.set(file_path_cell(&ctx), file_path);
    ctx
}

pub fn actor_context(file_path: PathBuf) -> ActorContext {
    // #stategraphjoin-allow: this IS a scope factory — the context is the returned
    // value's own graph, not a private island hidden inside a longer-lived owner.
    let ctx = ActorContext::new();
    ctx.set(file_path_cell(&ctx), file_path);
    ctx
}

pub fn cycle_context_from_project_root(project_root: PathBuf) -> CycleContext {
    cycle_context(project_root.join(".agent-doc"))
}

pub fn actor_context_from_project_root(project_root: PathBuf) -> ActorContext {
    actor_context(project_root.join(".agent-doc"))
}

pub fn actor_context_for_project_root(project_root: PathBuf) -> ActorContext {
    actor_context_from_project_root(project_root)
}

pub trait AgentDocContextExt {
    fn file_path(&self) -> PathBuf;
    fn set_file_path(&self, path: PathBuf);
    fn canonical_path(&self) -> PathBuf;
    fn project_root(&self) -> Option<PathBuf>;
    fn config_path(&self) -> Option<PathBuf>;
    fn project_config(&self) -> Arc<ProjectConfig>;
    fn snapshot_path(&self) -> Option<PathBuf>;
    fn doc_relative(&self) -> Option<String>;
    fn ssh_context(&self) -> Arc<ResolvedSshContext>;
    fn doc_content(&self) -> String;
    fn set_doc_content(&self, content: String);
    fn current_document(&self) -> Option<CurrentDocument>;
    fn set_current_document(&self, current: CurrentDocument);
    fn clear_current_document(&self);
    fn frontmatter(&self) -> Arc<Frontmatter>;
    fn components(&self) -> Arc<Vec<Component>>;
    fn doc_hash(&self) -> String;
    fn cycle_state(&self) -> Option<Arc<CycleState>>;
    fn snapshot_content(&self) -> Option<Arc<String>>;
    fn head_content(&self) -> Option<Arc<String>>;
    fn snapshot_commit_status(&self) -> agent_doc_snapshot_io::SnapshotCommitStatus;
    fn harness(&self) -> String;
    fn global_config(&self) -> Arc<agent_doc_config::Config>;
    fn session_registry(&self) -> Arc<tmux_router::Registry>;
    fn invalidate_cycle_state(&self);
    fn invalidate_snapshot_content(&self);
    fn invalidate_head_content(&self);
    fn invalidate_global_config(&self);
    fn invalidate_session_registry(&self);
    fn is_cycle_state_cached(&self) -> bool;
    fn is_snapshot_content_cached(&self) -> bool;
    fn is_head_content_cached(&self) -> bool;
    fn is_snapshot_commit_status_cached(&self) -> bool;
    fn is_harness_cached(&self) -> bool;
    fn is_global_config_cached(&self) -> bool;
    fn is_session_registry_cached(&self) -> bool;
    fn invalidate_project_root(&self);
    fn invalidate_project_config(&self);
    fn is_project_root_cached(&self) -> bool;
    fn is_project_config_cached(&self) -> bool;
    fn is_canonical_path_cached(&self) -> bool;
    fn is_doc_relative_cached(&self) -> bool;
    fn is_ssh_context_cached(&self) -> bool;
    fn is_frontmatter_cached(&self) -> bool;
    fn is_components_cached(&self) -> bool;
    fn is_doc_hash_cached(&self) -> bool;
    fn invalidate_doc_content(&self);
    fn snapshot_path_for(&self) -> Option<PathBuf>;
    fn lock_path_for(&self) -> Option<PathBuf>;
    fn on_file_change(&self, new_path: PathBuf);
    fn on_config_change(&self);
    fn on_session_registry_change(&self);
    fn invalidate_all(&self);
}

impl<Schema: 'static> AgentDocContextExt for TypedContext<Schema> {
    fn file_path(&self) -> PathBuf {
        self.get(file_path_cell(self))
    }

    fn set_file_path(&self, path: PathBuf) {
        self.set(file_path_cell(self), path);
    }

    fn canonical_path(&self) -> PathBuf {
        timed_read("canonical_path", || self.get(canonical_path_slot(self)))
    }

    fn project_root(&self) -> Option<PathBuf> {
        timed_read("project_root", || self.get(project_root_slot(self)))
    }

    fn config_path(&self) -> Option<PathBuf> {
        timed_read("config_path", || self.get(config_path_slot(self)))
    }

    fn project_config(&self) -> Arc<ProjectConfig> {
        timed_read("project_config", || self.get(project_config_slot(self)))
    }

    fn snapshot_path(&self) -> Option<PathBuf> {
        timed_read("snapshot_path", || self.get(snapshot_path_slot(self)))
    }

    fn doc_relative(&self) -> Option<String> {
        timed_read("doc_relative", || self.get(doc_relative_slot(self)))
    }

    fn ssh_context(&self) -> Arc<ResolvedSshContext> {
        timed_read("ssh_context", || self.get(ssh_context_slot(self)))
    }

    fn doc_content(&self) -> String {
        self.current_document()
            .map(CurrentDocument::into_content)
            .unwrap_or_default()
    }

    fn set_doc_content(&self, content: String) {
        let current = CurrentDocument::new(self.file_path(), reconcile_current_doc(&content, None));
        self.set_current_document(current);
    }

    fn current_document(&self) -> Option<CurrentDocument> {
        self.get(current_document_cell(self))
    }

    fn set_current_document(&self, current: CurrentDocument) {
        self.set(current_document_cell(self), Some(current));
    }

    fn clear_current_document(&self) {
        self.set(current_document_cell(self), None);
    }

    fn frontmatter(&self) -> Arc<Frontmatter> {
        timed_read("frontmatter", || self.get(frontmatter_slot(self)))
    }

    fn components(&self) -> Arc<Vec<Component>> {
        timed_read("components", || self.get(components_slot(self)))
    }

    fn doc_hash(&self) -> String {
        timed_read("doc_hash", || self.get(doc_hash_slot(self)))
    }

    fn cycle_state(&self) -> Option<Arc<CycleState>> {
        timed_read("cycle_state", || self.get(cycle_state_slot(self)))
    }

    fn snapshot_content(&self) -> Option<Arc<String>> {
        timed_read("snapshot_content", || self.get(snapshot_content_slot(self)))
    }

    fn head_content(&self) -> Option<Arc<String>> {
        timed_read("head_content", || self.get(head_content_slot(self)))
    }

    fn snapshot_commit_status(&self) -> agent_doc_snapshot_io::SnapshotCommitStatus {
        timed_read("snapshot_commit_status", || {
            self.get(snapshot_commit_status_slot(self))
        })
    }

    fn harness(&self) -> String {
        timed_read("harness", || self.get(harness_slot(self)))
    }

    fn global_config(&self) -> Arc<agent_doc_config::Config> {
        timed_read("global_config", || self.get(global_config_slot(self)))
    }

    fn session_registry(&self) -> Arc<tmux_router::Registry> {
        timed_read("session_registry", || self.get(session_registry_slot(self)))
    }

    fn invalidate_cycle_state(&self) {
        cycle_state_slot(self).clear(self);
    }

    fn invalidate_snapshot_content(&self) {
        snapshot_content_slot(self).clear(self);
        snapshot_commit_status_slot(self).clear(self);
    }

    fn invalidate_head_content(&self) {
        head_content_slot(self).clear(self);
        snapshot_commit_status_slot(self).clear(self);
    }

    fn invalidate_global_config(&self) {
        global_config_slot(self).clear(self);
    }

    fn invalidate_session_registry(&self) {
        session_registry_slot(self).clear(self);
    }

    fn is_cycle_state_cached(&self) -> bool {
        self.is_set(&cycle_state_slot(self))
    }

    fn is_snapshot_content_cached(&self) -> bool {
        self.is_set(&snapshot_content_slot(self))
    }

    fn is_head_content_cached(&self) -> bool {
        self.is_set(&head_content_slot(self))
    }

    fn is_snapshot_commit_status_cached(&self) -> bool {
        self.is_set(&snapshot_commit_status_slot(self))
    }

    fn is_harness_cached(&self) -> bool {
        self.is_set(&harness_slot(self))
    }

    fn is_global_config_cached(&self) -> bool {
        self.is_set(&global_config_slot(self))
    }

    fn is_session_registry_cached(&self) -> bool {
        self.is_set(&session_registry_slot(self))
    }

    fn invalidate_project_root(&self) {
        project_root_slot(self).clear(self);
    }

    fn invalidate_project_config(&self) {
        project_config_slot(self).clear(self);
    }

    fn is_project_root_cached(&self) -> bool {
        self.is_set(&project_root_slot(self))
    }

    fn is_project_config_cached(&self) -> bool {
        self.is_set(&project_config_slot(self))
    }

    fn is_canonical_path_cached(&self) -> bool {
        self.is_set(&canonical_path_slot(self))
    }

    fn is_doc_relative_cached(&self) -> bool {
        self.is_set(&doc_relative_slot(self))
    }

    fn is_ssh_context_cached(&self) -> bool {
        self.is_set(&ssh_context_slot(self))
    }

    fn is_frontmatter_cached(&self) -> bool {
        self.is_set(&frontmatter_slot(self))
    }

    fn is_components_cached(&self) -> bool {
        self.is_set(&components_slot(self))
    }

    fn is_doc_hash_cached(&self) -> bool {
        self.is_set(&doc_hash_slot(self))
    }

    fn invalidate_doc_content(&self) {
        current_document_cell(self).clear_dependents(self);
    }

    fn snapshot_path_for(&self) -> Option<PathBuf> {
        self.project_root()?;
        agent_doc_fs::snapshot_path_for(&self.canonical_path()).ok()
    }

    fn lock_path_for(&self) -> Option<PathBuf> {
        self.project_root()?;
        agent_doc_fs::state_lock_path_for(&self.canonical_path()).ok()
    }

    fn on_file_change(&self, new_path: PathBuf) {
        self.set_file_path(new_path);
    }

    fn on_config_change(&self) {
        self.invalidate_project_root();
        self.invalidate_global_config();
    }

    fn on_session_registry_change(&self) {
        self.invalidate_session_registry();
    }

    fn invalidate_all(&self) {
        self.invalidate_project_root();
        self.invalidate_global_config();
        self.invalidate_session_registry();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::{Mutex, MutexGuard};
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    static TEST_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        TEST_ENV_LOCK.lock()
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

        let rc = cycle_context(doc.clone());

        let root = rc.project_root().unwrap();
        assert_eq!(root, dir.path());

        let cp = rc.config_path().unwrap();
        assert_eq!(cp, dir.path().join(".agent-doc").join("config.toml"));
    }

    #[test]
    fn run_context_no_project_root_finds_ancestor() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        std::fs::create_dir_all(dir.path().join("nested")).unwrap();
        let doc = dir.path().join("nested/file.md");
        std::fs::write(&doc, "").unwrap();

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc);

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

        let ac = actor_context(doc);

        let _root = ac.project_root();
        let _cfg = ac.project_config();
        assert!(ac.is_project_root_cached());
        assert!(ac.is_project_config_cached());

        ac.on_config_change();

        assert!(!ac.is_project_root_cached());
        assert!(!ac.is_project_config_cached());
    }

    #[test]
    fn actor_context_on_file_change_clears_dependents() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "").unwrap();

        let ac = actor_context(doc);
        let _root = ac.project_root();
        assert!(ac.is_project_root_cached());

        ac.on_file_change(PathBuf::from("/other/path.md"));

        assert!(!ac.is_project_root_cached());
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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc.clone());
        rc.set_doc_content(std::fs::read_to_string(&doc).unwrap());

        let fm = rc.frontmatter();
        assert_eq!(fm.agent.as_deref(), Some("claude"));

        let comps = rc.components();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "exchange");
    }

    #[test]
    fn run_context_current_document_is_authoritative_document_cell() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "disk replica\n").unwrap();

        let rc = cycle_context(doc.clone());
        let current = CurrentDocument::new(
            doc.clone(),
            reconcile_current_doc(
                "disk replica\n",
                Some(&agent_doc_document_realtime::BufferState::new(
                    "editor buffer\n",
                    true,
                    7,
                )),
            ),
        );
        rc.set_current_document(current);

        let stored = rc.current_document().expect("current document seeded");
        assert_eq!(stored.key().as_path(), doc.as_path());
        assert_eq!(
            stored.authority(),
            agent_doc_document_realtime::DocAuthority::EditorBuffer
        );
        assert_eq!(rc.doc_content(), "editor buffer\n");
    }

    #[test]
    fn frontmatter_returns_default_when_no_frontmatter() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "no frontmatter here\n").unwrap();

        let rc = cycle_context(doc);
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

        let rc = cycle_context(doc);
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

        let rc = cycle_context(doc);

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

        let rc = cycle_context(doc.clone());

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

        let rc = cycle_context(doc.clone());

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

        let rc = cycle_context(doc.clone());

        assert_eq!(
            rc.lock_path_for(),
            agent_doc_fs::state_lock_path_for(&doc).ok()
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

        let rc = cycle_context(doc.clone());
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

        let rc = cycle_context(doc);

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "snapshot body",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let rc = cycle_context(doc);
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

        let rc = cycle_context(doc);
        assert!(rc.snapshot_content().is_none());
        assert!(rc.is_snapshot_content_cached(), "the None result is cached");
    }

    #[test]
    fn phase7_cycle_state_slot_loads_and_invalidates() {
        let dir = TempDir::new().unwrap();
        setup_project(dir.path());
        let doc = dir.path().join("file.md");
        std::fs::write(&doc, "hello").unwrap();

        let rc = cycle_context(doc.clone());
        // No ledger row yet → None, and the None is cached.
        assert!(rc.cycle_state().is_none());
        assert!(rc.is_cycle_state_cached());

        // Create a ledger cycle, then prove invalidation reloads it.
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

        agent_doc_cycle_state_io::mark_committed(&doc, "test", Some("hello"), Some("hello"))
            .unwrap();
        assert!(
            !agent_doc_cycle_state_io::load(&doc)
                .unwrap()
                .unwrap()
                .is_open()
        );
        rc.invalidate_cycle_state();
        let projected = rc
            .cycle_state()
            .expect("projection-aware cycle state should still load");
        assert!(
            !projected.is_open(),
            "cycle-state slot should honor the terminal closeout projection"
        );
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

        let rc = cycle_context(doc.clone());
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "committed\n",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        commit_path(dir.path(), "file.md", "add doc");

        let rc = cycle_context(doc.clone());
        assert!(!rc.is_snapshot_commit_status_cached());
        assert_eq!(
            rc.snapshot_commit_status(),
            agent_doc_snapshot_io::SnapshotCommitStatus::Committed
        );
        assert!(rc.is_snapshot_commit_status_cached());

        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            "snapshot drift\n",
            agent_doc_ops_log_io::log_op,
        )
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

        let rc = cycle_context(PathBuf::from("doc.md"));
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
            "cached harness should not change during one CycleContext"
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

        let rc = cycle_context(PathBuf::from("doc.md"));
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

        let rc = cycle_context(doc.clone());
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

        let ac = actor_context(doc.clone());
        assert!(ac.session_registry().is_empty());
        assert!(ac.is_session_registry_cached());

        let mut registry = tmux_router::Registry::new();
        registry.insert("live".to_string(), registry_entry("%9", "live", &doc));
        agent_doc_session_registry_io::save_in(dir.path(), &registry).unwrap();

        assert!(
            ac.session_registry().is_empty(),
            "actor registry stays cached until an actor event invalidates it"
        );
        ac.on_session_registry_change();
        assert!(
            ac.session_registry()
                .values()
                .any(|entry| entry.pane == "%9")
        );
    }
}
