//! I/O observations for the typed pane-execution authority graph.

use std::path::Path;

use agent_doc_state_backbone::DocumentScope;
use agent_doc_state_backbone::pane_execution_authority::{
    AuthorityOperation, AuthorityVerdict, InvocationIdentity, OwnerLiveness, OwnerObservation,
    PaneExecutionAuthority,
};
use anyhow::{Context, Result};

fn owner_liveness(tmux: &tmux_router::Tmux, pane_id: &str, file: &Path) -> OwnerLiveness {
    if !tmux.pane_alive(pane_id) {
        return OwnerLiveness::Stale;
    }
    let Some(pane_pid) = agent_doc_tmux_io::pane_pid(tmux, pane_id) else {
        return OwnerLiveness::Indeterminate;
    };
    let pane_pid = pane_pid.to_string();
    if agent_doc_process_owner_io::process_tree_has_agent_doc_owner_for_file(
        &pane_pid,
        &file.to_string_lossy(),
    ) {
        OwnerLiveness::Live
    } else if agent_doc_supervisor_process::session_liveness::pane_owns_live_agent(tmux, pane_id) {
        OwnerLiveness::Indeterminate
    } else {
        OwnerLiveness::Stale
    }
}

fn pane_exactly_owns_document(tmux: &tmux_router::Tmux, pane_id: &str, file: &Path) -> bool {
    agent_doc_tmux_io::pane_pid(tmux, pane_id).is_some_and(|pane_pid| {
        agent_doc_process_owner_io::process_tree_has_agent_doc_owner_for_file(
            &pane_pid.to_string(),
            &file.to_string_lossy(),
        )
    })
}

fn observe_owner(file: &Path, tmux: &tmux_router::Tmux) -> Result<OwnerObservation> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    if let Some(project_root) = agent_doc_project_root_io::project_root_containing(&canonical) {
        let controller_socket = agent_doc_controller::paths::socket_path(&project_root);
        let actor = if controller_socket.exists() {
            agent_doc_controller_io::project_controller::authoritative_actor_binding(
                &project_root,
                &canonical,
            )?
        } else {
            // Actorless/bootstrap boundary only: no live controller graph exists,
            // so the durable actor projection is the cold observation source.
            let document_id = agent_doc_session_actor_io::canonical_document_id_in(
                &project_root,
                &canonical.to_string_lossy(),
            );
            agent_doc_session_actor_io::load_record_in(&project_root, &document_id)?
        };
        if let Some(actor) = actor
            && !matches!(actor.state, agent_doc_controller::actor::ActorState::Closed)
        {
            return Ok(OwnerObservation::Present {
                liveness: owner_liveness(tmux, &actor.pane_id, &canonical),
                pane_id: actor.pane_id,
                generation: Some(actor.generation),
            });
        }

        if let Ok(current) = agent_doc_document_realtime_io::try_resolve_current_document_content(
            &canonical,
            "pane_execution_authority",
        ) && let Ok((frontmatter, _)) = agent_doc_frontmatter::frontmatter::parse(&current)
            && let Some(session_id) = frontmatter.session
            && let Some(entry) =
                agent_doc_session_registry_io::lookup_entry_in(&project_root, &session_id)?
        {
            return Ok(OwnerObservation::Present {
                liveness: owner_liveness(tmux, &entry.pane, &canonical),
                pane_id: entry.pane,
                generation: None,
            });
        }
    }
    Ok(OwnerObservation::Absent)
}

/// Publish boundary observations into the caller's document scope and read the
/// one derived authority verdict. The tmux override used by controller effects
/// is intentionally honored by `current_pane_id_from_env_or_tmux`.
pub fn verdict_in(
    scope: &DocumentScope,
    file: &Path,
    operation: AuthorityOperation,
) -> Result<AuthorityVerdict> {
    let authority = PaneExecutionAuthority::new_in(scope);
    authority.observe_operation(operation);
    // A controller can be headless while executing on behalf of a tmux actor.
    // The thread-local override is explicit actor evidence and therefore takes
    // precedence over the controller process's missing ambient `TMUX` value.
    let tmux = agent_doc_tmux_io::configured_tmux();
    let explicit_invocation_pane =
        agent_doc_tmux_io::current_live_pane_id_from_env_or_override(&tmux);
    // One bounded read graph gives owner liveness and invocation proof the same
    // `/proc` cut. Splitting these observations across scopes can classify a
    // pane as stale and exact-owner from two different process generations.
    let _process_observations = agent_doc_process_owner_io::begin_process_observation_scope();
    let owner = observe_owner(file, &tmux)?;
    if !agent_doc_tmux_io::in_tmux() && explicit_invocation_pane.is_none() {
        authority.observe_owner(owner);
        authority.observe_invocation(InvocationIdentity::Headless);
        return Ok(authority.verdict());
    }

    let invocation = match explicit_invocation_pane
        .map(Ok)
        .unwrap_or_else(|| agent_doc_tmux_io::current_pane_id_from_env_or_tmux(&tmux))
    {
        Ok(pane_id) => {
            let proves_document_owner = pane_exactly_owns_document(&tmux, &pane_id, file);
            let generation = match &owner {
                OwnerObservation::Present {
                    pane_id: owner_pane,
                    generation,
                    ..
                } if owner_pane == &pane_id && proves_document_owner => *generation,
                _ => None,
            };
            InvocationIdentity::ExplicitActor {
                pane_id,
                generation,
                proves_document_owner,
            }
        }
        Err(_) => InvocationIdentity::Unavailable,
    };
    authority.observe_owner(owner);
    authority.observe_invocation(invocation);
    Ok(authority.verdict())
}

