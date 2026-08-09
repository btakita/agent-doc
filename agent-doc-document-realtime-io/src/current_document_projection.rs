//! Pass-scoped reactive projections of the authoritative current document.
//!
//! A closeout pass has many readers (document guards, pipeline cleanup, queue
//! continuation). Resolving live editor authority for each reader is expensive,
//! while memoizing only by path can hide operator edits made during the pass.
//! This graph keys every derived value by the live Yrs state vector, or by a
//! disk-content hash when no editor is attached. The CRDT remains the sole
//! durable text authority; this module owns only short-lived derived values.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_doc_crdt_relay_io::CurrentRevision;
use agent_doc_document_realtime::CurrentDocument;
use agent_doc_element::element::Component;
use agent_doc_frontmatter::frontmatter::{AgentDocPipeline, Frontmatter};
use agent_doc_state_scope::TurnScope;
use anyhow::{Result, anyhow};
use lazily::{ThreadSafeComputedMap, ThreadSafeSourceMap};

thread_local! {
    static PASS: RefCell<Option<PassGraph>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectionRevision {
    Crdt(CurrentRevision),
    DiskHash(String),
}

#[derive(Debug, Clone)]
pub struct QueueDocumentProjection {
    components: Arc<Vec<Component>>,
}

impl QueueDocumentProjection {
    pub fn components(&self) -> &[Component] {
        self.components.as_slice()
    }

    pub fn component(&self, name: &str) -> Option<&Component> {
        self.components
            .iter()
            .find(|component| component.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct CurrentDocumentProjection {
    document: CurrentDocument,
    frontmatter: std::result::Result<Arc<Frontmatter>, Arc<str>>,
    pipeline: std::result::Result<Arc<AgentDocPipeline>, Arc<str>>,
    queue: std::result::Result<Arc<QueueDocumentProjection>, Arc<str>>,
}

impl PartialEq for CurrentDocumentProjection {
    fn eq(&self, other: &Self) -> bool {
        // Every other field is a pure parse of these exact authoritative bytes.
        self.document == other.document
    }
}

impl Eq for CurrentDocumentProjection {}

impl CurrentDocumentProjection {
    fn new(document: CurrentDocument) -> Self {
        let content = document.content();
        let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
            .map(|(frontmatter, _)| Arc::new(frontmatter))
            .map_err(|error| Arc::<str>::from(format!("{error:#}")));
        let pipeline = frontmatter
            .as_ref()
            .map(|frontmatter| Arc::new(frontmatter.pipeline.clone()))
            .map_err(Arc::clone);
        let queue = agent_doc_element::element::parse(content)
            .map(|components| {
                let components = components
                    .into_iter()
                    .filter(|component| {
                        matches!(
                            component.name.as_str(),
                            "queue" | "backlog" | "pending" | "review" | "icebox"
                        )
                    })
                    .collect();
                Arc::new(QueueDocumentProjection {
                    components: Arc::new(components),
                })
            })
            .map_err(|error| Arc::<str>::from(format!("{error:#}")));
        Self {
            document,
            frontmatter,
            pipeline,
            queue,
        }
    }

    pub fn document(&self) -> &CurrentDocument {
        &self.document
    }

    pub fn frontmatter(&self) -> Result<Arc<Frontmatter>> {
        self.frontmatter
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| anyhow!("{error}"))
    }

    pub fn pipeline(&self) -> Result<Arc<AgentDocPipeline>> {
        self.pipeline
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| anyhow!("{error}"))
    }

    pub fn queue(&self) -> Result<Arc<QueueDocumentProjection>> {
        self.queue
            .as_ref()
            .map(Arc::clone)
            .map_err(|error| anyhow!("{error}"))
    }
}

struct PassGraph {
    scope: TurnScope,
    revision: ThreadSafeSourceMap<PathBuf, ProjectionRevision>,
    manual_epoch: ThreadSafeSourceMap<PathBuf, u64>,
    projection: ThreadSafeComputedMap<PathBuf, Option<Arc<CurrentDocumentProjection>>>,
}

impl PassGraph {
    fn new() -> Self {
        let scope = TurnScope::new();
        Self {
            revision: ThreadSafeSourceMap::new(scope.ctx()),
            manual_epoch: ThreadSafeSourceMap::new(scope.ctx()),
            projection: ThreadSafeComputedMap::new(scope.ctx()),
            scope,
        }
    }
}

struct PassGuard {
    installed: bool,
}

impl Drop for PassGuard {
    fn drop(&mut self) {
        if self.installed {
            PASS.with(|slot| *slot.borrow_mut() = None);
        }
    }
}

/// Run one compact/commit/closeout/session-check pass with shared projections.
///
/// Nested calls reuse the outer graph. The graph is dropped even if the pass
/// unwinds, so cached authority never leaks into a later command.
pub fn with_current_document_projection_pass<T>(f: impl FnOnce() -> T) -> T {
    let installed = PASS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            false
        } else {
            *slot = Some(PassGraph::new());
            true
        }
    });
    let _guard = PassGuard { installed };
    f()
}

