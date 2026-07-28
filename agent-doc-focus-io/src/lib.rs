//! # Module: focus
//!
//! Focus the tmux pane associated with a session document.
//!
//! Usage: `agent-doc focus <file.md>`
//!
//! ## Spec
//! - `run(file, pane)`: entry point. Without an explicit pane it delegates to the
//!   Project Controller so one serialized owner can focus an existing pane or
//!   resume a killed latest session before focusing it. An explicit pane remains
//!   the direct tmux escape hatch.
//! - `run_blocking(file, pane)`: legacy synchronous path; performs best-effort
//!   stash promotion before selecting the resolved pane.
//! - `run_with_tmux(file, pane_override, tmux)`: if `pane_override` is `Some`, skips
//!   frontmatter lookup and calls `tmux select-pane` on the supplied pane directly;
//!   errors if the override pane is not alive.
//! - When `pane_override` is `None`, reads the file from disk, parses YAML frontmatter,
//!   and extracts the `agent_doc_session` UUID; errors if the field is absent.
//! - The standalone `run_with_tmux*` helpers and explicit-pane path retain the
//!   local resolution behavior below for layout internals and operator overrides.
//! - When `.agent-doc/state.db` has a live local actor record for the document
//!   session, focus prefers that actor-owned pane over stale durable registry
//!   metadata without launching or waiting on the project controller.
//! - Otherwise, looks up the UUID in the durable registry via
//!   `agent_doc_session_registry_io::lookup`.
//! - Live-owner precedence: a pane resolved from the local actor record or the
//!   registry is only proof the pane is *alive*, not that it still owns the document.
//!   After a reroute / fresh-restart the session can move to a new pane while the old
//!   pane stays alive with a dead owner. Before selecting, focus reconciles the
//!   candidate against `sync::find_live_owner_pane_quiet`; when a different pane
//!   provably owns the document right now, focus swaps to that live owner and lets
//!   resync repair the registry. This also recovers a dead-registered or
//!   unregistered document whose session is still running in another pane, instead
//!   of failing closed.
//! - On success, calls `tmux select-pane` when the resolved pane is already visible
//!   in the agent-doc window and logs the focused pane + file path to stderr. If the
//!   pane is parked in the stash window, default focus defers surfacing + selection
//!   to the sync reconciler so the tmux pane switch stays fast and does not grow the
//!   visible layout additively.
//!
//! ## Agentic Contracts
//! - `run_with_tmux` never modifies the durable registry or the document on disk.
//! - A file without `agent_doc_session` in its frontmatter always returns an error with
//!   a message directing the caller to run `claim` first.
//! - A registered pane that is no longer alive returns an error; the caller is responsible
//!   for pruning or re-claiming.
//! - `pane_override` is an escape hatch for callers that already know the pane ID (e.g.
//!   `layout.rs` focusing a resolved pane); it bypasses all registry and frontmatter I/O.
//!
//! ## Evals
//! - `focus_live_pane` (aspirational): file has a valid session UUID and a live pane →
//!   `select-pane` is called and `Ok(())` is returned.
//! - `focus_prefers_local_actor_record` (aspirational): stale registry pane +
//!   live local actor record → focus selects the actor-owned pane without an RPC.
//! - `focus_dead_pane` (aspirational): session UUID exists in registry, pane is dead, and
//!   no live owner is provable → error containing "pane … is dead" is returned.
//! - `focus_repairs_stale_registry_to_live_owner` (aspirational): registered pane is alive
//!   but its owner is gone while the document runs in another pane → focus selects the live
//!   owner pane, not the stale registry pane.
//! - `focus_no_session` (aspirational): file frontmatter has no `agent_doc_session` →
//!   error directing caller to run `claim` is returned.
//! - `focus_file_not_found` (aspirational): file path does not exist on disk →
//!   error containing "file not found" is returned.
//! - `focus_pane_override_live` (aspirational): `pane_override` supplied and pane is live →
//!   registry is never read and `select-pane` is called on the override pane.
//! - `focus_pane_override_dead` (aspirational): `pane_override` supplied but pane is dead →
//!   error containing "pane … is dead" is returned without reading frontmatter.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_frontmatter::frontmatter;
use agent_doc_tmux::{FocusPaneDecision, decide_focus_pane};
use tmux_router::Tmux;

pub trait FocusEffects {
    fn focus_or_resume_document_via_controller(&self, file: &Path) -> Result<()>;

    fn find_live_owner_pane_quiet(
        &self,
        tmux: &Tmux,
        file: &Path,
        session_id: &str,
    ) -> Option<String>;

    fn local_actor_record_pane_for_document(
        &self,
        file: &Path,
        session_id: &str,
        tmux: &Tmux,
    ) -> Option<String>;

    fn pane_in_stash_window(&self, tmux: &Tmux, pane: &str) -> bool;

    fn promote_pane_to_agent_doc_window(&self, tmux: &Tmux, pane: &str) -> Result<bool>;
}