/// Gate a mutating command before it can acquire a lease, repair a cycle, or
/// persist a response. Live-owner mismatch deliberately carries no claim hint:
/// taking a live owner's session is never an admissible recovery.
pub fn require_in(scope: &DocumentScope, file: &Path) -> Result<AuthorityVerdict> {
    let verdict = verdict_in(scope, file, AuthorityOperation::Mutation)?;
    if verdict.permits() {
        return Ok(verdict);
    }
    // `#ownerremedyabsolutepath`: the remedy names the document by its ABSOLUTE
    // path. It used to say only "run the command in the owning pane", so whoever
    // relayed it supplied the path from the operator's own words — which is how a
    // relative `agent-doc tasks/infra.md` got recommended for a pane whose cwd
    // makes that path a DIFFERENT (here, nonexistent) file. Two panes with
    // different working directories cannot share a relative trigger, and the
    // owning pane is by definition not the pane the refusal was printed in.
    let document = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document = document.display();
    match &verdict {
        AuthorityVerdict::RejectLiveOwnerMismatch {
            owner_pane_id,
            invocation_pane_id,
        } => {
            if let Some(remedy) = recover_shared_codex_daemon_mismatch(
                file,
                &document.to_string(),
                owner_pane_id,
                invocation_pane_id,
            ) {
                anyhow::bail!("{remedy}");
            }
            anyhow::bail!(
                "{}",
                live_owner_mismatch_remedy(&document.to_string(), owner_pane_id, invocation_pane_id)
            )
        }
        AuthorityVerdict::RejectStaleOwner {
            owner_pane_id,
            invocation_pane_id,
        } => anyhow::bail!(
            "pane execution authority rejected before mutation: stale owner binding {owner_pane_id}, invocation pane {invocation_pane_id}. The owner was proven stale; repair or force-claim the session, then retry."
        ),
        AuthorityVerdict::RejectIndeterminateOwner {
            owner_pane_id,
            invocation_pane_id,
        } => anyhow::bail!(
            "pane execution authority is indeterminate before mutation: registered pane {owner_pane_id}, invocation pane {invocation_pane_id}. Inspect session ownership before retrying; no cycle or write state was changed."
        ),
        AuthorityVerdict::RejectGenerationMismatch {
            pane_id,
            invocation_generation,
            owner_generation,
        } => anyhow::bail!(
            "pane execution authority generation mismatch before mutation: pane {pane_id}, invocation generation {invocation_generation:?}, owner generation {owner_generation:?}. No cycle or write state was changed."
        ),
        AuthorityVerdict::RejectUnavailableInvocation => anyhow::bail!(
            "pane execution authority could not identify the invoking pane before mutation; no cycle or write state was changed"
        ),
        permitted => Err(anyhow::anyhow!(
            "internal pane execution authority error: non-rejecting verdict {permitted:?}"
        )),
    }
    .with_context(|| format!("pane authority for {}", file.display()))
}

