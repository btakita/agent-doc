//! Backlog, icebox, review, and done-archive I/O adapters.
//!
//! Pure tracked-work parsing and mutation policy lives in
//! `agent-doc-element-backlog` and `agent-doc-element-review`. This crate owns
//! path-aware command/document mutation adapters and the done-archive store.

use anyhow::Result;
use std::cell::RefCell;
use std::path::Path;

pub mod backlog_cmd;
pub mod cross_document;
pub mod deletion_authority;
pub mod done_archive;

pub trait BacklogCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()>;

    fn record_document_write_provenance(&self, file: &Path, content: &str);

    /// `#backlogiceboxreview`: a standalone tracked-work command's write landed;
    /// attribute its structural delta (`before` → `after`) to the agent in the
    /// session baseline. Only called while
    /// [`with_agent_write_baseline_attribution`] is active. Effects without a
    /// baseline store keep the default no-op.
    fn attribute_agent_write_to_baseline(&self, _file: &Path, _before: &str, _after: &str) {}
}

thread_local! {
    static AGENT_WRITE_BASELINE_ATTRIBUTION: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Restores the previous attribution scope when dropped.
pub struct AgentWriteBaselineAttributionGuard {
    previous: bool,
}

impl Drop for AgentWriteBaselineAttributionGuard {
    fn drop(&mut self) {
        AGENT_WRITE_BASELINE_ATTRIBUTION.with(|slot| slot.set(self.previous));
    }
}

/// `#backlogiceboxreview`: mark the rest of this thread's work as a standalone
/// binary-owned tracked-work command (`agent-doc backlog|icebox|review`), whose
/// landed writes advance the session baseline by their own structural delta.
///
/// The response/closeout path does NOT enter this scope: its commit owns the
/// baseline for the whole cycle.
pub fn enter_agent_write_baseline_attribution() -> AgentWriteBaselineAttributionGuard {
    let previous = AGENT_WRITE_BASELINE_ATTRIBUTION.with(|slot| slot.replace(true));
    AgentWriteBaselineAttributionGuard { previous }
}

/// Run `f` inside [`enter_agent_write_baseline_attribution`].
pub fn with_agent_write_baseline_attribution<T>(f: impl FnOnce() -> T) -> T {
    let _guard = enter_agent_write_baseline_attribution();
    f()
}

pub(crate) fn agent_write_baseline_attribution_active() -> bool {
    AGENT_WRITE_BASELINE_ATTRIBUTION.with(std::cell::Cell::get)
}

thread_local! {
    static CURRENT_EFFECTS: RefCell<Vec<&'static dyn BacklogCommandEffects>> =
        RefCell::new(Vec::new());
}

pub fn with_backlog_command_effects<T>(
    effects: &'static dyn BacklogCommandEffects,
    f: impl FnOnce() -> T,
) -> T {
    CURRENT_EFFECTS.with(|slot| slot.borrow_mut().push(effects));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CURRENT_EFFECTS.with(|slot| {
        slot.borrow_mut()
            .pop()
            .expect("backlog command effects stack underflow");
    });
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub(crate) fn current_document_content(file: &Path, source: &str) -> Result<String> {
    CURRENT_EFFECTS.with(|slot| {
        if let Some(effects) = slot.borrow().last().copied() {
            return effects.current_document_content(file, source);
        }
        anyhow::bail!(
            "backlog command read effects are not installed for {}",
            file.display()
        )
    })
}

pub(crate) fn force_disk_document_content(file: &Path, source: &str) -> Result<String> {
    CURRENT_EFFECTS.with(|slot| {
        if let Some(effects) = slot.borrow().last().copied() {
            return effects.force_disk_document_content(file, source);
        }
        anyhow::bail!(
            "backlog command force-disk read effects are not installed for {}",
            file.display()
        )
    })
}

pub(crate) fn converge_or_disk_write(
    file: &Path,
    current_content: &str,
    target_content: &str,
    reason: &str,
) -> Result<()> {
    CURRENT_EFFECTS.with(|slot| {
        if let Some(effects) = slot.borrow().last().copied() {
            return effects.converge_or_disk_write(file, current_content, target_content, reason);
        }
        anyhow::bail!(
            "backlog command write effects are not installed for {}",
            file.display()
        )
    })
}

pub(crate) fn record_document_write_provenance(file: &Path, content: &str) {
    CURRENT_EFFECTS.with(|slot| {
        if let Some(effects) = slot.borrow().last().copied() {
            effects.record_document_write_provenance(file, content);
        }
    });
}

pub(crate) fn attribute_agent_write_to_baseline(file: &Path, before: &str, after: &str) {
    if !agent_write_baseline_attribution_active() {
        return;
    }
    CURRENT_EFFECTS.with(|slot| {
        if let Some(effects) = slot.borrow().last().copied() {
            effects.attribute_agent_write_to_baseline(file, before, after);
        }
    });
}