/// The cache key for a document: its canonical path.
///
/// `#preflightprojpass`: keying on the path AS GIVEN silently splits one
/// document into two cache entries whenever callers disagree about spelling.
/// Measured on 2026-08-09, in one 20-minute window, the same session document
/// resolved 43 times as `/home/brian/.../tasks/agent-doc/agent-doc-bugs2.md` and
/// 40 times as `tasks/agent-doc/agent-doc-bugs2.md` — every one of those a
/// guaranteed miss against the other spelling's entry.
///
/// The wasted work is the smaller problem. Two entries hold two independent
/// revisions, and `invalidate_current_document_projection` clears only the
/// spelling it is handed — so an agent-authored mutation invalidated under one
/// spelling leaves the other serving pre-mutation text. That is the exact trap
/// this module's own doc comment warns about ("memoizing only by path can hide
/// operator edits made during the pass").
///
/// Falls back to the given path when canonicalization fails (a document that
/// does not exist yet), which preserves the previous behavior for that case.
fn projection_key(file: &Path) -> PathBuf {
    std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf())
}

/// Explicitly invalidate after an agent-authored mutation.
///
/// Operator edits do not need this call: their Yrs state vector (or detached
/// disk hash) changes and invalidates automatically on the next read.
pub fn invalidate_current_document_projection(file: &Path) {
    PASS.with(|slot| {
        let slot = slot.borrow();
        let Some(graph) = slot.as_ref() else {
            return;
        };
        let key = projection_key(file);
        let ctx = graph.scope.ctx();
        let next = graph
            .manual_epoch
            .observe(ctx, &key)
            .unwrap_or(0)
            .wrapping_add(1);
        graph.manual_epoch.set(ctx, key, next);
    });
}

fn observed_revision(file: &Path) -> Result<ProjectionRevision> {
    match agent_doc_crdt_relay_io::current_revision_for_file(file)? {
        CurrentRevision::Detached => {
            let content = std::fs::read_to_string(file)?;
            Ok(ProjectionRevision::DiskHash(agent_doc_hash::content_hash(
                &content,
            )))
        }
        revision => Ok(ProjectionRevision::Crdt(revision)),
    }
}

fn pass_projection(
    file: &Path,
    revision: ProjectionRevision,
    resolve: Arc<dyn Fn() -> Result<CurrentDocument> + Send + Sync>,
) -> Option<Arc<CurrentDocumentProjection>> {
    PASS.with(|slot| {
        let slot = slot.borrow();
        let graph = slot.as_ref()?;
        let key = projection_key(file);
        let ctx = graph.scope.ctx();
        if graph.revision.observe(ctx, &key).as_ref() != Some(&revision) {
            graph.revision.set(ctx, key.clone(), revision);
        }
        if !graph.manual_epoch.is_present(&key) {
            graph.manual_epoch.set(ctx, key.clone(), 0);
        }
        let tracked_ctx = ctx.clone();
        let revisions = graph.revision.clone();
        let manual_epochs = graph.manual_epoch.clone();
        graph
            .projection
            .get_or_insert_with(ctx, key, move |_ctx, key| {
                let _revision = revisions.observe(&tracked_ctx, key);
                let _manual_epoch = manual_epochs.observe(&tracked_ctx, key);
                resolve()
                    .ok()
                    .map(CurrentDocumentProjection::new)
                    .map(Arc::new)
            })
    })
}

