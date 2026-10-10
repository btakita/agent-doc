//! Idempotent tmux effects for detached editor-view bindings (GH #218).
//!
//! [`EditorViewPolicy`](agent_doc_editor_surface::editor_view_policy::EditorViewPolicy)
//! owns whether a document is detached-owned. This module owns only the effect
//! boundary that realizes a durable `BindPending` or `ReleasePending` fact.
//! Cross-session movement is denied everywhere else by `tmux-router`; this
//! adapter is the deliberately narrow authorization point.

use std::path::PathBuf;

pub use agent_doc_editor_surface::{EditorViewSessionKey, isolated_view_session_name};
use agent_doc_state_backbone::{
    EditorViewBindingIdentity, EditorViewBindingState, EditorViewPaneReceipt,
    EditorViewReleaseDestination, EditorViewReleaseReason, StateFact,
};
use anyhow::{Context as _, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tmux_router::{PaneMoveOp, Tmux};

const VIEW_WINDOW_NAME: &str = "view";
const CROSS_SESSION_REASON: &str = "agent-doc editor-view lifecycle";

/// One typed reconciliation request. The pane identity comes from the durable
/// actor/registry projection, never from focus or ambient tmux state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorViewTmuxRequest {
    pub document_hash: String,
    pub canonical_path: String,
    pub binding_epoch: u64,
    pub state: EditorViewBindingState,
    pub pane_id: String,
    pub actor_generation: u64,
    pub main_session: String,
    pub main_window_id: String,
    pub project_root: PathBuf,
    pub session_key: EditorViewSessionKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorViewTmuxEffect {
    Bind,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewPanePlacement {
    pub pane_id: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub window_width: u32,
    pub window_height: u32,
}

/// Main-session evidence captured on both sides of an effect. `stash_panes`
/// is sorted so a complete bind/release round trip has a byte-stable snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewMainSnapshot {
    pub current_window: String,
    pub active_pane: String,
    pub main_window_id: String,
    pub main_window_layout: String,
    pub main_window_width: u32,
    pub main_window_height: u32,
    pub main_window_panes: Vec<String>,
    pub stash_window_id: String,
    pub stash_panes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditorViewTmuxReceipt {
    pub document_hash: String,
    pub canonical_path: String,
    pub binding_epoch: u64,
    pub view_id: String,
    pub effect: EditorViewTmuxEffect,
    pub pane: EditorViewPanePlacement,
    pub main_before: EditorViewMainSnapshot,
    pub main_after: EditorViewMainSnapshot,
    pub verified_exact_pane_placement: bool,
    pub view_session_removed: bool,
    pub settled_state: EditorViewBindingState,
}

impl EditorViewTmuxReceipt {
    /// Durable superseding fact to append before applying any later policy
    /// transition. The epoch/view fencing is preserved verbatim from the
    /// pending intent that authorized this effect.
    pub fn settled_fact(&self) -> StateFact {
        StateFact::EditorViewBindingObserved {
            document_hash: self.document_hash.clone(),
            canonical_path: self.canonical_path.clone(),
            binding_epoch: self.binding_epoch,
            state: self.settled_state.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorViewTmuxSurvey {
    pub pane_session: Option<String>,
    pub pane_window_name: Option<String>,
    pub view_session_exists: bool,
    pub view_window_name: Option<String>,
    pub view_panes: Vec<String>,
    pub main_stash_exists: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorViewTmuxStep {
    CreateViewPlaceholder,
    MoveExactPaneToView,
    RemovePlaceholder,
    VerifyBound,
    MoveExactPaneToMainStash,
    VerifyReleased,
    KillViewSession,
    SettleBound,
    SettleReleased,
}

/// Pure restart planner. An executor may crash between any two steps; a fresh
/// survey then chooses only the remaining idempotent suffix.
pub fn plan_editor_view_tmux(
    state: &EditorViewBindingState,
    pane_id: &str,
    main_session: &str,
    survey: &EditorViewTmuxSurvey,
) -> Result<Vec<EditorViewTmuxStep>> {
    match state {
        EditorViewBindingState::BindPending { .. } => {
            if survey.pane_session.as_deref() == Some(main_session)
                && survey.pane_window_name.as_deref() == Some("stash")
            {
                let mut steps = Vec::new();
                if !survey.view_session_exists {
                    steps.push(EditorViewTmuxStep::CreateViewPlaceholder);
                } else {
                    ensure!(
                        survey.view_window_name.as_deref() == Some(VIEW_WINDOW_NAME),
                        "view session exists without the reserved 'view' window"
                    );
                    ensure!(
                        survey.view_panes.len() == 1,
                        "view session placeholder must contain exactly one pane"
                    );
                }
                steps.extend([
                    EditorViewTmuxStep::MoveExactPaneToView,
                    EditorViewTmuxStep::RemovePlaceholder,
                    EditorViewTmuxStep::VerifyBound,
                    EditorViewTmuxStep::SettleBound,
                ]);
                return Ok(steps);
            }

            if survey
                .pane_session
                .as_deref()
                .is_some_and(|session| session != main_session && survey.view_session_exists)
            {
                ensure!(
                    survey.view_window_name.as_deref() == Some(VIEW_WINDOW_NAME),
                    "bound pane is not in a reserved view window"
                );
                ensure!(
                    survey.view_panes.iter().any(|pane| pane == pane_id),
                    "view session does not contain the requested pane"
                );
                ensure!(
                    survey.view_panes.len() <= 2,
                    "view session contains foreign panes"
                );
                let mut steps = Vec::new();
                if survey.view_panes.len() == 2 {
                    steps.push(EditorViewTmuxStep::RemovePlaceholder);
                }
                steps.extend([
                    EditorViewTmuxStep::VerifyBound,
                    EditorViewTmuxStep::SettleBound,
                ]);
                return Ok(steps);
            }
            bail!("bind reconciliation requires the exact pane in main stash or its view session")
        }
        EditorViewBindingState::ReleasePending { .. } => {
            ensure!(
                survey.main_stash_exists,
                "release reconciliation requires an existing main stash"
            );
            if survey.pane_session.as_deref() == Some(main_session)
                && survey.pane_window_name.as_deref() == Some("stash")
            {
                let mut steps = vec![EditorViewTmuxStep::VerifyReleased];
                if survey.view_session_exists {
                    steps.push(EditorViewTmuxStep::KillViewSession);
                }
                steps.push(EditorViewTmuxStep::SettleReleased);
                return Ok(steps);
            }
            ensure!(
                survey.view_session_exists
                    && survey.view_window_name.as_deref() == Some(VIEW_WINDOW_NAME)
                    && survey.view_panes == [pane_id],
                "release reconciliation requires exactly the requested pane in its view session"
            );
            Ok(vec![
                EditorViewTmuxStep::MoveExactPaneToMainStash,
                EditorViewTmuxStep::VerifyReleased,
                EditorViewTmuxStep::KillViewSession,
                EditorViewTmuxStep::SettleReleased,
            ])
        }
        EditorViewBindingState::Bound { pane, .. } => {
            ensure!(pane.pane_id == pane_id, "bound pane receipt mismatch");
            ensure!(
                survey.view_session_exists
                    && survey.view_window_name.as_deref() == Some(VIEW_WINDOW_NAME)
                    && survey.view_panes == [pane_id],
                "settled bound state drifted from its isolated view"
            );
            Ok(Vec::new())
        }
        EditorViewBindingState::Released { .. } => Ok(Vec::new()),
    }
}

pub fn reconcile_editor_view_tmux(
    tmux: &Tmux,
    request: &EditorViewTmuxRequest,
) -> Result<EditorViewTmuxReceipt> {
    let expected_view_session = isolated_view_session_name(&request.session_key);
    let binding = request.state.binding();
    ensure!(
        binding.view_session == expected_view_session,
        "durable view session '{}' does not match derived session '{}'",
        binding.view_session,
        expected_view_session
    );
    match &request.state {
        EditorViewBindingState::BindPending { .. } => bind(tmux, request, binding),
        EditorViewBindingState::ReleasePending {
            reason,
            destination,
            ..
        } => release(tmux, request, binding, *reason, *destination),
        _ => bail!("reconcile requires a pending editor-view lifecycle state"),
    }
}

fn bind(
    tmux: &Tmux,
    request: &EditorViewTmuxRequest,
    binding: &EditorViewBindingIdentity,
) -> Result<EditorViewTmuxReceipt> {
    let view_session = &binding.view_session;
    let before = snapshot_main(tmux, request)?;
    let survey = survey(tmux, request, view_session)?;
    let plan = plan_editor_view_tmux(
        &request.state,
        &request.pane_id,
        &request.main_session,
        &survey,
    )?;

    if plan.contains(&EditorViewTmuxStep::CreateViewPlaceholder) {
        let placeholder = tmux
            .new_session(view_session, &request.project_root)
            .with_context(|| format!("create isolated view session {view_session}"))?;
        tmux.raw_cmd(&["rename-window", "-t", &placeholder, VIEW_WINDOW_NAME])
            .context("name isolated editor-view window")?;
    }

    if plan.contains(&EditorViewTmuxStep::MoveExactPaneToView) {
        require_exact_stashed_pane(tmux, request)?;
        let placeholder = single_view_pane(tmux, view_session)?;
        PaneMoveOp::new(tmux, &request.pane_id, &placeholder)
            .allow_cross_session(CROSS_SESSION_REASON)
            .join("-dh")
            .context("move exact stashed pane into isolated editor view")?;
    }

    if plan.contains(&EditorViewTmuxStep::RemovePlaceholder) {
        let panes = tmux.list_session_panes(view_session);
        ensure!(
            panes.iter().any(|pane| pane == &request.pane_id),
            "isolated view lost requested pane before placeholder removal"
        );
        ensure!(panes.len() <= 2, "isolated view contains foreign panes");
        if let Some(placeholder) = panes.iter().find(|pane| *pane != &request.pane_id) {
            tmux.kill_pane(placeholder)
                .context("remove isolated view placeholder")?;
        }
    }

    let placement = require_exact_view_pane(tmux, request, view_session)?;
    let after = snapshot_main(tmux, request)?;
    ensure_main_unchanged(&before, &after)?;
    let pane_receipt = durable_pane_receipt(request, &placement);
    let settled_state = EditorViewBindingState::Bound {
        binding: binding.clone(),
        pane: pane_receipt,
    };
    Ok(receipt(
        request,
        EditorViewTmuxEffect::Bind,
        placement,
        before,
        after,
        settled_state,
    ))
}

fn release(
    tmux: &Tmux,
    request: &EditorViewTmuxRequest,
    binding: &EditorViewBindingIdentity,
    reason: EditorViewReleaseReason,
    destination: EditorViewReleaseDestination,
) -> Result<EditorViewTmuxReceipt> {
    ensure!(
        destination == EditorViewReleaseDestination::MainStash,
        "unsupported editor-view release destination"
    );
    let view_session = &binding.view_session;
    let before = snapshot_main(tmux, request)?;
    let survey = survey(tmux, request, view_session)?;
    let plan = plan_editor_view_tmux(
        &request.state,
        &request.pane_id,
        &request.main_session,
        &survey,
    )?;

    if plan.contains(&EditorViewTmuxStep::MoveExactPaneToMainStash) {
        require_exact_view_pane(tmux, request, view_session)?;
        let stash_window = tmux
            .find_stash_window(&request.main_session)
            .context("main stash disappeared during editor-view release")?;
        let stash_target = tmux
            .largest_pane_in_window(&stash_window)
            .context("main stash has no target pane")?;
        let selected_stash_pane = tmux
            .raw_cmd(&["display-message", "-t", &stash_window, "-p", "#{pane_id}"])
            .ok();
        PaneMoveOp::new(tmux, &request.pane_id, &stash_target)
            .allow_cross_session(CROSS_SESSION_REASON)
            .join("-dv")
            .context("release exact view pane to main stash")?;
        if let Some(selected) = selected_stash_pane.filter(|pane| pane != &request.pane_id) {
            tmux.raw_cmd(&["select-pane", "-t", &selected])
                .context("restore stash internal selection")?;
        }
    }

    let placement = require_exact_stashed_pane(tmux, request)?;
    if tmux.session_exists(view_session) {
        let panes = tmux.list_session_panes(view_session);
        ensure!(
            panes.is_empty(),
            "refusing to kill view session containing foreign panes: {panes:?}"
        );
        tmux.kill_session(view_session)
            .context("remove verified empty editor-view session")?;
    }
    ensure!(
        !tmux.session_exists(view_session),
        "view session survived verified release"
    );
    let after = snapshot_main(tmux, request)?;
    ensure_main_unchanged(&before, &after)?;
    let pane_receipt = durable_pane_receipt(request, &placement);
    let settled_state = EditorViewBindingState::Released {
        binding: binding.clone(),
        pane: Some(pane_receipt),
        reason,
        destination,
    };
    Ok(receipt(
        request,
        EditorViewTmuxEffect::Release,
        placement,
        before,
        after,
        settled_state,
    ))
}

fn receipt(
    request: &EditorViewTmuxRequest,
    effect: EditorViewTmuxEffect,
    pane: EditorViewPanePlacement,
    main_before: EditorViewMainSnapshot,
    main_after: EditorViewMainSnapshot,
    settled_state: EditorViewBindingState,
) -> EditorViewTmuxReceipt {
    EditorViewTmuxReceipt {
        document_hash: request.document_hash.clone(),
        canonical_path: request.canonical_path.clone(),
        binding_epoch: request.binding_epoch,
        view_id: request.state.binding().view_id.clone(),
        effect,
        pane,
        main_before,
        main_after,
        verified_exact_pane_placement: true,
        view_session_removed: effect == EditorViewTmuxEffect::Release,
        settled_state,
    }
}

fn durable_pane_receipt(
    request: &EditorViewTmuxRequest,
    placement: &EditorViewPanePlacement,
) -> EditorViewPaneReceipt {
    EditorViewPaneReceipt {
        pane_id: placement.pane_id.clone(),
        actor_generation: request.actor_generation,
        session_name: placement.session_name.clone(),
        window_id: placement.window_id.clone(),
    }
}

fn survey(
    tmux: &Tmux,
    request: &EditorViewTmuxRequest,
    view_session: &str,
) -> Result<EditorViewTmuxSurvey> {
    let pane_session = tmux.pane_session(&request.pane_id).ok();
    let pane_window_name = tmux
        .pane_window_identity(&request.pane_id)
        .ok()
        .map(|(_, name)| name);
    let view_session_exists = tmux.session_exists(view_session);
    let view_panes = if view_session_exists {
        tmux.list_session_panes(view_session)
    } else {
        Vec::new()
    };
    let view_window_name = view_session_exists
        .then(|| {
            tmux.raw_cmd(&[
                "display-message",
                "-t",
                &format!("{view_session}:"),
                "-p",
                "#{window_name}",
            ])
            .ok()
        })
        .flatten();
    Ok(EditorViewTmuxSurvey {
        pane_session,
        pane_window_name,
        view_session_exists,
        view_window_name,
        view_panes,
        main_stash_exists: tmux.find_stash_window(&request.main_session).is_some(),
    })
}

fn single_view_pane(tmux: &Tmux, view_session: &str) -> Result<String> {
    let panes = tmux.list_session_panes(view_session);
    ensure!(
        panes.len() == 1,
        "isolated view placeholder must contain exactly one pane"
    );
    Ok(panes[0].clone())
}

fn require_exact_view_pane(
    tmux: &Tmux,
    request: &EditorViewTmuxRequest,
    view_session: &str,
) -> Result<EditorViewPanePlacement> {
    ensure!(
        tmux.session_exists(view_session),
        "isolated view session missing"
    );
    let panes = tmux.list_session_panes(view_session);
    ensure!(
        panes == [request.pane_id.as_str()],
        "isolated view must contain exactly pane {}, observed {panes:?}",
        request.pane_id
    );
    let placement = pane_placement(tmux, &request.pane_id)?;
    ensure!(
        placement.session_name == view_session,
        "view session mismatch"
    );
    ensure!(
        placement.window_name == VIEW_WINDOW_NAME,
        "view window name mismatch"
    );
    Ok(placement)
}

fn require_exact_stashed_pane(
    tmux: &Tmux,
    request: &EditorViewTmuxRequest,
) -> Result<EditorViewPanePlacement> {
    let placement = pane_placement(tmux, &request.pane_id)?;
    ensure!(
        placement.session_name == request.main_session,
        "pane {} is not in main session {}",
        request.pane_id,
        request.main_session
    );
    ensure!(
        placement.window_name == "stash",
        "pane is not in main stash"
    );
    Ok(placement)
}

fn pane_placement(tmux: &Tmux, pane_id: &str) -> Result<EditorViewPanePlacement> {
    let session_name = tmux.pane_session(pane_id)?;
    let (window_id, window_name) = tmux.pane_window_identity(pane_id)?;
    let (window_width, window_height) = window_size(tmux, &window_id)?;
    Ok(EditorViewPanePlacement {
        pane_id: pane_id.to_string(),
        session_name,
        window_id,
        window_name,
        window_width,
        window_height,
    })
}

fn snapshot_main(tmux: &Tmux, request: &EditorViewTmuxRequest) -> Result<EditorViewMainSnapshot> {
    let stash_window_id = tmux
        .find_stash_window(&request.main_session)
        .context("editor-view lifecycle requires an existing main stash")?;
    let current_window = tmux
        .active_window(&request.main_session)
        .context("resolve main current window")?;
    let active_pane = tmux
        .active_pane(&request.main_session)
        .context("resolve main active pane")?;
    let mut stash_panes = tmux.list_window_panes(&stash_window_id)?;
    stash_panes.sort();
    let main_window_panes = tmux.list_panes_ordered(&request.main_window_id)?;
    let main_window_layout = tmux.raw_cmd(&[
        "display-message",
        "-t",
        &request.main_window_id,
        "-p",
        "#{window_layout}",
    ])?;
    let (main_window_width, main_window_height) = window_size(tmux, &request.main_window_id)?;
    Ok(EditorViewMainSnapshot {
        current_window,
        active_pane,
        main_window_id: request.main_window_id.clone(),
        main_window_layout,
        main_window_width,
        main_window_height,
        main_window_panes,
        stash_window_id,
        stash_panes,
    })
}

fn window_size(tmux: &Tmux, window_id: &str) -> Result<(u32, u32)> {
    let raw = tmux.raw_cmd(&[
        "display-message",
        "-t",
        window_id,
        "-p",
        "#{window_width} #{window_height}",
    ])?;
    let mut fields = raw.split_whitespace();
    let width = fields
        .next()
        .context("missing tmux window width")?
        .parse()
        .context("invalid tmux window width")?;
    let height = fields
        .next()
        .context("missing tmux window height")?
        .parse()
        .context("invalid tmux window height")?;
    Ok((width, height))
}

fn ensure_main_unchanged(
    before: &EditorViewMainSnapshot,
    after: &EditorViewMainSnapshot,
) -> Result<()> {
    ensure!(
        before.current_window == after.current_window,
        "editor-view effect changed main current window"
    );
    ensure!(
        before.active_pane == after.active_pane,
        "editor-view effect changed main active pane"
    );
    ensure!(
        before.main_window_panes == after.main_window_panes,
        "editor-view effect changed main pane membership/order"
    );
    ensure!(
        before.main_window_layout == after.main_window_layout,
        "editor-view effect changed main window layout"
    );
    ensure!(
        (before.main_window_width, before.main_window_height)
            == (after.main_window_width, after.main_window_height),
        "editor-view effect changed main window geometry"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tmux_router::IsolatedTmux;

    fn key() -> EditorViewSessionKey {
        EditorViewSessionKey {
            project_id: "/project".into(),
            client_id: "client-a".into(),
            connection_generation: 7,
            surface_id: "floating-1".into(),
            surface_generation: 3,
        }
    }

    fn binding() -> EditorViewBindingIdentity {
        EditorViewBindingIdentity {
            view_id: "client-a/7/floating-1/3".into(),
            client_family: "client-a".into(),
            connection_generation: 7,
            surface_id: "floating-1".into(),
            surface_generation: 3,
            view_session: isolated_view_session_name(&key()),
        }
    }

    fn pending_bind() -> EditorViewBindingState {
        EditorViewBindingState::BindPending { binding: binding() }
    }

    fn survey_in_stash(view_session_exists: bool) -> EditorViewTmuxSurvey {
        EditorViewTmuxSurvey {
            pane_session: Some("main".into()),
            pane_window_name: Some("stash".into()),
            view_session_exists,
            view_window_name: view_session_exists.then(|| VIEW_WINDOW_NAME.into()),
            view_panes: if view_session_exists {
                vec!["%placeholder".into()]
            } else {
                Vec::new()
            },
            main_stash_exists: true,
        }
    }

    #[test]
    fn session_name_is_bounded_stable_and_generation_scoped() {
        let first = isolated_view_session_name(&key());
        assert_eq!(first, isolated_view_session_name(&key()));
        assert!(first.starts_with("agent-doc-view-"));
        assert!(first.len() <= 32);
        let mut next = key();
        next.surface_generation += 1;
        assert_ne!(first, isolated_view_session_name(&next));
    }

    #[test]
    fn pure_planner_replays_only_the_missing_bind_suffix() {
        assert_eq!(
            plan_editor_view_tmux(&pending_bind(), "%9", "main", &survey_in_stash(false)).unwrap(),
            vec![
                EditorViewTmuxStep::CreateViewPlaceholder,
                EditorViewTmuxStep::MoveExactPaneToView,
                EditorViewTmuxStep::RemovePlaceholder,
                EditorViewTmuxStep::VerifyBound,
                EditorViewTmuxStep::SettleBound,
            ]
        );
        assert_eq!(
            plan_editor_view_tmux(&pending_bind(), "%9", "main", &survey_in_stash(true)).unwrap(),
            vec![
                EditorViewTmuxStep::MoveExactPaneToView,
                EditorViewTmuxStep::RemovePlaceholder,
                EditorViewTmuxStep::VerifyBound,
                EditorViewTmuxStep::SettleBound,
            ]
        );
        let already_moved = EditorViewTmuxSurvey {
            pane_session: Some("view".into()),
            pane_window_name: Some(VIEW_WINDOW_NAME.into()),
            view_session_exists: true,
            view_window_name: Some(VIEW_WINDOW_NAME.into()),
            view_panes: vec!["%9".into(), "%placeholder".into()],
            main_stash_exists: true,
        };
        assert_eq!(
            plan_editor_view_tmux(&pending_bind(), "%9", "main", &already_moved).unwrap(),
            vec![
                EditorViewTmuxStep::RemovePlaceholder,
                EditorViewTmuxStep::VerifyBound,
                EditorViewTmuxStep::SettleBound,
            ]
        );
    }

    #[test]
    fn pure_planner_replays_release_after_the_cross_session_move() {
        let state = EditorViewBindingState::ReleasePending {
            binding: binding(),
            pane: None,
            reason: EditorViewReleaseReason::OwnerClosed,
            destination: EditorViewReleaseDestination::MainStash,
        };
        let moved = survey_in_stash(false);
        assert_eq!(
            plan_editor_view_tmux(&state, "%9", "main", &moved).unwrap(),
            vec![
                EditorViewTmuxStep::VerifyReleased,
                EditorViewTmuxStep::SettleReleased,
            ]
        );
    }

    #[test]
    fn pure_planner_fails_closed_for_non_stash_bind_source() {
        let mut bad = survey_in_stash(false);
        bad.pane_window_name = Some("agent-doc".into());
        assert!(plan_editor_view_tmux(&pending_bind(), "%9", "main", &bad).is_err());
    }

    fn fixture(socket: &str) -> (IsolatedTmux, EditorViewTmuxRequest, EditorViewMainSnapshot) {
        let tmux = IsolatedTmux::new(socket);
        let main_pane = tmux.new_session("main", Path::new("/tmp")).unwrap();
        tmux.raw_cmd(&["rename-window", "-t", &main_pane, "agent-doc"])
            .unwrap();
        let second_main = tmux
            .split_window(&main_pane, Path::new("/tmp"), "-dh")
            .unwrap();
        tmux.select_pane(&main_pane).unwrap();
        let stash_window = tmux.ensure_stash_window("main").unwrap();
        let stash_anchor = tmux.list_window_panes(&stash_window).unwrap()[0].clone();
        let detached = tmux
            .split_window(&stash_anchor, Path::new("/tmp"), "-dv")
            .unwrap();
        let main_window_id = tmux.pane_window(&main_pane).unwrap();
        assert_eq!(
            tmux.active_pane("main").as_deref(),
            Some(main_pane.as_str())
        );
        assert_ne!(main_pane, second_main);
        let request = EditorViewTmuxRequest {
            document_hash: "doc-hash".into(),
            canonical_path: "/project/doc.md".into(),
            binding_epoch: 11,
            state: pending_bind(),
            pane_id: detached,
            actor_generation: 19,
            main_session: "main".into(),
            main_window_id,
            project_root: PathBuf::from("/tmp"),
            session_key: key(),
        };
        let baseline = snapshot_main(&tmux, &request).unwrap();
        (tmux, request, baseline)
    }

    fn wait_for_window_size(tmux: &IsolatedTmux, window_id: &str, expected: (u32, u32)) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if window_size(tmux, window_id).ok() == Some(expected) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "tmux window {window_id} did not settle at {expected:?}; observed {:?}",
                window_size(tmux, window_id),
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn isolated_tmux_bind_release_round_trip_preserves_main_snapshot_bytes() {
        let (tmux, mut request, baseline) = fixture("gh218-view-roundtrip");
        let bind_receipt = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        assert_eq!(bind_receipt.effect, EditorViewTmuxEffect::Bind);
        assert_eq!(bind_receipt.pane.pane_id, request.pane_id);
        assert!(bind_receipt.verified_exact_pane_placement);
        assert!(!bind_receipt.view_session_removed);
        assert_eq!(
            bind_receipt.main_before.current_window,
            bind_receipt.main_after.current_window
        );
        assert_eq!(
            bind_receipt.main_before.active_pane,
            bind_receipt.main_after.active_pane
        );
        assert_eq!(
            bind_receipt.main_before.main_window_panes,
            bind_receipt.main_after.main_window_panes
        );
        assert!(matches!(
            bind_receipt.settled_fact(),
            StateFact::EditorViewBindingObserved {
                binding_epoch: 11,
                state: EditorViewBindingState::Bound { .. },
                ..
            }
        ));

        request.state = EditorViewBindingState::ReleasePending {
            binding: binding(),
            pane: match &bind_receipt.settled_state {
                EditorViewBindingState::Bound { pane, .. } => Some(pane.clone()),
                other => panic!("expected bound receipt, got {other:?}"),
            },
            reason: EditorViewReleaseReason::OwnerClosed,
            destination: EditorViewReleaseDestination::MainStash,
        };
        let release_receipt = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        assert_eq!(release_receipt.effect, EditorViewTmuxEffect::Release);
        assert!(release_receipt.verified_exact_pane_placement);
        assert!(release_receipt.view_session_removed);
        assert!(!tmux.session_exists(&binding().view_session));
        let after_round_trip = snapshot_main(&tmux, &request).unwrap();
        assert_eq!(
            serde_json::to_vec(&baseline).unwrap(),
            serde_json::to_vec(&after_round_trip).unwrap(),
            "main window, stash membership, current window, active pane, and geometry must be byte-identical"
        );
    }

    #[test]
    fn isolated_view_clients_never_contend_with_main_geometry_or_focus_matrix() {
        for policy in ["latest", "largest", "smallest"] {
            for aggressive_resize in ["off", "on"] {
                let socket = format!("gh218-view-geometry-{policy}-{aggressive_resize}");
                let (tmux, request, _) = fixture(&socket);
                let bound = reconcile_editor_view_tmux(&tmux, &request).unwrap();
                let view_session = bound.pane.session_name.clone();
                let view_window = bound.pane.window_id.clone();

                tmux.raw_cmd(&["set-option", "-t", "main", "status", "off"])
                    .unwrap();
                tmux.raw_cmd(&["set-option", "-t", &view_session, "status", "off"])
                    .unwrap();
                for session in ["main", view_session.as_str()] {
                    tmux.raw_cmd(&["set-option", "-t", session, "window-size", policy])
                        .unwrap();
                }
                for window in [request.main_window_id.as_str(), view_window.as_str()] {
                    tmux.raw_cmd(&[
                        "set-window-option",
                        "-t",
                        window,
                        "aggressive-resize",
                        aggressive_resize,
                    ])
                    .unwrap();
                }

                let mut main_client = tmux.attach_control_mode(Some("main")).unwrap();
                main_client
                    .send_command("refresh-client -C 240x50")
                    .unwrap();
                let mut view_client = tmux.attach_control_mode(Some(&view_session)).unwrap();
                view_client
                    .send_command("refresh-client -C 100x30")
                    .unwrap();
                wait_for_window_size(&tmux, &request.main_window_id, (240, 50));
                wait_for_window_size(&tmux, &view_window, (100, 30));
                let main_before = snapshot_main(&tmux, &request).unwrap();

                view_client
                    .send_command(&format!("select-window -t {view_session}:view"))
                    .unwrap();
                view_client
                    .send_command(&format!("select-pane -t {}", request.pane_id))
                    .unwrap();
                view_client
                    .send_command("refresh-client -C 120x35")
                    .unwrap();
                wait_for_window_size(&tmux, &view_window, (120, 35));

                let main_after = snapshot_main(&tmux, &request).unwrap();
                assert_eq!(
                    serde_json::to_vec(&main_before).unwrap(),
                    serde_json::to_vec(&main_after).unwrap(),
                    "separate sessions must isolate policy={policy} aggressive-resize={aggressive_resize}",
                );
            }
        }
    }

    #[test]
    fn ordinary_cross_session_move_is_forbidden_but_lifecycle_bind_is_authorized() {
        let (tmux, request, _) = fixture("gh218-view-authorization");
        let foreign = tmux.new_session("foreign", Path::new("/tmp")).unwrap();
        let denied = PaneMoveOp::new(&tmux, &request.pane_id, &foreign).join("-dh");
        assert!(denied.is_err());
        assert!(
            denied
                .unwrap_err()
                .to_string()
                .contains("cross-session pane move denied")
        );
        let receipt = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        assert_eq!(receipt.pane.pane_id, request.pane_id);
    }

    #[test]
    fn pending_bind_recovers_after_pane_moved_before_placeholder_cleanup() {
        let (tmux, request, _) = fixture("gh218-view-recover-bind");
        let view_session = binding().view_session;
        let placeholder = tmux.new_session(&view_session, Path::new("/tmp")).unwrap();
        tmux.raw_cmd(&["rename-window", "-t", &placeholder, VIEW_WINDOW_NAME])
            .unwrap();
        PaneMoveOp::new(&tmux, &request.pane_id, &placeholder)
            .allow_cross_session(CROSS_SESSION_REASON)
            .join("-dh")
            .unwrap();
        assert_eq!(tmux.list_session_panes(&view_session).len(), 2);
        let receipt = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        assert_eq!(
            tmux.list_session_panes(&view_session),
            vec![request.pane_id.clone()]
        );
        assert!(matches!(
            receipt.settled_state,
            EditorViewBindingState::Bound { .. }
        ));
    }

    #[test]
    fn pending_bind_is_idempotent_after_the_view_is_fully_bound() {
        let (tmux, request, _) = fixture("gh218-view-retry-bound");
        let first = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        let first_placement = first.pane.clone();
        let retried = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        assert_eq!(retried.pane, first_placement);
        assert_eq!(
            tmux.list_session_panes(&binding().view_session),
            vec![request.pane_id]
        );
    }

    #[test]
    fn pending_release_settles_after_the_pane_move_was_already_applied() {
        let (tmux, mut request, baseline) = fixture("gh218-view-retry-release");
        let bound = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        request.state = EditorViewBindingState::ReleasePending {
            binding: binding(),
            pane: match bound.settled_state {
                EditorViewBindingState::Bound { pane, .. } => Some(pane),
                other => panic!("expected bound receipt, got {other:?}"),
            },
            reason: EditorViewReleaseReason::RecoveryCompensation,
            destination: EditorViewReleaseDestination::MainStash,
        };
        let stash_window = tmux.find_stash_window("main").unwrap();
        let stash_target = tmux.largest_pane_in_window(&stash_window).unwrap();
        PaneMoveOp::new(&tmux, &request.pane_id, &stash_target)
            .allow_cross_session(CROSS_SESSION_REASON)
            .join("-dv")
            .unwrap();
        assert!(!tmux.session_exists(&binding().view_session));

        let settled = reconcile_editor_view_tmux(&tmux, &request).unwrap();
        assert!(matches!(
            settled.settled_state,
            EditorViewBindingState::Released {
                reason: EditorViewReleaseReason::RecoveryCompensation,
                ..
            }
        ));
        assert_eq!(snapshot_main(&tmux, &request).unwrap(), baseline);
    }
}