/// Resolve the document's live owner pane, but only return it when it is alive
/// and differs from `candidate`. This is the stale-record / stale-registry
/// repair used by [`run_with_tmux`]: after a reroute or fresh-restart moves the
/// session to a new pane, the durable registry can still point at the old
/// pane (alive as a pane but with a dead owner), so focus must defer to where
/// the session actually lives.
fn live_owner_override(
    effects: &impl FocusEffects,
    file: &Path,
    session_id: &str,
    candidate: &str,
    tmux: &Tmux,
) -> Option<String> {
    let owner = effects.find_live_owner_pane_quiet(tmux, file, session_id)?;
    match decide_focus_pane(candidate, Some(owner.as_str())) {
        FocusPaneDecision::RepairToLiveOwner(owner) if tmux.pane_alive(&owner) => Some(owner),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegisteredFocusDecision<'a> {
    SelectRegistered,
    RepairToLiveOwner(&'a str),
    FailUnproven,
}

fn decide_registered_focus_candidate<'a>(
    registered_pane: &'a str,
    live_owner: Option<&'a str>,
) -> RegisteredFocusDecision<'a> {
    match live_owner {
        Some(owner) if owner != registered_pane => {
            RegisteredFocusDecision::RepairToLiveOwner(owner)
        }
        Some(_) => RegisteredFocusDecision::SelectRegistered,
        None => RegisteredFocusDecision::FailUnproven,
    }
}

pub fn run(effects: &impl FocusEffects, file: &Path, pane: Option<&str>) -> Result<()> {
    if pane.is_none() {
        return effects.focus_or_resume_document_via_controller(file);
    }
    run_with_tmux(effects, file, pane, &Tmux::default_server())
}

/// Legacy synchronous focus path. This preserves the old standalone `focus`
/// behavior for operators that explicitly want focus to surface a stashed pane
/// before returning.
pub fn run_blocking(effects: &impl FocusEffects, file: &Path, pane: Option<&str>) -> Result<()> {
    run_with_tmux_blocking(effects, file, pane, &Tmux::default_server())
}

/// Promote a live-owner pane out of the stash window (best-effort) and then
/// select it, so editor focus surfaces the session in the working agent-doc
/// layout instead of selecting it in place inside the stash
/// (`#stash-pane-promote-on-focus`). tmux preserves the pane id across the
/// reparent, so we select the same pane id either way.
///
/// When `defer_stash_promote` is set the additive promote is skipped and stash
/// surfacing is left to the sync reconciler (`#jb-nav-3pane-promote-swap`).
fn promote_and_select(
    effects: &impl FocusEffects,
    tmux: &Tmux,
    pane: &str,
    defer_stash_promote: bool,
) -> Result<()> {
    if defer_stash_promote {
        // Editor-navigation focus defers stash reparenting to the sync
        // reconciler (`#jb-nav-3pane-promote-swap`) so the additive promote does
        // not race the reconcile and grow the window to an extra pane. The
        // selection must be deferred together with the reparenting: selecting a
        // pane that still lives in the stash window surfaces editor focus
        // *inside* the stash (`#jb-tsift-pane-sync`). Leave surfacing + selection
        // to the reconciler's atomic SWAP, which swaps the stashed pane into the
        // agent-doc window and selects the focus pane in one operation.
        if effects.pane_in_stash_window(tmux, pane) {
            eprintln!(
                "[focus] pane {} is stashed; deferring surface+select to the sync reconciler (not selecting in place)",
                pane
            );
            return Ok(());
        }
        return tmux.select_pane(pane);
    }
    if let Err(e) = effects.promote_pane_to_agent_doc_window(tmux, pane) {
        eprintln!("[focus] stash promotion check failed for {}: {}", pane, e);
    }
    tmux.select_pane(pane)
}

pub fn run_with_tmux(
    effects: &impl FocusEffects,
    file: &Path,
    pane_override: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_with_tmux_opts(effects, file, pane_override, tmux, true)
}

pub fn run_with_tmux_blocking(
    effects: &impl FocusEffects,
    file: &Path,
    pane_override: Option<&str>,
    tmux: &Tmux,
) -> Result<()> {
    run_with_tmux_opts(effects, file, pane_override, tmux, false)
}