/// Recover a live-owner mismatch that is an artifact of Codex's shared daemon.
///
/// Both proofs are required before acting: this command must descend from a
/// shared Codex app-server daemon (so `TMUX_PANE` is the daemon's pane, not the
/// invoker's), and the owner pane must be running a daemon-attached Codex TUI.
/// The recovery is the operator's own "Restart Agent" intent, which the owner's
/// live supervisor defers to its turn boundary and completes as an exact resume
/// — so the conversation survives and the relaunch carries `--no-daemon`. If
/// the invocation really came from a foreign pane, the only effect is that the
/// owner is moved onto an unshared app-server, which is the safe configuration.
fn recover_shared_codex_daemon_mismatch(
    file: &Path,
    document: &str,
    owner_pane_id: &str,
    invocation_pane_id: &str,
) -> Option<String> {
    use crate::codex_shared_daemon as shared;
    let daemon_pid = shared::invocation_descends_from_shared_codex_daemon()?;
    let tmux = agent_doc_tmux_io::configured_tmux();
    let pane_pid: u32 =
        agent_doc_tmux_io::display_message_value_nonempty(&tmux, Some(owner_pane_id), "#{pane_pid}")?
            .trim()
            .parse()
            .ok()?;
    shared::daemon_attached_codex_tui_under(pane_pid)?;
    let requested = agent_doc_project_root_io::project_root_containing(file)
        .ok_or_else(|| "no project root".to_string())
        .and_then(|root| {
            agent_doc_controller_io::project_controller::request_supervisor_replacement(
                &root,
                agent_doc_controller_io::project_controller::SupervisorReplacementRequest {
                    file: file.canonicalize().unwrap_or_else(|_| file.to_path_buf()),
                    mode: "agent:continue".to_string(),
                    force: false,
                },
            )
            .map(|_| ())
            .map_err(|err| format!("{err:#}"))
        });
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "pane_authority_shared_codex_daemon owner_pane={owner_pane_id} reported_pane={invocation_pane_id} daemon_pid={daemon_pid} relaunch_requested={}",
            requested.is_ok()
        ),
    );
    Some(shared::shared_daemon_owner_mismatch_remedy(
        document,
        owner_pane_id,
        invocation_pane_id,
        daemon_pid,
        requested,
    ))
}

/// The live-owner-mismatch refusal, as one operator-facing sentence.
///
/// `#ownerremedyabsolutepath`: extracted so the remedy's one load-bearing
/// property — that it names the document absolutely — is assertable without a
/// live tmux server, two panes, and a registered owner.
pub fn live_owner_mismatch_remedy(
    document: &str,
    owner_pane_id: &str,
    invocation_pane_id: &str,
) -> String {
    format!(
        "pane execution authority rejected before mutation: live owner pane {owner_pane_id}, \
         invocation pane {invocation_pane_id}. Run `agent-doc {document}` in pane \
         {owner_pane_id} — use that absolute path, since a relative one resolves against each \
         pane's own working directory. This command did not open or repair a cycle."
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn headless_invocation_still_observes_the_registered_owner() {
        let source = include_str!("pane_execution_authority.rs");
        let owner_observation = source
            .find("let owner = observe_owner(file, &tmux)?;")
            .expect("owner observation must exist");
        let headless_branch = source
            .find("if !agent_doc_tmux_io::in_tmux() && explicit_invocation_pane.is_none()")
            .expect("headless branch must exist");
        assert!(
            owner_observation < headless_branch,
            "headless admission must not classify the owner as absent before observing it"
        );
        let forged_absence = ["authority.observe_owner(OwnerObservation::", "Absent)"].concat();
        assert!(
            !source.contains(&forged_absence),
            "the adapter must not forge owner absence for a headless invocation"
        );
    }

    /// `#ownerremedyabsolutepath`: operator-reported 2026-09-28 on
    /// `src/haiven-dev/tasks/infra.md`. The refusal named no document, so the
    /// agent relaying it supplied the path from the operator's own words and
    /// recommended `agent-doc tasks/infra.md` — which, from the refusing pane's
    /// working directory (the superproject), names a file that does not exist,
    /// and from the owning pane's (the submodule) names the real one. A remedy
    /// addressed to a pane with a different working directory cannot use a
    /// relative path.
    #[test]
    fn the_owner_mismatch_remedy_names_the_document_absolutely() {
        let remedy = super::live_owner_mismatch_remedy(
            "/home/u/work/agent-loop/src/haiven-dev/tasks/infra.md",
            "%214",
            "%231",
        );
        assert!(
            remedy.contains("`agent-doc /home/u/work/agent-loop/src/haiven-dev/tasks/infra.md`"),
            "the remedy must carry the runnable absolute command: {remedy}"
        );
        assert!(
            remedy.contains("%214") && remedy.contains("%231"),
            "both panes stay named so the operator can tell them apart: {remedy}"
        );
        assert!(
            !remedy.contains("`agent-doc tasks/"),
            "no relative form may appear — it is the exact ambiguity this fixes: {remedy}"
        );
        assert!(
            !remedy.contains("claim"),
            "taking a live owner's session is never offered as a recovery: {remedy}"
        );
    }
}