pub(crate) fn resolve_projection(
    file: &Path,
    source: &str,
) -> Result<Arc<CurrentDocumentProjection>> {
    let file = file.to_path_buf();
    // `#passattrib`: tag every resolve with WHY the pass did not answer it.
    //
    // A pass HIT never calls `resolve`, so each emitted `realtime_doc_resolve`
    // is already a miss — but the line could not say which kind, and the three
    // kinds want completely different fixes:
    //
    // - `bypassed_no_revision`: `observed_revision` failed, so the pass was
    //   never consulted. Every such call is an unconditional re-resolve that no
    //   amount of pass tuning removes.
    // - `uninstalled`: no pass is open on this thread — the caller never
    //   wrapped its work in `with_current_document_projection_pass`.
    // - `miss`: a pass IS open and was consulted; the revision or manual epoch
    //   moved, so the re-read is genuine work.
    //
    // Measured 2026-08-09: one `session-check` run logged 8 resolves with the
    // same handful of guards repeating 2-3x each, and nothing distinguished a
    // wasted re-read from a necessary one. That ambiguity is the blocker on the
    // remaining `#preflightprojpass` work, not the resolve count itself.
    let resolve_tagged =
        |pass: &'static str| -> Arc<dyn Fn() -> Result<CurrentDocument> + Send + Sync> {
            let file = file.clone();
            let source = format!("{source} pass={pass}");
            Arc::new(move || {
                super::try_resolve_current_document_uncached_with_source(&file, &source)
            })
        };

    let Ok(revision) = observed_revision(&file) else {
        return resolve_tagged("bypassed_no_revision")()
            .map(CurrentDocumentProjection::new)
            .map(Arc::new);
    };
    if let Some(projection) = pass_projection(&file, revision, resolve_tagged("miss")) {
        return Ok(projection);
    }
    resolve_tagged("uninstalled")()
        .map(CurrentDocumentProjection::new)
        .map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_document_realtime::reconcile_current_doc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    fn document(file: &Path, content: String) -> CurrentDocument {
        CurrentDocument::new(file, reconcile_current_doc(&content, None))
    }

    fn test_projection(
        file: &Path,
        revision: u64,
        content: Arc<Mutex<String>>,
        calls: Arc<AtomicU64>,
        delay: Duration,
    ) -> Arc<CurrentDocumentProjection> {
        let file_owned = file.to_path_buf();
        let resolve = Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(delay);
            Ok(document(
                &file_owned,
                content.lock().expect("content lock").clone(),
            ))
        });
        pass_projection(
            file,
            ProjectionRevision::DiskHash(revision.to_string()),
            resolve,
        )
        .expect("test pass must be installed")
    }

    /// `#passattrib`: a resolve that cannot say why it missed cannot be acted
    /// on. Every emitted `realtime_doc_resolve` is a miss (a hit never calls
    /// `resolve`), and the three miss kinds want different fixes, so the reason
    /// must be on the line.
    #[test]
    fn every_pass_outcome_is_named_and_the_names_are_distinct() {
        // The reasons are the contract this guard protects; keep them in sync
        // with `resolve_projection`.
        let source = include_str!("current_document_projection.rs");
        let body = source
            .split_once("#[cfg(test)]")
            .map(|(before, _)| before)
            .unwrap_or(source);
        let reasons = ["bypassed_no_revision", "uninstalled", "miss"];
        for reason in reasons {
            assert!(
                body.contains(&format!("resolve_tagged(\"{reason}\")")),
                "`resolve_projection` must tag its {reason} path"
            );
        }
        assert!(
            body.contains("format!(\"{source} pass={pass}\")"),
            "the reason must reach the emitted log line as its own `pass=` field"
        );
        // Distinct, single-token values: a reason with a space would be
        // unparseable from a space-delimited log line — the exact defect that
        // hid this data twice already.
        let mut seen = std::collections::HashSet::new();
        for reason in reasons {
            assert!(!reason.contains(' '), "`{reason}` must be a single token");
            assert!(seen.insert(reason), "`{reason}` must be distinct");
        }
    }

    #[test]
    fn repeated_derived_reads_pay_one_authority_and_parse_cost() {
        static SERIAL: OnceLock<Mutex<()>> = OnceLock::new();
        let _serial = SERIAL.get_or_init(|| Mutex::new(())).lock().unwrap();
        let file = Path::new("/tmp/agent-doc-projection-latency.md");
        let content = Arc::new(Mutex::new(
            "---\nagent_doc_pipeline:\n  step: committing\n---\n\
             <!-- agent:queue -->\n- [ ] do work\n<!-- /agent:queue -->\n"
                .to_string(),
        ));
        let calls = Arc::new(AtomicU64::new(0));

        with_current_document_projection_pass(|| {
            let started = Instant::now();
            for _ in 0..5 {
                let projection = test_projection(
                    file,
                    1,
                    Arc::clone(&content),
                    Arc::clone(&calls),
                    Duration::from_millis(35),
                );
                assert_eq!(
                    projection.pipeline().unwrap().step.as_deref(),
                    Some("committing")
                );
                assert!(projection.queue().unwrap().component("queue").is_some());
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(
                started.elapsed() < Duration::from_millis(150),
                "five readers should reuse the one simulated authority delay"
            );
        });
    }

    /// `#preflightprojpass`: two spellings of one document must share one cache
    /// entry — and one invalidation must clear both.
    ///
    /// Measured on 2026-08-09: in a single 20-minute window the same session
    /// document resolved 43 times under its absolute path and 40 times under its
    /// repo-relative path. Splitting one document into two entries wastes a full
    /// CRDT materialization per miss, and — worse — lets an invalidation under
    /// one spelling leave the other serving pre-mutation text.
    #[test]
    fn two_spellings_of_one_document_share_one_projection_entry() {
        let dir = tempfile::tempdir().unwrap();
        let absolute = dir.path().join("plan.md");
        std::fs::write(&absolute, "seed").unwrap();
        // A second spelling of the SAME file. It must survive `Path`'s
        // component-wise equality, which silently normalizes a `.` hop away — so
        // a `..` hop is used instead. Canonicalization, not string equality, is
        // what has to make these agree.
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let alias = dir.path().join("sub").join("..").join("plan.md");
        assert_ne!(absolute.as_path(), alias.as_path(), "distinct as written");

        let content = Arc::new(Mutex::new(
            "<!-- agent:queue -->\n- [ ] #keep\n<!-- /agent:queue -->\n".to_string(),
        ));
        let calls = Arc::new(AtomicU64::new(0));

        with_current_document_projection_pass(|| {
            let first = test_projection(
                &absolute,
                1,
                Arc::clone(&content),
                Arc::clone(&calls),
                Duration::ZERO,
            );
            assert!(first.document().content().contains("#keep"));
            assert_eq!(calls.load(Ordering::SeqCst), 1);

            // The other spelling must HIT, not resolve again.
            let second = test_projection(
                &alias,
                1,
                Arc::clone(&content),
                Arc::clone(&calls),
                Duration::ZERO,
            );
            assert!(second.document().content().contains("#keep"));
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "the alias spelling must reuse the entry, not mint a second one"
            );

            // An agent-authored mutation invalidated under ONE spelling must be
            // visible through the OTHER. This is the correctness half: a stale
            // second entry serves pre-mutation text.
            *content.lock().unwrap() =
                "<!-- agent:queue -->\n- [ ] #added\n<!-- /agent:queue -->\n".to_string();
            invalidate_current_document_projection(&absolute);

            let after = test_projection(
                &alias,
                1,
                Arc::clone(&content),
                Arc::clone(&calls),
                Duration::ZERO,
            );
            assert!(
                after.document().content().contains("#added"),
                "invalidation under one spelling must reach the other"
            );
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        });
    }

    #[test]
    fn revision_changes_expose_operator_additions_and_deletions() {
        let file = Path::new("/tmp/agent-doc-projection-concurrent-edit.md");
        let content = Arc::new(Mutex::new(
            "<!-- agent:queue -->\n- [ ] #keep\n<!-- /agent:queue -->\n".to_string(),
        ));
        let calls = Arc::new(AtomicU64::new(0));

        with_current_document_projection_pass(|| {
            let first = test_projection(
                file,
                1,
                Arc::clone(&content),
                Arc::clone(&calls),
                Duration::ZERO,
            );
            assert!(first.document().content().contains("#keep"));

            content
                .lock()
                .unwrap()
                .insert_str("<!-- agent:queue -->\n".len(), "- [ ] #added\n");
            let added = test_projection(
                file,
                2,
                Arc::clone(&content),
                Arc::clone(&calls),
                Duration::ZERO,
            );
            assert!(added.document().content().contains("#added"));

            *content.lock().unwrap() =
                "<!-- agent:queue -->\n- [ ] #added\n<!-- /agent:queue -->\n".to_string();
            let deleted = test_projection(
                file,
                3,
                Arc::clone(&content),
                Arc::clone(&calls),
                Duration::ZERO,
            );
            assert!(!deleted.document().content().contains("#keep"));
            assert_eq!(calls.load(Ordering::SeqCst), 3);
        });
    }
}