pub fn run_with_tmux_opts(
    effects: &impl FocusEffects,
    file: &Path,
    pane_override: Option<&str>,
    tmux: &Tmux,
    defer_stash_promote: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // If an explicit pane was provided, use it directly
    if let Some(p) = pane_override {
        if tmux.pane_alive(p) {
            promote_and_select(effects, tmux, p, defer_stash_promote)?;
            eprintln!("Focused pane {} ({})", p, file.display());
            return Ok(());
        } else {
            anyhow::bail!("pane {} is dead for {}", p, file.display());
        }
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let (fm, _) = frontmatter::parse(&content)?;
    let session_id = match fm.session {
        Some(id) => id,
        None => anyhow::bail!(
            "no session UUID in {} (use Claim to register)",
            file.display()
        ),
    };

    if let Some(actor_pane) = effects.local_actor_record_pane_for_document(file, &session_id, tmux)
    {
        // The local actor record only proves the pane is *alive*, not that
        // it still owns this document. After a reroute / fresh-restart the
        // session may have moved; defer to the live owner when one exists.
        if let Some(owner) = live_owner_override(effects, file, &session_id, &actor_pane, tmux) {
            promote_and_select(effects, tmux, &owner, defer_stash_promote)?;
            eprintln!(
                "Focused live-owner pane {} (stale actor record {}) ({})",
                owner,
                actor_pane,
                file.display()
            );
            return Ok(());
        }
        promote_and_select(effects, tmux, &actor_pane, defer_stash_promote)?;
        eprintln!("Focused pane {} ({})", actor_pane, file.display());
        return Ok(());
    }

    let pane = agent_doc_session_registry_io::lookup(&session_id)?;
    match pane {
        Some(pane_id) if tmux.pane_alive(&pane_id) => {
            // A registered pane that is alive as a *pane* is not proof it still
            // owns the document — its owner process may be gone while the live
            // session runs in another pane, or the pane may be a stale
            // geometry-only binding from passive editor sync. Select only a
            // pane that currently proves ownership.
            let live_owner = effects
                .find_live_owner_pane_quiet(tmux, file, &session_id)
                .filter(|owner| tmux.pane_alive(owner));
            match decide_registered_focus_candidate(&pane_id, live_owner.as_deref()) {
                RegisteredFocusDecision::RepairToLiveOwner(owner) => {
                    promote_and_select(effects, tmux, owner, defer_stash_promote)?;
                    eprintln!(
                        "Focused live-owner pane {} (stale registry pane {}) ({})",
                        owner,
                        pane_id,
                        file.display()
                    );
                    return Ok(());
                }
                RegisteredFocusDecision::SelectRegistered => {}
                RegisteredFocusDecision::FailUnproven => {
                    anyhow::bail!(
                        "registered pane {} is alive for {} but no live agent-doc owner was proven; run `agent-doc start {}` to create a fresh pane",
                        pane_id,
                        file.display(),
                        file.display()
                    );
                }
            }
            promote_and_select(effects, tmux, &pane_id, defer_stash_promote)?;
            eprintln!("Focused pane {} ({})", pane_id, file.display());
            Ok(())
        }
        Some(pane_id) => {
            // The registered pane is dead. Before failing, recover the live
            // owner if the session is still running in a different pane.
            if let Some(owner) = effects
                .find_live_owner_pane_quiet(tmux, file, &session_id)
                .filter(|owner| tmux.pane_alive(owner))
            {
                promote_and_select(effects, tmux, &owner, defer_stash_promote)?;
                eprintln!(
                    "Focused live-owner pane {} (registered pane {} is dead) ({})",
                    owner,
                    pane_id,
                    file.display()
                );
                return Ok(());
            }
            anyhow::bail!("pane {} is dead for {}", pane_id, file.display());
        }
        None => {
            // No registry entry, but the session may still be running with a
            // lost registration — recover the live owner pane if provable.
            if let Some(owner) = effects
                .find_live_owner_pane_quiet(tmux, file, &session_id)
                .filter(|owner| tmux.pane_alive(owner))
            {
                promote_and_select(effects, tmux, &owner, defer_stash_promote)?;
                eprintln!(
                    "Focused live-owner pane {} (no registry entry) ({})",
                    owner,
                    file.display()
                );
                return Ok(());
            }
            anyhow::bail!(
                "no pane registered for {} (session {})",
                file.display(),
                &session_id[..std::cmp::min(8, session_id.len())]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ControllerFocusEffects;

    impl FocusEffects for ControllerFocusEffects {
        fn focus_or_resume_document_via_controller(&self, _file: &Path) -> Result<()> {
            Ok(())
        }

        fn find_live_owner_pane_quiet(
            &self,
            _tmux: &Tmux,
            _file: &Path,
            _session_id: &str,
        ) -> Option<String> {
            None
        }

        fn local_actor_record_pane_for_document(
            &self,
            _file: &Path,
            _session_id: &str,
            _tmux: &Tmux,
        ) -> Option<String> {
            None
        }

        fn pane_in_stash_window(&self, _tmux: &Tmux, _pane: &str) -> bool {
            false
        }

        fn promote_pane_to_agent_doc_window(&self, _tmux: &Tmux, _pane: &str) -> Result<bool> {
            Ok(false)
        }
    }

    #[test]
    fn default_focus_delegates_to_controller_before_local_file_or_registry_checks() {
        let missing = Path::new("/definitely/missing/agent-doc-focus.md");
        run(&ControllerFocusEffects, missing, None).unwrap();
    }

    #[test]
    fn registered_focus_decision_selects_matching_live_owner() {
        assert_eq!(
            decide_registered_focus_candidate("%7", Some("%7")),
            RegisteredFocusDecision::SelectRegistered
        );
    }

    #[test]
    fn registered_focus_decision_repairs_to_different_live_owner() {
        assert_eq!(
            decide_registered_focus_candidate("%7", Some("%9")),
            RegisteredFocusDecision::RepairToLiveOwner("%9")
        );
    }

    #[test]
    fn registered_focus_decision_fails_unproven_alive_registry_pane() {
        assert_eq!(
            decide_registered_focus_candidate("%7", None),
            RegisteredFocusDecision::FailUnproven
        );
    }
}
