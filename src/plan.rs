//! # Module: plan
//!
//! ## Spec
//! - `run(file)`: derives a structured planning/dispatch record for the current
//!   session document and prints it as pretty JSON.
//! - Reads the current document and computes the current diff against the saved
//!   snapshot via `diff::compute(file)`.
//! - Produces an ordered record with prompt targets, execution scope, repo
//!   actions, required binary commands, pending mutations that must be
//!   resolved this cycle, a handoff target, concrete blockers, and a compact
//!   selected-path operation tree.
//! - Does not fail closed solely on session-accretion heuristics; accretion
//!   remains advisory while planning still derives prompt targets and actions.
//! - Downgrades tsift graph evidence collection failures to `manual_packet_only`
//!   warnings so agent-doc turns and job packet creation can continue without
//!   graph acceptance evidence.
//! - Uses the same deterministic diff classifiers as preflight (`prompt_bearing_changes`,
//!   imperative-directive extraction, slash-command parsing, orchestration detection)
//!   so the planning record is binary-owned rather than free-form skill prose.
//! - `build(file)`: pure planning entry point for tests/callers after the file
//!   read + diff computation; returns the structured plan instead of printing.
//! - Connected harnesses receive queue-subagent dispatch and unbound-claim
//!   recovery in this same record. A lightweight MCP admission therefore cannot
//!   hide the queue lifecycle contract that hook-driven preflight normally emits.
//!
//! ## Agentic Contracts
//! - The plan record is deterministic for a given document + snapshot pair.
//! - `operation_tree` is binary-owned and selected-path-only: it names the
//!   blocker decision and ordered executable phases without inviting the
//!   harness agent to invent or narrate unselected alternatives.
//! - `handoff=orchestrate` means the skill should execute the emitted
//!   `agent-doc orchestrate ...` command before attempting a manual response.
//! - `pending_mutations` captures pre-response pending work that must be
//!   explicitly resolved before persistence; it does not silently complete items.
//! - `execution_scope=plan_backlog_only` means the prompt contract is a
//!   report/planning turn such as `#agent-doc-bug`, so repo work must wait
//!   for a later explicit implementation directive.
//! - `required_commands` may include placeholder arguments such as
//!   state.db document baseline because the planning phase does not own the
//!   preflight baseline path.
//!
//! ## Evals
//! - `build_plan_detects_orchestration_handoff_and_existing_pending_item`
//! - `build_plan_includes_finalize_placeholder_for_template_docs`
//! - `test_plan_detects_backlog_request`
//! - `test_plan_detects_recommendation_request`
//! - `test_plan_no_false_positive_on_questions`

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use agent_doc_element::element::{self, is_backlog_component, is_tracked_work_component};
use agent_doc_element_backlog::backlog;
use agent_doc_prompt_context::loaded_context::{
    LoadedContextLedger, build_loaded_context_ledger, loaded_context_record,
};

use agent_doc_diff as diff;
use agent_doc_frontmatter::frontmatter;
use agent_doc_workflow::session_cycle::{
    FinalizePendingMutation, FinalizePendingMutationKind, SessionExecutionScope,
    classify_execution_scope, finalize_command, prompt_targets_from_changes,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DispatchPlan {
    /// Unclaimed queue heads that this cycle must dispatch to background
    /// workers. Connected harnesses consume this projection because their
    /// lightweight admission does not emit the hook preflight contract.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_subagent_dispatch: Vec<agent_doc_preflight_io::QueueSubagentDispatch>,
    /// How to execute [`Self::queue_subagent_dispatch`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_subagent_dispatch_guidance: Option<String>,
    /// Claims that still need the successful spawn's worker id, or must be
    /// released for redispatch when no worker was spawned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue_dispatch_acknowledgement_required:
        Vec<agent_doc_queue_io::queue_claim::DispatchAcknowledgementObligation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompt_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_evidence: Option<crate::tsift_graph::TsiftGraphEvidencePlan>,
    pub dispatch_candidate: bool,
    pub task_class: String,
    pub risk: String,
    pub parallelizable: bool,
    pub suggested_parent_tier: String,
    pub model_tier: String,
    pub dispatch_mode: String,
    pub manual_packet_only: bool,
    pub context_budget_tokens: usize,
    pub job_packet_budget_tokens: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_proof: Vec<String>,
    pub tsift_context: TsiftContextPlan,
    pub execution_scope: ExecutionScope,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repo_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_mutations: Vec<PendingMutationPlan>,
    pub handoff: HandoffTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blockers: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Binary-owned execution order for this plan. The tree contains only the
    /// selected path through each decision so harness agents do not spend
    /// context proposing speculative alternatives to the same contract.
    pub operation_tree: OperationTree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationTree {
    pub policy: String,
    pub root: OperationNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationNode {
    pub id: String,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<OperationCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OperationNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationCondition {
    pub expression: String,
    pub result: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PendingMutationPlan {
    pub kind: PendingMutationKind,
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TsiftContextPlan {
    pub status: String,
    pub freshness_policy: String,
    pub index_status_command: String,
    pub context_pack_command: String,
    pub source_read_command: String,
    pub diff_digest_command: String,
    pub test_digest_command: String,
    pub stale_fallback: String,
    pub loaded_context_ledger: LoadedContextLedger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_manifest: Option<agent_doc_dynamic_context_io::DynamicContextSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingMutationKind {
    ResolveExisting,
    ExpectAdd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffTarget {
    None,
    Orchestrate,
    Compact,
    Claim,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionScope {
    Normal,
    PlanBacklogOnly,
}

pub fn run(file: &Path) -> Result<()> {
    let plan = build(file)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&plan).context("failed to serialize dispatch plan")?
    );
    Ok(())
}

pub fn build(file: &Path) -> Result<DispatchPlan> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "plan_command_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;
    let (fm, _body) = agent_doc_frontmatter_io::session::parse_for_file(&content, file)
        .with_context(|| format!("failed to parse frontmatter in {}", file.display()))?;
    let effective_prompt_presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_file(&content, file)
            .with_context(|| format!("failed to resolve prompt presets for {}", file.display()))?;

    // `#connectedqueuedispatch`: hook-driven entrypoints receive these fields
    // in preflight output. Grok's connected path deliberately uses lightweight
    // `agent_doc_admit` followed by this plan, so plan must project the same
    // queue state or a subagents queue silently becomes inline work. A claim is
    // actionable immediately at this explicit planning boundary: either bind
    // the worker returned by the successful spawn or release it for redispatch.
    let pending_queue_subagent_dispatch =
        agent_doc_queue_io::subagent_dispatch::pending_subagent_dispatch_for_content(
            file, &content,
        )?;
    let queue_subagent_dispatch = agent_doc_preflight_io::queue_subagent_dispatch_entries(
        file,
        &content,
        &pending_queue_subagent_dispatch,
    )?;
    let queue_dispatch_acknowledgement_required =
        agent_doc_queue_io::queue_claim::dispatch_acknowledgement_obligations_for_content_at(
            file,
            &content,
            agent_doc_queue_io::queue_claim::claim_clock_now_secs(),
            0,
        )?;
    let queue_subagent_dispatch_guidance = (!queue_subagent_dispatch.is_empty())
        .then(|| agent_doc_preflight_io::QUEUE_SUBAGENT_DISPATCH_GUIDANCE.to_string());
    let queue_lifecycle_required_commands = queue_lifecycle_required_commands(
        &queue_subagent_dispatch,
        &queue_dispatch_acknowledgement_required,
    );

    let doc_diff = agent_doc_diff_io::compute(
        &agent_doc_snapshot_io::DiffBaselineStore::new(agent_doc_ops_log_io::log_op),
        file,
    )?;
    let harness_diff = if doc_diff.is_none() {
        agent_doc_harness::prompt_source::synthetic_diff_for_file(
            file,
            agent_doc_codex_hook_io::load_prompt_for_current_session,
        )?
    } else {
        None
    };
    // `#planhead`: plan must NOT derive its own "which head is next". SKILL.md
    // step 0d tells the agent plan's `prompt_targets` / `repo_actions` /
    // `required_commands` ARE the execution contract, so a second derivation that
    // disagrees with preflight's `selected_queue_prompts` makes the agent work the
    // wrong item -- observed three times naming an `[operator-verify]` head that
    // preflight had already deferred. Defer to the same in-session drainable-head
    // selection preflight uses; when no head is drainable in this scope, plan
    // reports no queue-driven work rather than inventing one.
    let (queue_prompt, submit_claims) = if doc_diff.is_none() && harness_diff.is_none() {
        // Select with owner claims only. This cycle's dispatch heads are
        // suppressed inside that queue; adding them to the selection set would
        // pause it and inline the next `agent:run` in the same plan.
        let owner_selection =
            agent_doc_queue_io::subagent_dispatch::owner_selection_claims_for_content(
                file, &content,
            );
        let mut suppressed = owner_selection.clone();
        if let Some(queue_subagent_plan) =
            agent_doc_queue_io::subagent_dispatch::queue_attr_subagent_plan(file, &content)
        {
            suppressed = suppressed
                .with_heads(&pending_queue_subagent_dispatch)
                .with_heads(&queue_subagent_plan.held);
        }
        let prompt = agent_doc_queue::queue_continuation::dispatchable_head_prompt_text_for_owner(
            &content,
            agent_doc_queue::queue_continuation::DrainScope::InSessionLoop,
            &owner_selection,
            &suppressed,
            &effective_prompt_presets,
        );
        (prompt, suppressed)
    } else {
        (
            None,
            agent_doc_queue::queue_claim::ClaimedQueueItems::none(),
        )
    };
    let queue_head_slash_command = queue_prompt
        .as_deref()
        .and_then(agent_doc_queue::queue_command::slash_command_text);
    // `#plansealedturn` (GH #68 §1): the binary's preflight already sealed this
    // turn and consumed the diff baseline, so a re-diff here legitimately finds
    // nothing. That is NOT "no changes" -- the turn's prompt lives in the sealed
    // cycle contract. Recover the free-text queue head preflight selected, and
    // otherwise report no blocker (SKILL.md step 0d stops on blockers, so a
    // false one aborts a legitimate cycle).
    let sealed_turn = if doc_diff.is_none() && harness_diff.is_none() && queue_prompt.is_none() {
        sealed_preflight_turn(file)
    } else {
        None
    };
    let queue_prompt = queue_prompt.or_else(|| {
        sealed_turn
            .as_ref()
            .and_then(|cycle| cycle.selected_free_text_queue_heads.first().cloned())
    });
    // `#nestsubmit`: the leaf stays `queue_prompt` (slash detection and the
    // orchestrate source). The diff the agent receives carries parent lineage
    // and the entire prompt tree.
    let submitted_queue_prompt = queue_prompt.as_ref().map(|prompt| {
        if queue_head_slash_command.is_some() {
            prompt.clone()
        } else {
            agent_doc_queue::queue_response::submitted_continuation_prompt(
                &content,
                prompt,
                |row| submit_claims.claims(row),
            )
        }
    });
    let queue_diff = submitted_queue_prompt
        .as_deref()
        .map(|prompt| diff::synthetic_added_lines_diff(prompt, "queue"));

    // `#chatprompt` (GH #125): chat prompts the admitted cycle carries are this
    // turn's targets even when the document diff is empty; without them plan
    // reported `no_changes` / an empty `prompt_targets` for a real turn.
    let chat_prompts = open_cycle_chat_prompts(file);
    let chat_diff = (!chat_prompts.is_empty())
        .then(|| diff::synthetic_added_lines_diff(&chat_prompts.join("\n"), "harness"));
    let Some(diff_text) = doc_diff
        .or(harness_diff.clone())
        .or(queue_diff)
        .or(chat_diff)
    else {
        let has_queue_lifecycle_work = !queue_lifecycle_required_commands.is_empty();
        let (task_class, blockers, warnings) = if has_queue_lifecycle_work {
            (
                "queue_subagent_dispatch".to_string(),
                Vec::new(),
                Vec::new(),
            )
        } else {
            match sealed_turn.as_ref() {
                Some(cycle) => (
                    "preflight_sealed".to_string(),
                    Vec::new(),
                    vec![sealed_turn_warning(&cycle.cycle_id)],
                ),
                None => (
                    "no_changes".to_string(),
                    vec!["No changes detected since the last snapshot.".to_string()],
                    Vec::new(),
                ),
            }
        };
        let mut required_commands = queue_lifecycle_required_commands;
        required_commands.extend(finalize_placeholder_commands(file, &fm, &[]));
        let parallelizable = queue_subagent_dispatch.len() > 1;
        let blockers_for_tree = blockers.clone();
        let operation_tree = build_operation_tree(
            &[],
            &[],
            &required_commands,
            &[],
            HandoffTarget::None,
            &blockers_for_tree,
        );
        return Ok(DispatchPlan {
            queue_subagent_dispatch,
            queue_subagent_dispatch_guidance,
            queue_dispatch_acknowledgement_required,
            prompt_targets: Vec::new(),
            graph_evidence: None,
            dispatch_candidate: has_queue_lifecycle_work,
            task_class,
            risk: if has_queue_lifecycle_work {
                "medium".to_string()
            } else {
                "low".to_string()
            },
            parallelizable,
            suggested_parent_tier: "low".to_string(),
            model_tier: "low".to_string(),
            dispatch_mode: dispatch_mode(&fm),
            manual_packet_only: false,
            context_budget_tokens: if has_queue_lifecycle_work { 6000 } else { 0 },
            job_packet_budget_tokens: if has_queue_lifecycle_work { 4000 } else { 0 },
            write_scope: Vec::new(),
            required_proof: if has_queue_lifecycle_work {
                vec!["queue_worker_binding".to_string()]
            } else {
                Vec::new()
            },
            tsift_context: tsift_context_plan(file, &content, &[]),
            execution_scope: ExecutionScope::Normal,
            repo_actions: Vec::new(),
            required_commands,
            pending_mutations: Vec::new(),
            handoff: HandoffTarget::None,
            blockers,
            warnings,
            operation_tree,
        });
    };

    let queue_active_for_prompt_extraction =
        agent_doc_queue::queue_heads::queue_is_active_for_diff(&content, &diff_text);
    let command_diff_text = if queue_active_for_prompt_extraction {
        diff_text.clone()
    } else {
        diff::suppress_inactive_queue_additions(&diff_text, &content)
    };
    let prompt_diff_text = if queue_head_slash_command.is_some() {
        String::new()
    } else {
        command_diff_text.clone()
    };

    let prompt_bearing_changes = diff::classify_prompt_bearing_changes(&prompt_diff_text);
    let mut prompt_targets = prompt_targets_from_changes(&prompt_bearing_changes);
    // `#nestsubmit`: a nested leaf's lineage plus its prompt tree is the prompt
    // even when the classifier does not recognize `parent: #id` as an
    // imperative or question. Top-level submission equals the leaf, so this
    // adds nothing there.
    if let Some(submitted) = submitted_queue_prompt.as_deref().filter(|submitted| {
        queue_head_slash_command.is_none() && queue_prompt.as_deref() != Some(*submitted)
    }) {
        agent_doc_prompt_contract::push_unique_strings(
            &mut prompt_targets,
            [submitted.to_string()],
        );
    }
    agent_doc_prompt_contract::push_unique_strings(&mut prompt_targets, chat_prompts.clone());
    let added_diff_lines = agent_doc_prompt_contract::collect_added_diff_lines(&prompt_diff_text);

    let execution_scope = execution_scope_for_prompt_targets(
        &prompt_targets,
        &added_diff_lines,
        harness_diff.is_some(),
        &effective_prompt_presets,
    );
    let repo_actions = if execution_scope == ExecutionScope::PlanBacklogOnly {
        Vec::new()
    } else {
        diff::extract_imperative_directives(&prompt_diff_text)
    };
    let orchestration_request = diff::detect_orchestration_request(&prompt_diff_text);
    let exchange_compaction_requested = diff::detect_exchange_compaction_request(&prompt_diff_text);
    let parsed_commands = diff::parse_slash_commands_classified(&command_diff_text);
    let pending_mutations = pending_mutations_for_doc(
        file,
        &content,
        &repo_actions,
        &prompt_targets,
        &added_diff_lines,
        &prompt_bearing_changes,
        &effective_prompt_presets,
    )?;
    let mut routing = routing_fields(
        file,
        &content,
        execution_scope,
        &repo_actions,
        &prompt_targets,
        &pending_mutations,
        &fm,
    );
    if !queue_lifecycle_required_commands.is_empty() {
        routing.dispatch_candidate = true;
        routing.parallelizable |= queue_subagent_dispatch.len() > 1;
        routing.context_budget_tokens = routing.context_budget_tokens.max(6000);
        routing.job_packet_budget_tokens = routing.job_packet_budget_tokens.max(4000);
        agent_doc_prompt_contract::push_unique_strings(
            &mut routing.required_proof,
            ["queue_worker_binding".to_string()],
        );
        if prompt_targets.is_empty() && repo_actions.is_empty() && pending_mutations.is_empty() {
            routing.task_class = "queue_subagent_dispatch".to_string();
            routing.risk = "medium".to_string();
        }
    }
    let release_publish_requested = execution_scope == ExecutionScope::Normal
        && prompt_targets
            .iter()
            .chain(repo_actions.iter())
            .any(|text| explicitly_requests_release_publish(text));
    if release_publish_requested {
        agent_doc_prompt_contract::push_unique_strings(
            &mut routing.required_proof,
            ["github_release".to_string(), "pypi_publication".to_string()],
        );
    }
    let mut blockers = shared_doc_security_blockers(file, &fm, &pending_mutations);
    let mut warnings = Vec::new();
    if let Some(deferral) = plan_backlog_only_deferral_warning(execution_scope, &prompt_diff_text) {
        warnings.push(deferral);
    }
    match agent_doc_memory_io::session::semantic_completion_matches(file, None, 5) {
        Ok(matches) => {
            warnings.extend(
                matches
                    .iter()
                    .map(agent_doc_memory::format_semantic_completion_warning),
            );
        }
        Err(err) => warnings.push(format!("semantic completion retrieval unavailable: {err}")),
    }
    let mut manual_packet_only = false;
    let graph_targets = prompt_targets
        .iter()
        .chain(repo_actions.iter())
        .cloned()
        .collect::<Vec<_>>();
    let graph_evidence = match crate::tsift_graph::collect_for_do_items(file, &graph_targets) {
        Ok(graph_evidence) => graph_evidence,
        Err(err) => {
            manual_packet_only = true;
            warnings.push(crate::tsift_graph::manual_packet_fallback_warning(&err));
            None
        }
    };

    let mut required_commands = queue_lifecycle_required_commands;
    let mut handoff = HandoffTarget::None;

    // `#releasepublishcontract`: a branch push, local install, or successful tag
    // push is not the publication the operator requested.  Put the irreversible
    // command and its public proof in the deterministic plan so the harness
    // cannot silently reinterpret "release + publish" as ordinary closeout.
    // `make release` owns the wait for both GitHub Release and PyPI visibility.
    if release_publish_requested {
        required_commands.push(
            "After integrating and versioning the release batch, run `make release`. Do not finalize until it exits successfully: it must verify the tag's green Release workflow, complete GitHub Release assets, green PyPI workflow, and PyPI simple-index wheel."
                .to_string(),
        );
    }

    if exchange_compaction_requested {
        required_commands.push(format!(
            "Run `agent-doc compact {} --commit` before any free-form response.",
            file.display()
        ));
        handoff = HandoffTarget::Compact;
    }

    // `#noparallelfanoutwhennotparallelizable`: one contract must not order what
    // it has itself judged impossible. `required_commands` is mandatory for the
    // skill, so a `--mode parallel` handoff emitted next to
    // `parallelizable: false` (this turn has a single repo action, so there is no
    // batch to split) is an instruction to fan out nothing. Observed cost: a
    // pasted status report resolved as `Parallel`, plan ordered the fan-out, and
    // `orchestrate` created three git worktrees and three `claude -p` panes, one
    // per quoted diff line. When the plan sees no parallel batch, it emits no
    // parallel dispatch — the turn falls through to ordinary response + finalize.
    let orchestration_request = orchestration_request.filter(|request| {
        let refuse = request.mode == agent_doc_diff::OrchestrationRequestMode::Parallel
            && !routing.parallelizable;
        if refuse {
            warnings.push(format!(
                "declined a `--mode parallel` orchestration handoff: this plan resolved {} repo action(s) and parallelizable=false, so there is no batch to fan out. Trigger text: {:?}",
                repo_actions.len(),
                request.trigger_text.chars().take(160).collect::<String>(),
            ));
        }
        !refuse
    });

    if let Some(request) = orchestration_request {
        let task_source = if queue_prompt.is_some() {
            "--from-queue"
        } else {
            "--from-exchange"
        };
        required_commands.push(format!(
            "agent-doc orchestrate {} --mode {} {}",
            file.display(),
            orchestration_mode_arg(request.mode),
            task_source,
        ));
        handoff = HandoffTarget::Orchestrate;
    }

    for command in parsed_commands.builtin_commands {
        match command.as_str() {
            "/compact" => {
                handoff = HandoffTarget::Compact;
                required_commands.push(format!(
                    "Tell the user to run `{}` at the terminal before continuing.",
                    command
                ));
            }
            _ => {
                required_commands.push(format!(
                    "Tell the user to run `{}` at the terminal before continuing.",
                    command
                ));
                if matches!(handoff, HandoffTarget::None) {
                    handoff = HandoffTarget::Other;
                }
            }
        }
    }

    for command in parsed_commands.skill_commands {
        required_commands.push(format!(
            "Dispatch slash command before free-form reply: {}",
            command
        ));
        if matches!(handoff, HandoffTarget::None) {
            handoff = HandoffTarget::Other;
        }
    }

    if !exchange_compaction_requested && matches!(handoff, HandoffTarget::None) {
        let speculative_done = speculative_done_ids(&pending_mutations);
        if speculative_done.len() > MAX_SPECULATIVE_DONE_IDS {
            warnings.push(mass_done_suppression_warning(&speculative_done));
        }
        required_commands.extend(finalize_placeholder_commands(file, &fm, &pending_mutations));
    }

    let operation_tree = build_operation_tree(
        &prompt_targets,
        &repo_actions,
        &required_commands,
        &pending_mutations,
        handoff,
        &blockers,
    );

    Ok(DispatchPlan {
        queue_subagent_dispatch,
        queue_subagent_dispatch_guidance,
        queue_dispatch_acknowledgement_required,
        prompt_targets,
        graph_evidence,
        dispatch_candidate: routing.dispatch_candidate,
        task_class: routing.task_class,
        risk: routing.risk,
        parallelizable: routing.parallelizable,
        suggested_parent_tier: routing.suggested_parent_tier.clone(),
        model_tier: routing.model_tier,
        dispatch_mode: routing.dispatch_mode,
        manual_packet_only,
        context_budget_tokens: routing.context_budget_tokens,
        job_packet_budget_tokens: routing.job_packet_budget_tokens,
        write_scope: routing.write_scope,
        required_proof: routing.required_proof,
        tsift_context: routing.tsift_context,
        execution_scope,
        repo_actions,
        required_commands,
        pending_mutations,
        handoff,
        blockers: std::mem::take(&mut blockers),
        warnings,
        operation_tree,
    })
}

fn build_operation_tree(
    prompt_targets: &[String],
    repo_actions: &[String],
    required_commands: &[String],
    pending_mutations: &[PendingMutationPlan],
    handoff: HandoffTarget,
    blockers: &[String],
) -> OperationTree {
    let gate_passed = blockers.is_empty();
    let selected_branch = if gate_passed {
        OperationNode {
            id: "path.execute".to_string(),
            operation: "sequence".to_string(),
            detail: Some(
                "Execute the binary-selected phases in order; do not expand alternative paths."
                    .to_string(),
            ),
            condition: None,
            children: selected_operation_phases(
                prompt_targets,
                repo_actions,
                required_commands,
                pending_mutations,
                handoff,
            ),
        }
    } else {
        OperationNode {
            id: "path.stop".to_string(),
            operation: "stop".to_string(),
            detail: Some(blockers.join(" | ")),
            condition: None,
            children: Vec::new(),
        }
    };

    OperationTree {
        policy: "selected_path_only".to_string(),
        root: OperationNode {
            id: "cycle".to_string(),
            operation: "evaluate_cycle".to_string(),
            detail: None,
            condition: None,
            children: vec![OperationNode {
                id: "gate.blockers".to_string(),
                operation: "evaluate".to_string(),
                detail: None,
                condition: Some(OperationCondition {
                    expression: "blockers.is_empty()".to_string(),
                    result: gate_passed,
                }),
                children: vec![selected_branch],
            }],
        },
    }
}

fn queue_lifecycle_required_commands(
    dispatch: &[agent_doc_preflight_io::QueueSubagentDispatch],
    acknowledgements: &[agent_doc_queue_io::queue_claim::DispatchAcknowledgementObligation],
) -> Vec<String> {
    let mut commands = dispatch
        .iter()
        .map(|entry| {
            format!(
                "Dispatch queue item {:?} now: run `{}`; launch one native background subagent \
                 with `authorization.subagent_prompt_preamble` verbatim; after spawn returns its \
                 worker id, run `{}` with that id and the claim's printed generation.",
                entry.item, entry.claim_command, entry.start_command,
            )
        })
        .collect::<Vec<_>>();
    commands.extend(
        acknowledgements
            .iter()
            .map(|obligation| obligation.message()),
    );
    commands
}

fn selected_operation_phases(
    prompt_targets: &[String],
    repo_actions: &[String],
    required_commands: &[String],
    pending_mutations: &[PendingMutationPlan],
    handoff: HandoffTarget,
) -> Vec<OperationNode> {
    let mut phases = Vec::new();
    push_operation_phase(
        &mut phases,
        "prompts",
        "select_prompt_target",
        prompt_targets,
    );

    if !matches!(handoff, HandoffTarget::None) {
        phases.push(OperationNode {
            id: "phase.handoff".to_string(),
            operation: "perform_handoff".to_string(),
            detail: Some(format!("{handoff:?}").to_ascii_lowercase()),
            condition: None,
            children: required_commands
                .iter()
                .enumerate()
                .map(|(index, detail)| OperationNode {
                    id: format!("required_commands.{index}"),
                    operation: "run_required_command".to_string(),
                    detail: Some(detail.clone()),
                    condition: None,
                    children: Vec::new(),
                })
                .collect(),
        });
        return phases;
    }

    push_operation_phase(
        &mut phases,
        "repo_actions",
        "execute_repo_action",
        repo_actions,
    );

    if !pending_mutations.is_empty() {
        phases.push(OperationNode {
            id: "phase.pending_mutations".to_string(),
            operation: "resolve_pending_mutations".to_string(),
            detail: None,
            condition: None,
            children: pending_mutations
                .iter()
                .enumerate()
                .map(|(index, mutation)| OperationNode {
                    id: format!("pending_mutations.{index}"),
                    operation: match mutation.kind {
                        PendingMutationKind::ResolveExisting => "resolve_existing",
                        PendingMutationKind::ExpectAdd => "expect_add",
                    }
                    .to_string(),
                    detail: Some(format!("{}: {}", mutation.id, mutation.text)),
                    condition: None,
                    children: Vec::new(),
                })
                .collect(),
        });
    }

    push_operation_phase(
        &mut phases,
        "required_commands",
        "run_required_command",
        required_commands,
    );
    phases
}

fn push_operation_phase(
    phases: &mut Vec<OperationNode>,
    phase_id: &str,
    leaf_operation: &str,
    details: &[String],
) {
    if details.is_empty() {
        return;
    }
    phases.push(OperationNode {
        id: format!("phase.{phase_id}"),
        operation: leaf_operation.to_string(),
        detail: None,
        condition: None,
        children: details
            .iter()
            .enumerate()
            .map(|(index, detail)| OperationNode {
                id: format!("{phase_id}.{index}"),
                operation: leaf_operation.to_string(),
                detail: Some(detail.clone()),
                condition: None,
                children: Vec::new(),
            })
            .collect(),
    });
}

fn orchestration_mode_arg(mode: diff::OrchestrationRequestMode) -> &'static str {
    match mode {
        diff::OrchestrationRequestMode::Sequential => "sequential",
        diff::OrchestrationRequestMode::Parallel => "parallel",
        diff::OrchestrationRequestMode::Dag => "dag",
    }
}

fn execution_scope_for_prompt_targets(
    prompt_targets: &[String],
    added_diff_lines: &[String],
    harness_prompt_only: bool,
    prompt_presets: &indexmap::IndexMap<String, String>,
) -> ExecutionScope {
    match classify_execution_scope(
        prompt_targets,
        added_diff_lines,
        harness_prompt_only,
        prompt_presets,
    ) {
        SessionExecutionScope::PlanBacklogOnly => ExecutionScope::PlanBacklogOnly,
        SessionExecutionScope::Normal => ExecutionScope::Normal,
    }
}

/// `#lr-queue-response-miss` (step 2): when a `#agent-doc-bug` capture forces
/// `execution_scope=plan_backlog_only` but the same prompt diff also carries
/// runnable imperative directives (a `do [#id]` head, `build + push`, etc.),
/// surface the deferral explicitly so the actionable work is not silently
/// suppressed. Returns `None` for the pure bug-capture case (no imperative
/// directive) so normal plan-only turns stay quiet.
fn plan_backlog_only_deferral_warning(
    execution_scope: ExecutionScope,
    prompt_diff_text: &str,
) -> Option<String> {
    if execution_scope != ExecutionScope::PlanBacklogOnly {
        return None;
    }
    let deferred = diff::extract_imperative_directives(prompt_diff_text);
    if deferred.is_empty() {
        return None;
    }
    Some(format!(
        "execution_scope=plan_backlog_only suppressed runnable directive(s) this cycle to capture the #agent-doc-bug plan/backlog: [{}]. The deferral is explicit — run the directive(s) in a later cycle.",
        deferred.join("; ")
    ))
}

/// Above this many `ResolveExisting` ids, a pre-filled `--done` list stops being
/// an ergonomic placeholder and becomes a footgun (`#planmassdone`).
///
/// A turn realistically completes one to a few queued items. A larger set only
/// appears when a backlog→queue mirror (`#mirrorall`) puts every open tracked-work
/// head in the queue at once, and `plan` cannot distinguish "the operator asked me
/// to do these" from "these were mirrored in bulk". `SKILL.md` tells agents to
/// treat `required_commands` as the execution contract, so emitting one `--done`
/// per mirrored head proposes closing the entire backlog — including
/// `[focused-cycle]`, `[future/separate]`, and `[operator-verify]` items the turn
/// never touched. Observed live at 26 ids, of which only a handful were actually
/// complete.
const MAX_SPECULATIVE_DONE_IDS: usize = 3;

fn speculative_done_ids(pending_mutations: &[PendingMutationPlan]) -> Vec<&str> {
    pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == PendingMutationKind::ResolveExisting)
        .map(|mutation| mutation.id.as_str())
        .collect()
}

/// Warning emitted when the speculative `--done` list is suppressed, so the
/// suppression is visible rather than looking like plan simply found no work.
fn mass_done_suppression_warning(ids: &[&str]) -> String {
    format!(
        "suppressed {} pre-filled `--done` flag(s) from required_commands (#planmassdone): a plan cannot know which tracked-work items this turn actually completed, and a bulk backlog→queue mirror makes that list the whole open backlog. Adjudicate each id individually and pass only the ones this turn genuinely resolved: [{}]",
        ids.len(),
        ids.join(", ")
    )
}

fn finalize_placeholder_commands(
    file: &Path,
    fm: &frontmatter::Frontmatter,
    pending_mutations: &[PendingMutationPlan],
) -> Vec<String> {
    // `#planmassdone`: past the cap, drop the `ResolveExisting` ids entirely and
    // leave the bare finalize skeleton. `ExpectAdd` placeholders are unaffected —
    // they add tracked work rather than closing it, so they cannot lose history.
    let suppress_done = speculative_done_ids(pending_mutations).len() > MAX_SPECULATIVE_DONE_IDS;
    let pending = pending_mutations
        .iter()
        .filter(|mutation| {
            !(suppress_done && mutation.kind == PendingMutationKind::ResolveExisting)
        })
        .map(|mutation| FinalizePendingMutation {
            kind: match mutation.kind {
                PendingMutationKind::ResolveExisting => {
                    FinalizePendingMutationKind::ResolveExisting
                }
                PendingMutationKind::ExpectAdd => FinalizePendingMutationKind::ExpectAdd,
            },
            id: &mutation.id,
            target_files: &mutation.target_files,
        })
        .collect::<Vec<_>>();
    vec![finalize_command(file, fm.resolve_mode(), &pending)]
}

struct RoutingFields {
    dispatch_candidate: bool,
    task_class: String,
    risk: String,
    parallelizable: bool,
    suggested_parent_tier: String,
    model_tier: String,
    dispatch_mode: String,
    context_budget_tokens: usize,
    job_packet_budget_tokens: usize,
    write_scope: Vec<String>,
    required_proof: Vec<String>,
    tsift_context: TsiftContextPlan,
}

fn routing_fields(
    file: &Path,
    document: &str,
    execution_scope: ExecutionScope,
    repo_actions: &[String],
    prompt_targets: &[String],
    pending_mutations: &[PendingMutationPlan],
    fm: &frontmatter::Frontmatter,
) -> RoutingFields {
    let combined_text = prompt_targets
        .iter()
        .chain(repo_actions.iter())
        .chain(pending_mutations.iter().map(|mutation| &mutation.text))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let task_class = infer_task_class(repo_actions, &combined_text);
    let risk = infer_risk(repo_actions, &combined_text);
    let suggested_parent_tier = tier_for_risk(&risk).to_string();
    let dispatch_candidate = execution_scope == ExecutionScope::Normal
        && !repo_actions.is_empty()
        && dispatch_mode(fm) != "off";
    let parallelizable = dispatch_candidate && repo_actions.len() > 1;
    let context_budget_tokens = if risk == "high" { 10_000 } else { 6_000 };
    let mut required_proof = vec![
        "changed_paths".to_string(),
        "commands".to_string(),
        "verification".to_string(),
        "confidence".to_string(),
        "escalation".to_string(),
    ];
    if combined_text.contains("commit") {
        required_proof.push("commit".to_string());
    }
    if combined_text.contains("push") {
        required_proof.push("push".to_string());
    }

    RoutingFields {
        dispatch_candidate,
        task_class,
        risk: risk.clone(),
        parallelizable,
        suggested_parent_tier: suggested_parent_tier.clone(),
        model_tier: suggested_parent_tier,
        dispatch_mode: dispatch_mode(fm),
        context_budget_tokens,
        job_packet_budget_tokens: context_budget_tokens,
        write_scope: infer_write_scope(&combined_text),
        required_proof,
        tsift_context: tsift_context_plan(file, document, prompt_targets),
    }
}

fn dispatch_mode(fm: &frontmatter::Frontmatter) -> String {
    fm.dispatch
        .as_deref()
        .unwrap_or("manual")
        .trim()
        .to_ascii_lowercase()
}

/// `#releasepublishcontract`: recognize an operator instruction to ship, while
/// excluding status questions and requests to change the release process itself.
/// The latter may contain the same words but does not authorize a release now.
fn explicitly_requests_release_publish(text: &str) -> bool {
    let normalized = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    let meta_request = [
        "release process",
        "release workflow",
        "publishing process",
        "publishing workflow",
        "when i tell you",
        "whenever i tell you",
        "always release",
        "always publish",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    if meta_request {
        return false;
    }

    let status_question = [
        "is the release published",
        "is this release published",
        "was the release published",
        "has the release been published",
        "did the release publish",
    ]
    .iter()
    .any(|marker| normalized.contains(marker));
    if status_question {
        return false;
    }

    [
        "release + publish",
        "release and publish",
        "publish + release",
        "publish and release",
        "cut a release",
        "make a release",
        "ship the release",
        "release this",
        "publish this",
        "release it",
        "publish it",
        "release now",
        "publish now",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn infer_task_class(repo_actions: &[String], text: &str) -> String {
    if repo_actions.is_empty() {
        return "prompt_response".to_string();
    }
    if text.contains("lower-agent") || text.contains("orchestration") || text.contains("job packet")
    {
        return "lower_agent_orchestration".to_string();
    }
    if text.contains("spec") && text.contains("test") {
        return "spec_test_build".to_string();
    }
    if repo_actions.len() > 1 {
        return "multi_target_repo_work".to_string();
    }
    "tracked_repo_work".to_string()
}

fn infer_risk(repo_actions: &[String], text: &str) -> String {
    let high_risk_terms = [
        "concurrency",
        "crdt",
        "tmux",
        "routing",
        "orchestration",
        "lower-agent",
        "job packet",
        "dispatch",
        "git",
        "security",
        "cross-module",
    ];
    if repo_actions.len() > 3 || high_risk_terms.iter().any(|term| text.contains(term)) {
        return "high".to_string();
    }
    if repo_actions.len() > 1 || text.contains("test") || text.contains("build") {
        return "medium".to_string();
    }
    "low".to_string()
}

fn tier_for_risk(risk: &str) -> &'static str {
    match risk {
        "high" => "high",
        "medium" => "med",
        _ => "low",
    }
}

fn infer_write_scope(text: &str) -> Vec<String> {
    let mut scope = Vec::new();
    if text.contains("agent-doc") || text.contains("job") || text.contains("plan") {
        scope.push("src/agent-doc/src/".to_string());
    }
    if text.contains("spec") {
        scope.push("src/agent-doc/specs/".to_string());
    }
    if text.contains("runbook") {
        scope.push("src/agent-doc/runbooks/".to_string());
    }
    if text.contains("test") {
        scope.push("src/agent-doc/tests/".to_string());
    }
    if text.contains("tsift") {
        scope.push("src/tsift/".to_string());
    }
    scope.sort();
    scope.dedup();
    if scope.is_empty() {
        scope.push("undetermined".to_string());
    }
    scope
}

fn tsift_context_plan(file: &Path, document: &str, prompt_targets: &[String]) -> TsiftContextPlan {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let root = agent_doc_fs::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let status = if root.join(".tsift/index.db").exists() {
        "available"
    } else if root.join(".tsift").is_dir() {
        "missing_index"
    } else {
        "missing"
    };
    let file_arg = shell_quote(&display_path(file));
    let root_arg = shell_quote(&display_path(&root));
    let index_status_command = format!("tsift status --json {root_arg}");
    let context_pack_command = format!("tsift context-pack {file_arg} --json --budget normal");
    let diff_digest_command = format!("tsift diff-digest --cached {root_arg} --json");
    let test_digest_command =
        format!("tsift test-digest --path {root_arg} --input <test.log> --json");
    let loaded_context_ledger = build_loaded_context_ledger(vec![
        loaded_context_record(
            "agent-doc.plan.tsift-status",
            "generated-state",
            &display_path(&root),
            &index_status_command,
            None,
            "plan_build",
            "verify tsift index availability for this cycle",
        ),
        loaded_context_record(
            "agent-doc.plan.context-pack",
            "generated-state",
            &display_path(file),
            &context_pack_command,
            None,
            "plan_build",
            "declare bounded context-pack source for this cycle",
        ),
        loaded_context_record(
            "agent-doc.plan.diff-digest",
            "generated-state",
            &display_path(&root),
            &diff_digest_command,
            None,
            "plan_build",
            "declare bounded diff evidence source for this cycle",
        ),
        loaded_context_record(
            "agent-doc.plan.test-digest",
            "generated-state",
            &display_path(&root),
            &test_digest_command,
            None,
            "plan_build",
            "declare bounded test-output source for this cycle",
        ),
    ]);
    let mut diagnostics = Vec::new();
    let dynamic_manifest = match agent_doc_dynamic_context_io::build_dynamic_context_snapshot(
        file,
        document,
        prompt_targets,
    ) {
        Ok(manifest) => manifest,
        Err(err) => {
            diagnostics.push(format!("dynamic context manifest unavailable: {err:#}"));
            None
        }
    };
    if let Some(manifest) = &dynamic_manifest {
        diagnostics.extend(manifest.diagnostics.iter().cloned());
    }
    TsiftContextPlan {
        status: status.to_string(),
        freshness_policy: "fresh context required for automatic dispatch; manual packets record stale or missing context explicitly".to_string(),
        index_status_command,
        context_pack_command,
        source_read_command: "tsift source-read <handle>".to_string(),
        diff_digest_command,
        test_digest_command,
        stale_fallback: "skip tsift graph/context evidence, continue with parent-owned execution or manual packets, and carry the diagnostic for review".to_string(),
        loaded_context_ledger,
        dynamic_manifest,
        diagnostics,
    }
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
        })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn pending_mutations_for_doc(
    file: &Path,
    content: &str,
    repo_actions: &[String],
    prompt_targets: &[String],
    added_diff_lines: &[String],
    prompt_bearing_changes: &[diff::PromptBearingChange],
    prompt_presets: &agent_doc_frontmatter::frontmatter::PromptPresets,
) -> Result<Vec<PendingMutationPlan>> {
    let (fm, _) = frontmatter::parse(content).context("failed to parse frontmatter")?;
    let components = element::parse(content).context("failed to parse document components")?;
    let has_backlog = components
        .iter()
        .any(|component| is_backlog_component(&component.name));

    if !has_backlog {
        return Ok(Vec::new());
    }

    let items: Vec<backlog::PendingItem> = components
        .iter()
        .filter(|component| is_tracked_work_component(&component.name))
        .flat_map(|component| {
            let (_, items, _) = backlog::parse_items(component.content(content));
            items
        })
        .collect();
    let mut pending_mutations: Vec<PendingMutationPlan> = Vec::new();

    for action in repo_actions {
        for id in extract_do_pending_ids(action) {
            push_resolve_existing_mutation(&mut pending_mutations, &items, &id);
        }
    }

    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    let auto_done = agent_doc_frontmatter::project_config::resolve_auto_done(&fm, &project_config);
    let open_tracked_work_ids =
        agent_doc_document::tracked_work_projection::open_tracked_work_ids(content);
    let single_open_review_item_id =
        agent_doc_document::tracked_work_projection::single_open_review_item_id(content);
    for id in agent_doc_turn::closeout_signal::inline_done_signal_ids(
        prompt_targets,
        &open_tracked_work_ids,
        single_open_review_item_id.as_deref(),
        auto_done,
    ) {
        push_resolve_existing_mutation(&mut pending_mutations, &items, &id);
    }

    if agent_doc_prompt_contract::prompt_requests_backlog_work(
        prompt_targets,
        added_diff_lines,
        prompt_presets,
    ) {
        let issue_units = agent_doc_prompt_contract::ordered_issue_units_for_agent_doc_bug(
            prompt_targets,
            added_diff_lines,
            prompt_presets,
            prompt_bearing_changes,
        );
        let mut target_paths = agent_doc_prompt_contract::explicit_backlog_targets(
            file,
            prompt_targets,
            added_diff_lines,
            prompt_presets,
        )?;
        if target_paths.is_empty()
            && !issue_units.is_empty()
            && let Some(target) =
                agent_doc_project_config_io::agent_doc_bug_target_document_for_doc(file)?
        {
            target_paths.push(target);
        }
        let target_files = target_paths
            .into_iter()
            .map(|path| path.display().to_string())
            .collect();
        if issue_units.len() > 1 {
            eprintln!(
                "[plan] #agent-doc-bug declaration_order={} final_insert_order={}",
                issue_units
                    .iter()
                    .enumerate()
                    .map(|(idx, unit)| format!("{}:{}", idx + 1, truncate_for_plan_log(unit)))
                    .collect::<Vec<_>>()
                    .join(" | "),
                issue_units
                    .iter()
                    .enumerate()
                    .map(|(idx, unit)| format!("{}:{}", idx + 1, truncate_for_plan_log(unit)))
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
        }
        if issue_units.is_empty() {
            pending_mutations.push(PendingMutationPlan {
                kind: PendingMutationKind::ExpectAdd,
                id: String::new(),
                text: "user requested backlog/recommendations".to_string(),
                target_files,
            });
        } else {
            for issue in issue_units {
                pending_mutations.push(PendingMutationPlan {
                    kind: PendingMutationKind::ExpectAdd,
                    id: String::new(),
                    text: issue,
                    target_files: target_files.clone(),
                });
            }
        }
    }

    Ok(pending_mutations)
}

fn push_resolve_existing_mutation(
    pending_mutations: &mut Vec<PendingMutationPlan>,
    items: &[backlog::PendingItem],
    id: &str,
) {
    let Some(item) = items
        .iter()
        .find(|item| item.id.eq_ignore_ascii_case(id) && item.state != backlog::PendingState::Done)
    else {
        return;
    };
    if pending_mutations
        .iter()
        .any(|mutation| mutation.id == item.id)
    {
        return;
    }
    pending_mutations.push(PendingMutationPlan {
        kind: PendingMutationKind::ResolveExisting,
        id: item.id.clone(),
        text: item.text.clone(),
        target_files: Vec::new(),
    });
}

fn truncate_for_plan_log(text: &str) -> String {
    const MAX: usize = 80;
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX {
        return normalized;
    }
    let mut out = normalized.chars().take(MAX).collect::<String>();
    out.push_str("...");
    out
}

fn extract_do_pending_ids(action: &str) -> Vec<String> {
    agent_doc_queue::queue_directive::explicit_do_directive_target_ids(action)
}

/// `#plansealedturn`: the open, not-yet-captured preflight cycle for `file`,
/// if the binary's preflight has sealed this turn. Uses the same re-entry policy
/// preflight uses, so a stalled (crashed/superseded) older cycle never counts.
fn sealed_preflight_turn(file: &Path) -> Option<agent_doc_cycle_state_io::CycleState> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .filter(|cycle| {
            cycle.reusable_by_reentrant_preflight(
                0,
                now,
                agent_doc_cycle_state_io::STALLED_CYCLE_RESOLVE_SECS,
            )
        })
}

/// `#chatprompt` (GH #125): the chat prompts the open cycle carries.
fn open_cycle_chat_prompts(file: &Path) -> Vec<String> {
    agent_doc_cycle_state_io::load(file)
        .ok()
        .flatten()
        .filter(|cycle| cycle.is_open())
        .map(|cycle| {
            // `#steerbaselineabsorb`: absorbed steering the cycle carries is
            // the same kind of diff-invisible operator prompt.
            let mut prompts = cycle.chat_prompts;
            agent_doc_prompt_contract::push_unique_strings(
                &mut prompts,
                cycle.absorbed_steering_prompts,
            );
            prompts
        })
        .unwrap_or_default()
}

fn sealed_turn_warning(cycle_id: &str) -> String {
    format!(
        "#plansealedturn: no diff remains because preflight already sealed this turn \
         (cycle `{cycle_id}`) and consumed the baseline; this is not a no-op -- execute \
         the preflight cycle contract (`user_intent_prompt_changes`, `queue_prompts`)"
    )
}

fn shared_doc_security_blockers(
    file: &Path,
    fm: &frontmatter::Frontmatter,
    pending_mutations: &[PendingMutationPlan],
) -> Vec<String> {
    if fm.collaboration_mode() != frontmatter::CollaborationMode::Shared || fm.has_security_review()
    {
        return Vec::new();
    }

    pending_mutations
        .iter()
        .filter(|mutation| mutation.kind == PendingMutationKind::ResolveExisting)
        .filter_map(|mutation| {
            let referenced = agent_doc_fs::referenced_markdown_path(file, &mutation.text)?;
            Some(format!(
                "Shared document item `#{}` references {} but this file has no `agent_doc_security_review`. Add an approved review marker before reading another plan/backlog document in shared mode.",
                mutation.id,
                referenced.display()
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn template_frontmatter() -> frontmatter::Frontmatter {
        let doc = "---\nagent_doc_session: t\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\nbody\n";
        frontmatter::parse(doc).unwrap().0
    }

    fn resolve_existing(id: &str) -> PendingMutationPlan {
        PendingMutationPlan {
            kind: PendingMutationKind::ResolveExisting,
            id: id.to_string(),
            text: format!("item {id}"),
            target_files: Vec::new(),
        }
    }

    #[test]
    fn operation_tree_emits_only_the_selected_unblocked_path_in_phase_order() {
        let tree = build_operation_tree(
            &["fix the queue".to_string()],
            &["edit parser".to_string()],
            &["agent-doc finalize plan.md --stream".to_string()],
            &[resolve_existing("bug-1")],
            HandoffTarget::None,
            &[],
        );

        assert_eq!(tree.policy, "selected_path_only");
        let gate = &tree.root.children[0];
        assert_eq!(
            gate.condition,
            Some(OperationCondition {
                expression: "blockers.is_empty()".to_string(),
                result: true,
            })
        );
        let selected = &gate.children[0];
        assert_eq!(selected.id, "path.execute");
        assert_eq!(
            selected
                .children
                .iter()
                .map(|node| node.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "phase.prompts",
                "phase.repo_actions",
                "phase.pending_mutations",
                "phase.required_commands",
            ]
        );
        assert!(
            serde_json::to_string(&tree)
                .expect("operation tree json")
                .contains("selected_path_only")
        );
    }

    #[test]
    fn operation_tree_prunes_work_when_blocked() {
        let tree = build_operation_tree(
            &["fix the queue".to_string()],
            &["edit parser".to_string()],
            &["agent-doc finalize plan.md --stream".to_string()],
            &[],
            HandoffTarget::None,
            &["security review required".to_string()],
        );

        let gate = &tree.root.children[0];
        assert_eq!(
            gate.condition.as_ref().map(|condition| condition.result),
            Some(false)
        );
        assert_eq!(gate.children.len(), 1);
        assert_eq!(gate.children[0].id, "path.stop");
        assert!(gate.children[0].children.is_empty());
    }

    #[test]
    fn operation_tree_routes_handoff_without_manual_repo_action_branch() {
        let tree = build_operation_tree(
            &["fan out fixes".to_string()],
            &["edit parser".to_string()],
            &["agent-doc orchestrate plan.md --mode parallel --from-exchange".to_string()],
            &[],
            HandoffTarget::Orchestrate,
            &[],
        );

        let phases = &tree.root.children[0].children[0].children;
        assert_eq!(phases.len(), 2);
        assert_eq!(phases[0].id, "phase.prompts");
        assert_eq!(phases[1].id, "phase.handoff");
        assert!(
            phases.iter().all(|node| node.id != "phase.repo_actions"),
            "the orchestrator owns repo actions after handoff"
        );
        assert_eq!(phases[1].children[0].operation, "run_required_command");
    }

    /// `#planmassdone`: a bulk backlog→queue mirror must not produce a
    /// `required_commands` entry that closes the whole backlog. Observed live at
    /// 26 ids, only a handful of which the turn had actually completed.
    #[test]
    fn finalize_placeholder_suppresses_mass_speculative_done_flags() {
        let fm = template_frontmatter();
        let file = Path::new("plan.md");
        let many: Vec<PendingMutationPlan> = (0..26)
            .map(|i| resolve_existing(&format!("id{i}")))
            .collect();

        let commands = finalize_placeholder_commands(file, &fm, &many);
        let command = commands.first().expect("finalize placeholder command");
        assert!(
            !command.contains("--done"),
            "a 26-id mirror must not pre-fill --done flags: {command}"
        );
        assert!(
            command.contains("agent-doc finalize") && command.contains("--stream"),
            "the finalize skeleton itself must survive suppression: {command}"
        );

        let warning = mass_done_suppression_warning(&speculative_done_ids(&many));
        assert!(warning.contains("26"), "{warning}");
        assert!(warning.contains("#planmassdone"), "{warning}");
    }

    /// The ergonomic case is untouched: a turn that resolves one or a few named
    /// items still gets its `--done` flags pre-filled.
    #[test]
    fn finalize_placeholder_keeps_small_done_flag_lists() {
        let fm = template_frontmatter();
        let file = Path::new("plan.md");
        let few = vec![resolve_existing("abc1"), resolve_existing("def2")];

        let commands = finalize_placeholder_commands(file, &fm, &few);
        let command = commands.first().expect("finalize placeholder command");
        assert!(command.contains("--done abc1"), "{command}");
        assert!(command.contains("--done def2"), "{command}");
    }

    /// Suppression targets only backlog-CLOSING flags. `ExpectAdd` placeholders
    /// add tracked work, so they can never lose history and must survive.
    #[test]
    fn finalize_placeholder_suppression_spares_expect_add_placeholders() {
        let fm = template_frontmatter();
        let file = Path::new("plan.md");
        let mut mutations: Vec<PendingMutationPlan> = (0..26)
            .map(|i| resolve_existing(&format!("id{i}")))
            .collect();
        mutations.push(PendingMutationPlan {
            kind: PendingMutationKind::ExpectAdd,
            id: "newitem".to_string(),
            text: "add me".to_string(),
            target_files: vec!["tasks/other.md".to_string()],
        });

        let commands = finalize_placeholder_commands(file, &fm, &mutations);
        let command = commands.first().expect("finalize placeholder command");
        assert!(!command.contains("--done"), "{command}");
        assert!(
            command.contains("--pending-add-to tasks/other.md"),
            "expect-add placeholders must survive suppression: {command}"
        );
    }

    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
        _lock: parking_lot::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock();
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }

        fn unset(key: &'static str) -> Self {
            let lock = crate::test_support::TEST_ENV_LOCK.lock();
            let prev = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self {
                key,
                prev,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.prev {
                unsafe { std::env::set_var(self.key, value) };
            } else {
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn write_cycles_log(doc: &std::path::Path, entries: &[agent_doc_ops_log_io::CycleEntry]) {
        let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
        std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
        let mut file = std::fs::File::create(log_path).unwrap();
        for entry in entries {
            writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
        }
    }

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    fn subagents_queue_doc() -> &'static str {
        r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Queue

<!-- agent:queue priority subagents go -->
- Fix the claim button.
- Align the Close button.
<!-- /agent:queue -->
"#
    }

    #[test]
    fn connected_plan_exposes_queue_subagent_dispatch_without_a_document_diff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = subagents_queue_doc();
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.task_class, "queue_subagent_dispatch");
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(plan.queue_subagent_dispatch.len(), 2);
        assert_eq!(
            plan.queue_subagent_dispatch
                .iter()
                .map(|entry| entry.item.as_str())
                .collect::<Vec<_>>(),
            vec!["Fix the claim button.", "Align the Close button."]
        );
        assert!(plan.queue_subagent_dispatch_guidance.is_some());
        assert_eq!(
            plan.required_commands
                .iter()
                .filter(|command| command.contains("launch one native background subagent"))
                .count(),
            2
        );
    }

    struct LiveEditor {
        file: std::path::PathBuf,
        identity: String,
        replica: agent_doc_merge::crdt_sync::ReplicaState,
    }

    impl LiveEditor {
        fn new(file: &std::path::Path, editor_id: &str, content: &str) -> Self {
            let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
            agent_doc_test_support::seed_reliable_sync_editor_registration(
                &file,
                editor_id,
                &["operator_text_authority_v1", "lazily_transport_receipts_v1"],
            );
            let identity = format!("{editor_id}:{}", file.display());
            let (client_id, bootstrap) =
                agent_doc_crdt_relay_io::register_replica_for_file(&file, &identity)
                    .expect("test editor should register")
                    .expect("test editor should remain attached");
            let replica =
                agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)
                    .expect("test editor should decode the relay bootstrap");
            let editor = Self {
                file,
                identity,
                replica,
            };
            editor.publish(content);
            editor
        }

        fn publish(&self, content: &str) {
            let current = self.replica.text();
            self.replica
                .apply_local_edit(0, current.len() as u32, content);
            agent_doc_crdt_relay_io::relay_replica_update_for_file(
                &self.file,
                &self.identity,
                &self.replica.encode_state(),
            )
            .expect("test editor should publish")
            .expect("test editor update should be accepted");
        }
    }

    impl Drop for LiveEditor {
        fn drop(&mut self) {
            agent_doc_crdt_relay_io::deregister_editor_replica_for_file(
                &self.file,
                &self.identity,
                std::process::id(),
            )
            .expect("test editor should deregister");
        }
    }

    fn steering_context(payload: &str) -> Option<String> {
        agent_doc_session_check_io::midturn_steering::post_tool_use_response(payload)
            .expect("hook decision")
            .map(|value| {
                value["hookSpecificOutput"]["additionalContext"]
                    .as_str()
                    .expect("additionalContext")
                    .to_string()
            })
    }

    fn hook_payload(
        root: &std::path::Path,
        session_key: &str,
        session_id: &str,
        subagent: bool,
    ) -> String {
        let mut payload = serde_json::Map::new();
        payload.insert(
            session_key.to_string(),
            serde_json::Value::String(session_id.to_string()),
        );
        payload.insert(
            "cwd".to_string(),
            serde_json::Value::String(root.display().to_string()),
        );
        payload.insert(
            "tool_name".to_string(),
            serde_json::Value::String("Bash".to_string()),
        );
        if subagent {
            payload.insert(
                "subagentType".to_string(),
                serde_json::Value::String("general-purpose".to_string()),
            );
        }
        serde_json::Value::Object(payload).to_string()
    }

    fn normalize_steering(text: &str, file: &std::path::Path) -> String {
        let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
        text.replace(&canonical.display().to_string(), "<FILE>")
            .replace(&file.display().to_string(), "<FILE>")
    }

    /// `#steeringharnessparity`: a queue row added, and a claimed row struck,
    /// while disk stays stale, reaches Claude, Codex, and Grok through the
    /// same PostToolUse context. OpenCode has no PostToolUse channel; its
    /// busy delivery is the consuming `agent-doc steering` poll, and that
    /// render matches the hook. The next plan on each document reads that
    /// live cut.
    #[test]
    fn live_editor_steering_matches_across_claude_codex_grok_and_opencode() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let claimed = "release + publish: include issue 348";
        let baseline = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent_doc_steering_debounce_ms: 0\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n<!-- /agent:exchange -->\n\n## Queue\n\n<!-- agent:queue priority subagents go -->\n- Fix the claim button.\n- {claimed}\n<!-- /agent:queue -->\n"
        );
        let live = baseline.replace(&format!("- {claimed}\n"), "- Align the parity button.\n");

        let arm = |name: &str, editor_id: &str| {
            let file = dir.path().join(name);
            std::fs::write(&file, &baseline).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &file,
                &baseline,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();
            let editor = LiveEditor::new(&file, editor_id, &baseline);
            let cycle =
                agent_doc_cycle_state_io::start_preflight(&file, Some(&baseline), Some(&baseline))
                    .unwrap();
            agent_doc_session_check_io::midturn_steering::seed_for_cycle(
                &file,
                &cycle.cycle_id,
                &baseline,
                Some("Fix the claim button."),
                Vec::new(),
            )
            .unwrap();
            let (_, claim) =
                agent_doc_queue_io::queue_claim::claim(&file, claimed, "subagent:parity-348", 3600)
                    .unwrap();
            agent_doc_queue_io::queue_claim::start_attempt(
                &file,
                claimed,
                "subagent:parity-348",
                "parity-worker",
                3600,
                claim.generation,
            )
            .unwrap();
            // A whole-buffer relay replace is not provenance for cancelling a
            // claimed row. The operator delete has to be a captured editor op
            // replayed onto this cycle's baseline.
            let removed = format!("- {claimed}\n");
            let added = "- Align the parity button.\n";
            let offset = baseline.find(&removed).expect("claimed row");
            let ops = vec![
                agent_doc_merge::crdt::EditorOp::Delete {
                    offset,
                    len: removed.len(),
                },
                agent_doc_merge::crdt::EditorOp::Insert {
                    offset,
                    text: added.to_string(),
                },
            ];
            assert_eq!(
                agent_doc_merge::crdt::replay_editor_ops(&baseline, &ops).as_deref(),
                Some(live.as_str())
            );
            agent_doc_op_capture_io::record_editor_ops(
                &file,
                &agent_doc_hash::content_hash(&baseline),
                ops,
            )
            .unwrap();
            editor.publish(&live);
            assert_eq!(std::fs::read_to_string(&file).unwrap(), baseline);
            (file, editor)
        };
        let (grok_file, _grok_editor) = arm("grok.md", "grok-parity");
        let (claude_file, _claude_editor) = arm("claude.md", "claude-parity");
        let (codex_file, _codex_editor) = arm("codex.md", "codex-parity");
        let (opencode_file, _opencode_editor) = arm("opencode.md", "opencode-parity");

        agent_doc_codex_hook_io::bind_grok_admitted_session(&grok_file, "grok-session").unwrap();
        let bind_hook = |file: &std::path::Path, session_id: &str, harness| {
            agent_doc_codex_hook_io::apply_user_prompt_submit_for_harness(
                &agent_doc_codex_hook_io::UserPromptSubmitInput {
                    session_id: session_id.into(),
                    turn_id: String::new(),
                    cwd: dir.path().display().to_string(),
                    prompt: format!("agent-doc {}", file.display()),
                },
                harness,
            )
            .unwrap();
        };
        bind_hook(
            &claude_file,
            "claude-session",
            agent_doc_codex_hook_io::HookHarness::Claude,
        );
        bind_hook(
            &codex_file,
            "codex-session",
            agent_doc_codex_hook_io::HookHarness::Codex,
        );
        assert_eq!(
            agent_doc_harness::steering_delivery::steering_delivery_adapter("opencode").busy,
            agent_doc_harness::steering_delivery::BusyDelivery::PollAndCommandBoundary,
            "OpenCode stays on the poll; the test compares that delivery"
        );

        assert!(
            steering_context(&hook_payload(dir.path(), "session_id", "unbound", false)).is_none(),
            "an unbound session stays silent"
        );
        assert!(
            steering_context(&hook_payload(dir.path(), "sessionId", "grok-session", true))
                .is_none(),
            "a Grok subagentType tool call must not consume the owner watermark"
        );

        let mut delivered = Vec::new();
        for (label, file, key, session) in [
            ("Grok", &grok_file, "sessionId", "grok-session"),
            ("Claude", &claude_file, "session_id", "claude-session"),
            ("Codex", &codex_file, "session_id", "codex-session"),
        ] {
            let text = steering_context(&hook_payload(dir.path(), key, session, false))
                .unwrap_or_else(|| panic!("{label} owner hook"));
            assert!(
                steering_context(&hook_payload(dir.path(), key, session, false)).is_none(),
                "{label} must consume the delivered steering"
            );
            delivered.push((label, text, file.clone()));
        }
        let opencode = agent_doc_session_check_io::midturn_steering::observe(
            &opencode_file,
            agent_doc_session_check_io::midturn_steering::CONSUMER_CLI,
            true,
        )
        .unwrap()
        .expect("OpenCode poll")
        .render()
        .expect("OpenCode steering");
        assert!(
            agent_doc_session_check_io::midturn_steering::observe(
                &opencode_file,
                agent_doc_session_check_io::midturn_steering::CONSUMER_CLI,
                true,
            )
            .unwrap()
            .is_none(),
            "the OpenCode poll must consume the delivered steering"
        );
        delivered.push(("OpenCode", opencode, opencode_file.clone()));

        for (label, text, _) in &delivered {
            assert!(text.contains("Align the parity button."), "{label}: {text}");
            assert!(text.contains("dispatch=cancel_claimed"), "{label}: {text}");
            assert!(text.contains("dispatch=subagent"), "{label}: {text}");
        }
        let expected = normalize_steering(&delivered[0].1, &delivered[0].2);
        for (label, text, file) in &delivered[1..] {
            assert_eq!(
                normalize_steering(text, file),
                expected,
                "{label} steering must match {}",
                delivered[0].0
            );
        }

        let items = |plan: &DispatchPlan| {
            plan.queue_subagent_dispatch
                .iter()
                .map(|entry| entry.item.clone())
                .collect::<Vec<_>>()
        };
        let mut plans = Vec::new();
        for (label, _, file) in &delivered {
            plans.push((*label, items(&build(file).unwrap())));
            let on_disk = std::fs::read_to_string(file.as_path()).unwrap();
            assert_eq!(on_disk, baseline, "{label} disk must stay stale");
        }
        for (label, plan_items) in &plans[1..] {
            assert_eq!(plan_items, &plans[0].1, "{label} plan");
        }
        assert!(
            plans[0]
                .1
                .iter()
                .any(|item| item == "Align the parity button."),
            "{:?}",
            plans[0].1
        );
        assert!(
            plans[0].1.iter().all(|item| !item.contains(claimed)),
            "the struck claimed row must leave the next plan: {:?}",
            plans[0].1
        );
    }

    #[test]
    fn connected_plan_keeps_new_subagent_heads_out_of_inline_targets() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let current = subagents_queue_doc();
        let baseline = current.replace("- Fix the claim button.\n- Align the Close button.\n", "");
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(&baseline), Some(current)).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.queue_subagent_dispatch.len(), 2);
        assert_eq!(plan.task_class, "queue_subagent_dispatch");
        assert!(plan.dispatch_candidate);
        assert!(plan.parallelizable);
        assert!(
            plan.prompt_targets.is_empty(),
            "subagent dispatch heads must not be selected inline: {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.repo_actions.is_empty(),
            "subagent dispatch heads must not become inline repo actions: {:?}",
            plan.repo_actions
        );
    }

    /// Three `agent:run` sections. The first is only an operator-gated head, so
    /// the plan names both later `subagents` queues and does not inline them.
    #[test]
    fn connected_plan_names_the_next_run_when_the_first_has_nothing_to_do() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue: go
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Run <!-- agent:run id="main" subagents priority go -->
- 🙋 do [#blocked]:
  - 🙋 merge the pull request [#subagent-operator-request-blocked]
<!-- /agent:run -->

## API PRs <!-- agent:run id="api-prs" subagents -->
- https://example.com/pull/759
<!-- /agent:run -->

## API PRs <!-- agent:run id="contracts-prs" subagents -->
- https://example.com/pull/162
<!-- /agent:run -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.task_class, "queue_subagent_dispatch");
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(
            plan.queue_subagent_dispatch
                .iter()
                .map(|entry| entry.item.as_str())
                .collect::<Vec<_>>(),
            vec![
                "https://example.com/pull/759",
                "https://example.com/pull/162"
            ]
        );
        assert!(
            plan.prompt_targets.is_empty(),
            "the later runs must not be inlined: {:?}",
            plan.prompt_targets
        );
        assert!(plan.repo_actions.is_empty(), "{:?}", plan.repo_actions);
    }

    #[test]
    fn connected_plan_turns_an_unbound_claim_into_exact_recovery_commands() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = subagents_queue_doc();
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let (_, claim) = agent_doc_queue_io::queue_claim::claim(
            &doc,
            "Fix the claim button.",
            "subagent:claim-button",
            600,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.queue_subagent_dispatch.len(), 1);
        assert_eq!(plan.queue_dispatch_acknowledgement_required.len(), 1);
        let obligation = &plan.queue_dispatch_acknowledgement_required[0];
        assert_eq!(obligation.item, "Fix the claim button.");
        assert_eq!(obligation.generation, claim.generation);
        assert!(obligation.start_command.contains("queue start"));
        assert!(obligation.start_command.contains("--worker-id <worker-id>"));
        assert!(obligation.release_command.contains("release"));
        assert!(obligation.release_command.contains("--requeue"));
        assert!(
            plan.required_commands
                .iter()
                .any(|command| command.contains("no worker acknowledgement was recorded"))
        );
    }

    /// `#noparallelfanoutwhennotparallelizable`: the plan must not order a
    /// `--mode parallel` fan-out in the same contract that reports
    /// `parallelizable: false`. `required_commands` is mandatory for the skill, so
    /// emitting one made `orchestrate` create a git worktree and a `claude -p`
    /// pane per list line for a turn that has a single repo action.
    #[test]
    fn build_plan_declines_a_parallel_handoff_it_judged_not_parallelizable() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

Fix these concurrently
- agent: codex -> agent: claude
- queue: go -> queue: stop
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            !plan.parallelizable,
            "precondition: this turn must resolve as non-parallelizable"
        );
        assert_ne!(
            plan.handoff,
            HandoffTarget::Orchestrate,
            "a non-parallelizable turn must not hand off to orchestrate: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .all(|cmd| !cmd.contains("--mode parallel")),
            "no parallel fan-out may be ordered: {:?}",
            plan.required_commands
        );
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("declined a `--mode parallel`")),
            "the declined fan-out must be visible, not silent: {:?}",
            plan.warnings
        );
    }

    #[test]
    fn build_plan_detects_orchestration_handoff_and_existing_pending_item() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

synchronous orcestra
- do #1g42 Option A. update spec + tests. build + install for local testing. commit + push
- do #1g42 Option B. update spec + tests. build + install for local testing. commit + push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Orchestrate);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc orchestrate")
                    && cmd.contains("--mode sequential")
                    && cmd.contains("--from-exchange")),
            "expected orchestrate handoff command, got: {:?}",
            plan.required_commands
        );
        assert_eq!(plan.repo_actions.len(), 2);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "1g42");
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.prompt_targets.len(), 2);
    }

    #[test]
    fn build_plan_includes_finalize_placeholder_for_template_docs() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
What changed?
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.required_commands.iter().any(|cmd| {
                cmd.contains("agent-doc finalize")
                    && !cmd.contains("--baseline-file")
                    && cmd.contains("--stream")
            }),
            "expected finalize placeholder command, got: {:?}",
            plan.required_commands
        );
        assert_eq!(plan.handoff, HandoffTarget::None);
        assert!(plan.blockers.is_empty());
    }

    fn sealed_turn_doc() -> &'static str {
        r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#
    }

    /// `#plansealedturn` (GH #68 §1): once the binary's preflight has sealed the
    /// turn and consumed the baseline, plan's re-diff is empty. That must not be
    /// reported as a `No changes` blocker: SKILL.md step 0d stops on blockers.
    #[test]
    fn build_plan_reports_no_blocker_when_preflight_sealed_the_turn() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = sealed_turn_doc();
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let unsealed = build(&doc).unwrap();
        assert_eq!(unsealed.task_class, "no_changes");
        assert_eq!(
            unsealed.blockers.len(),
            1,
            "no open cycle keeps the blocker"
        );

        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        let plan = build(&doc).unwrap();
        assert!(plan.blockers.is_empty(), "sealed turn: {:?}", plan.blockers);
        assert_eq!(plan.task_class, "preflight_sealed");
        assert!(
            plan.warnings.iter().any(|w| w.contains("#plansealedturn")),
            "{:?}",
            plan.warnings
        );
    }

    /// GH #125 gap 1 (`#chatprompt`): the admitted cycle carries a chat prompt
    /// and the document did not change. Plan must treat it as the turn's
    /// target, not as an empty sealed turn or `no_changes`.
    #[test]
    fn build_plan_targets_chat_prompts_the_open_cycle_carries() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = sealed_turn_doc();
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_chat_prompts(
            &doc,
            &["Fix the flaky login test".to_string()],
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_ne!(plan.task_class, "no_changes");
        assert_ne!(plan.task_class, "preflight_sealed");
        assert!(
            plan.prompt_targets
                .contains(&"Fix the flaky login test".to_string()),
            "{:?}",
            plan.prompt_targets
        );
    }

    /// `#releasepublishcontract`: explicit shipping language is an executable
    /// plan requirement, not prose an agent may satisfy with a branch push or a
    /// local install.
    #[test]
    fn build_plan_requires_publication_for_explicit_release_request() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = sealed_turn_doc();
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_chat_prompts(
            &doc,
            &["Fix the queue parser + release + publish".to_string()],
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert!(
            plan.required_commands
                .iter()
                .any(|command| command.contains("run `make release`")),
            "{:?}",
            plan.required_commands
        );
        assert!(
            plan.required_proof.contains(&"github_release".to_string()),
            "{:?}",
            plan.required_proof
        );
        assert!(
            plan.required_proof
                .contains(&"pypi_publication".to_string()),
            "{:?}",
            plan.required_proof
        );
    }

    /// A question about the current release and a request to improve the process
    /// mention the same terms but do not themselves authorize a new release.
    #[test]
    fn build_plan_does_not_turn_release_status_or_process_questions_into_a_release() {
        for prompt in [
            "Is the release published?",
            "Can we fix the process to always release + publish when I tell you to",
        ] {
            let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
            let dir = setup_project();
            let doc = dir.path().join("plan.md");
            let content = sealed_turn_doc();
            std::fs::write(&doc, content).unwrap();
            agent_doc_snapshot_io::checkpoint_document_baseline(
                &doc,
                content,
                agent_doc_ops_log_io::log_op,
            )
            .unwrap();
            agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
            agent_doc_cycle_state_io::record_chat_prompts(&doc, &[prompt.to_string()]).unwrap();

            let plan = build(&doc).unwrap();
            assert!(
                plan.required_commands
                    .iter()
                    .all(|command| !command.contains("make release")),
                "prompt={prompt:?} commands={:?}",
                plan.required_commands
            );
            assert!(!plan.required_proof.contains(&"github_release".to_string()));
            assert!(
                !plan
                    .required_proof
                    .contains(&"pypi_publication".to_string())
            );
        }
    }

    /// `#plansealedturn`: the free-text queue head preflight selected for the
    /// sealed turn is recovered as the plan's prompt rather than dropped.
    #[test]
    fn build_plan_recovers_selected_queue_head_from_sealed_turn() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = sealed_turn_doc();
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(content), Some(content)).unwrap();
        agent_doc_cycle_state_io::record_selected_free_text_queue_heads(
            &doc,
            &["run tests".to_string()],
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_ne!(plan.task_class, "no_changes");
        assert_ne!(plan.task_class, "preflight_sealed");
        assert!(
            plan.repo_actions.iter().any(|a| a.contains("run tests")),
            "selected head should drive repo actions: {:?}",
            plan.repo_actions
        );
    }

    #[test]
    fn build_plan_uses_active_queue_prompt_when_document_has_no_diff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- do [#oobpmt]
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#oobpmt] Fix OOB prompt absorption.
<!-- /agent:backlog -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "active queue prompt should not plan as a no-op"
        );
        assert_eq!(plan.repo_actions, vec!["do [#oobpmt]"]);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "oobpmt");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done oobpmt")),
            "queue do item should require closeout with --done oobpmt: {:?}",
            plan.required_commands
        );
    }

    /// `#nestsubmit`: plan's queue prompt carries the parent lineage and the
    /// other branch. The leaf `#rebase` alone is not the prompt.
    #[test]
    fn build_plan_submits_a_nested_run_leaf_with_its_parent() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
- 📥 https://github.com/haiven-dev/haiven-api/pull/758
  - #rebase
- other branch
  - do [#notthis]
<!-- /agent:queue -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert!(
            plan.blockers.is_empty(),
            "nested queue prompt should not plan as a no-op: {:?}",
            plan.blockers
        );
        assert!(
            plan.prompt_targets.iter().any(|target| {
                target.starts_with("https://github.com/haiven-dev/haiven-api/pull/758: #rebase\n")
                    && target.contains("other branch")
                    && target.contains("do [#notthis]")
                    && target.contains("(this leaf)")
            }),
            "submitted prompt should carry the lineage and the other branch: {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.prompt_targets.iter().all(|target| target != "#rebase"),
            "the bare leaf is not the prompt: {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.repo_actions
                .iter()
                .all(|action| !action.contains("notthis")),
            "the sibling branch is context, not a directive: {:?}",
            plan.repo_actions
        );
    }

    /// `#planhead`: plan must select the same head preflight does. An
    /// `[operator-verify]` head sits ahead of a drainable one here; plan naming it
    /// would send the agent to work a human-gated item and then close it out.
    #[test]
    fn build_plan_skips_an_operator_verify_head_and_plans_the_drainable_one() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue priority go -->
- do [#livecheck]
- do [#realwork]
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#livecheck] [operator-verify] Watch a real IDE pane and confirm the layout.
- [ ] [#realwork] Fix the actual defect.
<!-- /agent:backlog -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.repo_actions,
            vec!["do [#realwork]"],
            "plan must skip the operator-gated head preflight defers"
        );
        assert_eq!(plan.prompt_targets, vec!["do [#realwork]"]);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done realwork")),
            "closeout must target the drainable head: {:?}",
            plan.required_commands
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|cmd| cmd.contains("livecheck")),
            "operator-verify head must never appear in plan's closeout contract: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_sources_explicit_queue_orchestration_from_queue() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
---
Fan out these tasks.
- do [#first]
- do [#second]
---
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#first] First task.
- [ ] [#second] Second task.
<!-- /agent:backlog -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Orchestrate, "{plan:?}");
        assert!(
            plan.required_commands.iter().any(|cmd| {
                cmd.contains("agent-doc orchestrate")
                    && cmd.contains("--mode parallel")
                    && cmd.contains("--from-queue")
            }),
            "queue orchestration must not source historical exchange prose: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_treats_active_queue_slash_command_as_command_handoff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue auto -->
-   /clear
<!-- /agent:queue -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.prompt_targets.is_empty(), "{plan:?}");
        assert!(plan.repo_actions.is_empty(), "{plan:?}");
        assert!(plan.pending_mutations.is_empty(), "{plan:?}");
        assert_eq!(plan.handoff, HandoffTarget::Other);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("`/clear`")),
            "slash command handoff should remain visible as a command requirement: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .all(|cmd| !cmd.contains("agent-doc finalize")),
            "slash-only command handoff must not require assistant finalization: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_warns_on_semantic_completion_match_for_free_text_queue() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        std::fs::write(
            dir.path().join("tasks.done.md"),
            "- 2026-06-07 [#cachefix] Repair cache duplication\n",
        )
        .unwrap();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
queue_active: true
---

<!-- agent:queue auto -->
- Repair cache duplication
<!-- /agent:queue -->

<!-- agent:done archive=tasks.done.md -->
<!-- /agent:done -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert_eq!(plan.prompt_targets, vec!["Repair cache duplication"]);
        assert!(
            plan.warnings.iter().any(|warning| {
                warning.contains("semantic completion candidate") && warning.contains("#cachefix")
            }),
            "{:?}",
            plan.warnings
        );
    }

    #[test]
    fn build_plan_ignores_inactive_queue_edit_as_repo_action() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: false
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue -->
<!-- /agent:queue -->

<!-- agent:backlog -->
- [ ] [#gdbpropscan] Inspect graph DB properties.
<!-- /agent:backlog -->
"#;
        let current = baseline.replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            "<!-- agent:queue -->\n- do [#gdbpropscan]\n<!-- /agent:queue -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.prompt_targets.is_empty(),
            "inactive queue edit must not become prompt targets: {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.repo_actions.is_empty(),
            "inactive queue edit must not become repo actions: {:?}",
            plan.repo_actions
        );
        assert!(
            plan.pending_mutations.is_empty(),
            "inactive queue edit must not resolve or capture pending work: {:?}",
            plan.pending_mutations
        );
    }

    /// `#planinactivequeuedone`: the reported shape, which the
    /// `build_plan_ignores_inactive_queue_edit_as_repo_action` case above does
    /// NOT cover. There the `do [#id]` line is an ADDITION in the diff, so
    /// `suppress_inactive_queue_additions` has something to suppress. Here the
    /// id-backed heads are already in the baseline and the operator's edit is
    /// the STOP itself — nothing is added, so suppression is a no-op and the
    /// heads can only reach the plan through the queue-head path.
    ///
    /// Observed on `tasks/software/lazily.md` at cycle-1790447018749: preflight
    /// correctly reported `queue_active: false`,
    /// `queue_drainable_head_count: 0`, `queue_continuation_required: false` and
    /// warned `inactive_queue_residue`, while `plan` still emitted
    /// `repo_actions` for all three inactive heads plus
    /// `required_commands: agent-doc finalize ... --done ... --done ... --done ...`.
    /// SKILL.md step 0d makes `required_commands` the execution contract, so
    /// following it literally marks three unexecuted items complete — including
    /// an `[operator-verify]` item no agent turn can satisfy. That is a
    /// false-closeout hazard, not cosmetic drift, which is why the `--done`
    /// assertion below is the load-bearing one.
    #[test]
    fn build_plan_emits_no_done_for_preexisting_heads_when_the_operator_stops_the_queue() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue_active: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue -->
- do [#lzfakepublisherproof]
- do [#lzartifactpurgeexec]
- do [#lzdocorphanresidue]
<!-- /agent:queue -->

<!-- agent:backlog queue -->
- [ ] [#lzfakepublisherproof] Prove the fake publisher.
- [ ] [#lzartifactpurgeexec] Execute the artifact purge.
- [ ] [#lzdocorphanresidue] [operator-verify] Clear the doc orphan residue.
<!-- /agent:backlog -->
"#;
        // The operator stops the queue and, in the same edit, appends one more
        // id-backed head. Both shapes must stay inert: the three pre-existing
        // heads are not in the diff at all (so suppression has nothing to
        // suppress and only a content-derived head path could surface them),
        // and the fourth IS an addition, which is what keeps
        // `suppress_inactive_queue_additions` load-bearing here.
        let current = baseline
            .replace("queue_active: true", "queue_active: false")
            .replace(
                "- do [#lzdocorphanresidue]\n<!-- /agent:queue -->",
                "- do [#lzdocorphanresidue]\n- do [#lzlatehead]\n<!-- /agent:queue -->",
            )
            .replace(
                "- [ ] [#lzdocorphanresidue] [operator-verify] Clear the doc orphan residue.",
                "- [ ] [#lzdocorphanresidue] [operator-verify] Clear the doc orphan residue.\n- [ ] [#lzlatehead] Added in the same edit that stopped the queue.",
            );
        std::fs::write(&doc, &current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.repo_actions.is_empty(),
            "a stopped queue's residue must not become repo actions: {:?}",
            plan.repo_actions
        );
        assert!(
            plan.pending_mutations.is_empty(),
            "a stopped queue's residue must not capture pending work: {:?}",
            plan.pending_mutations
        );
        let done_flags: Vec<&String> = plan
            .required_commands
            .iter()
            .filter(|command| command.contains("--done"))
            .collect();
        assert!(
            done_flags.is_empty(),
            "a plan must never pre-fill `--done` for heads the turn did not \
             execute — following this literally is a false closeout: {done_flags:?}"
        );
    }

    /// The same invariant from the other direction: no document diff at all, so
    /// `build` reaches the queue-head path (`#planhead`). A stopped queue must
    /// yield no dispatchable head, and therefore still no `--done`.
    #[test]
    fn build_plan_emits_no_done_for_a_stopped_queue_with_no_document_diff() {
        let _prompt = EnvGuard::unset("AGENT_DOC_HARNESS_PROMPT");
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
queue: pause
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

<!-- agent:queue -->
- do [#lzfakepublisherproof]
- do [#lzartifactpurgeexec]
<!-- /agent:queue -->

<!-- agent:backlog queue -->
- [ ] [#lzfakepublisherproof] Prove the fake publisher.
- [ ] [#lzartifactpurgeexec] Execute the artifact purge.
<!-- /agent:backlog -->
"#;
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.repo_actions.is_empty(),
            "a stopped queue has no drainable head: {:?}",
            plan.repo_actions
        );
        let done_flags: Vec<&String> = plan
            .required_commands
            .iter()
            .filter(|command| command.contains("--done"))
            .collect();
        assert!(
            done_flags.is_empty(),
            "no head was dispatched, so no `--done` may be pre-filled: {done_flags:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_plan_downgrades_locked_graph_db_to_manual_packet_only_warning() {
        use std::os::unix::fs::PermissionsExt;

        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#jobslock] Create job packets when graph.db is locked.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#jobslock]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let script = dir.path().join("fake-tsift-lock.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nif echo \"$*\" | grep -q 'graph-db.*--json status'; then echo 'Error code 5: The database file is locked' >&2; exit 1; fi\necho \"unexpected fake tsift args: $*\" >&2\nexit 2\n",
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", script.to_str().unwrap());

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert!(plan.manual_packet_only);
        assert!(plan.graph_evidence.is_none());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("database file is locked")),
            "expected lock warning, got {:?}",
            plan.warnings
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do [#jobslock]. spec-test-build-install-commit-push"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_plan_downgrades_stale_graph_db_to_manual_packet_only_warning() {
        use std::os::unix::fs::PermissionsExt;

        let dir = setup_project();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#staleg] Keep turns running when graph.db is stale.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#staleg]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let script = dir.path().join("fake-tsift-stale.sh");
        std::fs::write(
            &script,
            r#"#!/bin/sh
if echo "$*" | grep -q 'graph-db.*--json status'; then
  cat <<'JSON'
{"root":"/tmp/repo","graph_db":"/tmp/repo/.tsift/graph.db","freshness":{"status":"stale","fail_closed":true,"diagnostics":["graph.db is stale"]}}
JSON
  exit 0
fi
echo "unexpected fake tsift args: $*" >&2
exit 2
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
        let _env = EnvGuard::set("AGENT_DOC_TSIFT_BIN", script.to_str().unwrap());

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert!(plan.manual_packet_only);
        assert!(plan.graph_evidence.is_none());
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("graph.db is stale")
                    && warning.contains("manual_packet_only=true")),
            "expected stale graph warning, got {:?}",
            plan.warnings
        );
    }

    #[test]
    fn build_plan_includes_pending_done_for_bracketed_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#dodone] Close the matching backlog item
<!-- /agent:pending -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
do [#dodone]. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#dodone] Close the matching backlog item
<!-- /agent:pending -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.repo_actions.len(), 1);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.pending_mutations[0].id, "dodone");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc finalize")
                    && cmd.contains("--done dodone")
                    && cmd.contains("--stream")),
            "expected finalize command to carry --done, got: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_resolves_each_id_in_compound_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
- [ ] [#x63e] First packet target
- [ ] [#v4v0] Second packet target
<!-- /agent:pending -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#x63e] [#v4v0]. spec-test-build-install-commit-push\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.pending_mutations
                .iter()
                .map(|mutation| mutation.id.as_str())
                .collect::<Vec<_>>(),
            vec!["x63e", "v4v0"]
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done x63e") && cmd.contains("--done v4v0")),
            "expected finalize command to carry both --done flags, got: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_emits_lower_agent_routing_fields() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_dispatch: auto
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:backlog -->
- [ ] [#jobp1] Define lower-agent job packet spec and runbook.
- [ ] [#jobp2] Add tsift context packet tests.
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "do [#jobp1]\ndo [#jobp2]\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.dispatch_candidate);
        assert_eq!(plan.dispatch_mode, "auto");
        assert_eq!(plan.task_class, "lower_agent_orchestration");
        assert_eq!(plan.risk, "high");
        assert!(plan.parallelizable);
        assert_eq!(plan.model_tier, "high");
        assert_eq!(plan.context_budget_tokens, 10_000);
        assert!(
            plan.write_scope
                .contains(&"src/agent-doc/specs/".to_string())
        );
        assert!(plan.write_scope.contains(&"src/tsift/".to_string()));
        assert!(plan.required_proof.contains(&"verification".to_string()));
        assert_eq!(plan.tsift_context.status, "missing");
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--done jobp1") && cmd.contains("--done jobp2"))
        );
    }

    #[test]
    fn build_plan_resolves_existing_icebox_item_for_do_directive() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
<!-- /agent:pending -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ice01] Parked follow-up
<!-- /agent:icebox -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
do #ice01. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Pending

<!-- agent:pending -->
<!-- /agent:pending -->

## Icebox

<!-- agent:icebox -->
- [ ] [#ice01] Parked follow-up
<!-- /agent:icebox -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(
            plan.pending_mutations[0].kind,
            PendingMutationKind::ResolveExisting
        );
        assert_eq!(plan.pending_mutations[0].id, "ice01");
    }

    #[test]
    fn build_plan_dispatches_compact_exchange_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

compact exchange
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::Compact);
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc compact") && cmd.contains("--commit")),
            "expected compact handoff command, got: {:?}",
            plan.required_commands
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|cmd| cmd.contains("agent-doc finalize")),
            "compact handoff should not advertise finalize: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn test_plan_detects_backlog_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Existing item
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

add to backlog: what tasks remain?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#abc1] Existing item
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd mutation, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_expect_add_carries_explicit_backlog_target() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let target = dir.path().join("bugs.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            target.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "#agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd)
            .expect("expected ExpectAdd mutation");
        assert_eq!(
            expect_add.target_files,
            vec![
                std::fs::canonicalize(&target)
                    .unwrap()
                    .display()
                    .to_string()
            ]
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|cmd| cmd.contains("--pending-add-to") && cmd.contains("bugs.md")),
            "expected finalize hint to include --pending-add-to, got {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn agent_doc_bug_uses_configured_target_when_preset_has_no_explicit_file() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"agent_doc_bug_target_document = "tasks/agent-doc/agent-doc-bugs2.md"
"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/software")).unwrap();
        let doc = dir.path().join("tasks/software/source.md");
        let target = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog.
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "#agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd)
            .expect("expected ExpectAdd mutation");
        assert_eq!(
            expect_add.target_files,
            vec![target.canonicalize().unwrap().display().to_string()]
        );
    }

    #[test]
    fn agent_doc_bug_explicit_target_overrides_configured_target() {
        let dir = setup_project();
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            r#"agent_doc_bug_target_document = "configured-bugs.md"
"#,
        )
        .unwrap();
        let doc = dir.path().join("plan.md");
        let configured = dir.path().join("configured-bugs.md");
        let explicit = dir.path().join("explicit-bugs.md");
        std::fs::write(
            &configured,
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();
        std::fs::write(
            &explicit,
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            explicit.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "#agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd)
            .expect("expected ExpectAdd mutation");

        assert_eq!(
            expect_add.target_files,
            vec![explicit.canonicalize().unwrap().display().to_string()]
        );
    }

    #[test]
    fn plan_preserves_agent_doc_bug_declaration_order_for_target_adds() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let target = dir.path().join("bugs.md");
        std::fs::write(
            &target,
            "<!-- agent:backlog -->\n- [ ] [#old1] Existing\n<!-- /agent:backlog -->\n",
        )
        .unwrap();

        let baseline = format!(
            r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan. Add to the backlog of {}
---

## Exchange

<!-- agent:exchange patch=append -->
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#,
            target.display()
        );

        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            "First captured bug. #agent-doc-bug\n---\nSecond captured bug. #agent-doc-bug\n<!-- /agent:exchange -->",
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            &baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        let expect_adds = plan
            .pending_mutations
            .iter()
            .filter(|mutation| mutation.kind == PendingMutationKind::ExpectAdd)
            .collect::<Vec<_>>();

        assert_eq!(expect_adds.len(), 2);
        assert_eq!(expect_adds[0].text, "First captured bug. #agent-doc-bug");
        assert_eq!(expect_adds[1].text, "Second captured bug. #agent-doc-bug");
        assert_eq!(expect_adds[0].target_files, expect_adds[1].target_files);
    }

    #[test]
    fn test_plan_detects_recommendation_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

What should we do next? Any recommendations?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd for recommendation request, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_detects_backlog_request_via_prompt_preset_expansion() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

❯ #code-review
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_some(),
            "expected ExpectAdd for preset-expanded backlog request, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_classifies_embedded_next_steps_domain_prompt_as_actionable_backlog_request() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#next-steps': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let prompt =
            "Please analyze failed orders and bot traffic on sampleorders.com. #next-steps";
        let current = baseline.replace(
            "<!-- /agent:exchange -->",
            &format!("{prompt}\n<!-- /agent:exchange -->"),
        );
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.task_class, "prompt_response");
        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert!(
            plan.prompt_targets
                .iter()
                .any(|target| target.contains(prompt)),
            "expected embedded #next-steps domain prompt target, got {:?}",
            plan.prompt_targets
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|mutation| mutation.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from embedded #next-steps preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn test_plan_no_false_positive_on_questions() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xyz1] Some item
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.

How does the CRDT merge work?
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#xyz1] Some item
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        let expect_add = plan
            .pending_mutations
            .iter()
            .find(|m| m.kind == PendingMutationKind::ExpectAdd);
        assert!(
            expect_add.is_none(),
            "should not emit ExpectAdd for a plain question, got: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_uses_harness_prompt_when_snapshot_matches_document() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#code-review': Please review the codebase. '#follow-up-backlog'
  '#follow-up-backlog': Any follow-up items to place in the backlog?
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!("agent-doc {} #code-review", doc.display()),
        );
        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "unexpected blockers: {:?}",
            plan.blockers
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from harness prompt preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_blocks_shared_doc_plan_reference_without_security_review() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #spec2. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "spec2");
        assert_eq!(plan.blockers.len(), 1);
        assert!(plan.blockers[0].contains("agent_doc_security_review"));
    }

    #[test]
    fn build_plan_allows_shared_doc_plan_reference_with_security_review() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
agent_doc_security_review: sec-1
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
agent_doc_collaboration: shared
agent_doc_security_review: sec-1
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #spec2. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#spec2] Follow tasks/plan-follow-up.md before rollout
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();
        assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
        assert_eq!(plan.pending_mutations.len(), 1);
        assert_eq!(plan.pending_mutations[0].id, "spec2");
    }

    #[test]
    fn build_plan_resolves_existing_pending_item_from_harness_prompt() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#1g42] Add the post-preflight dispatch phase
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _prompt = EnvGuard::set(
            "AGENT_DOC_HARNESS_PROMPT",
            &format!(
                "agent-doc {}\ndo #1g42. spec-test-build-install-commit-push",
                doc.display()
            ),
        );
        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "unexpected blockers: {:?}",
            plan.blockers
        );
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| { m.kind == PendingMutationKind::ResolveExisting && m.id == "1g42" }),
            "expected ResolveExisting for harness prompt do-directive, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_resolves_explicit_inline_done_signal() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#inline-done-signal] Inline done signal
<!-- /agent:backlog -->
"#;

        let current = baseline.replace(
            "Done.\n<!-- /agent:exchange -->",
            "Done.\n\nmark #inline-done-signal done\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.pending_mutations.iter().any(|m| {
                m.kind == PendingMutationKind::ResolveExisting && m.id == "inline-done-signal"
            }),
            "expected ResolveExisting for explicit inline done signal, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_resolves_plain_done_to_single_review_item_when_auto_done_enabled() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
auto_done: true
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Waiting.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rev1] Await user acceptance
<!-- /agent:review -->
"#;

        let current = baseline.replace(
            "Waiting.\n<!-- /agent:exchange -->",
            "Waiting.\n\ndone\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ResolveExisting && m.id == "rev1"),
            "expected ResolveExisting for the single review item, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_resolve_plain_done_without_auto_done() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Waiting.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->

## Review

<!-- agent:review -->
- [/] [#rev1] Await user acceptance
<!-- /agent:review -->
"#;

        let current = baseline.replace(
            "Waiting.\n<!-- /agent:exchange -->",
            "Waiting.\n\ndone\n<!-- /agent:exchange -->",
        );

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            !plan
                .pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ResolveExisting && m.id == "rev1"),
            "plain done should require auto_done, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn plan_backlog_only_deferral_warning_names_suppressed_directive() {
        // #lr-queue-response-miss step 2: a runnable directive deferred by
        // plan_backlog_only must be surfaced by name.
        let diff = "--- snapshot\n+++ document\n@@ -0,0 +1 @@\n+do [#lr-queue-response-miss]\n";
        let warning =
            plan_backlog_only_deferral_warning(ExecutionScope::PlanBacklogOnly, diff).unwrap();
        assert!(warning.contains("plan_backlog_only"));
        assert!(
            warning.contains("lr-queue-response-miss"),
            "deferral warning must name the suppressed directive: {warning}"
        );
    }

    #[test]
    fn plan_backlog_only_deferral_warning_quiet_without_directive_or_in_normal_scope() {
        // Pure bug-capture (no imperative directive) stays quiet.
        assert!(
            plan_backlog_only_deferral_warning(
                ExecutionScope::PlanBacklogOnly,
                "+ just a clarifying question about the design?\n"
            )
            .is_none()
        );
        // Normal scope never warns even with a directive.
        assert!(plan_backlog_only_deferral_warning(ExecutionScope::Normal, "+do [#x]\n").is_none());
    }

    #[test]
    fn build_plan_marks_agent_doc_bug_prompt_as_plan_backlog_only() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let content = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — opus-4-6

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let _prompt = EnvGuard::set("AGENT_DOC_HARNESS_PROMPT", "#agent-doc-bug");
        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::PlanBacklogOnly);
        assert!(plan.repo_actions.is_empty(), "{:?}", plan.repo_actions);
        assert!(
            plan.pending_mutations
                .iter()
                .any(|m| m.kind == PendingMutationKind::ExpectAdd),
            "expected ExpectAdd from #agent-doc-bug preset expansion, got {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_treat_backlog_text_as_agent_doc_bug_prompt_scope() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of tasks/bugs.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #pbct. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#pbct] Respect `#agent-doc-bug` preset scope and fail closed before implementation.
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert_eq!(
            plan.repo_actions,
            vec!["do #pbct. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_keeps_copied_prompt_preset_definitions_out_of_prompt_scope() {
        let dir = setup_project();
        let doc = dir.path().join("tmux-router.md");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
<!-- /agent:backlog -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
prompt_presets:
  '#agent-doc-bug': Please create a plan for agent-doc to fix this issue. Add to the backlog of agent-loop/tasks/agent-doc/agent-doc-bugs2.md
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #tmuxreprocmd. spec-test-build-install-commit-push
<!-- /agent:exchange -->

## Backlog

<!-- agent:backlog -->
- [ ] [#tmuxreprocmd] Capture the exact command, crate root, and tooling context that produced the tmux-router diagnostic.
<!-- /agent:backlog -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert_eq!(plan.execution_scope, ExecutionScope::Normal);
        assert_eq!(
            plan.repo_actions,
            vec!["do #tmuxreprocmd. spec-test-build-install-commit-push".to_string()]
        );
        assert!(
            !plan
                .pending_mutations
                .iter()
                .any(|mutation| mutation.kind == PendingMutationKind::ExpectAdd),
            "copied preset definitions must not require agent-doc-bug backlog capture: {:?}",
            plan.pending_mutations
        );
    }

    #[test]
    fn build_plan_does_not_block_on_session_accretion_guard() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let long_exchange = (0..260)
            .map(|idx| format!("context line {idx}"))
            .collect::<Vec<_>>()
            .join("\n");

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#;

        let current = format!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{long_exchange}\ndo #ctxacc. spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n"
        );

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let plan = build(&doc).unwrap();

        assert!(plan.blockers.is_empty(), "unexpected blockers: {:?}", plan);
        assert_eq!(
            plan.repo_actions,
            vec!["do #ctxacc. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_keeps_repeated_noop_closeout_churn_advisory() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Re: prior — gpt-5

Done.

do #nooploop. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(20).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(10).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let plan = build(&doc).unwrap();

        assert_eq!(plan.handoff, HandoffTarget::None);
        assert_eq!(
            plan.repo_actions,
            vec!["do #nooploop. spec-test-build-install-commit-push".to_string()],
            "session-accretion no-op churn should remain advisory unless compact is explicit"
        );
        assert!(
            !plan
                .required_commands
                .iter()
                .any(|command| command.contains("agent-doc compact")),
            "session-accretion no-op churn must not force compaction: {:?}",
            plan.required_commands
        );
        assert!(
            plan.required_commands
                .iter()
                .any(|command| command.contains("agent-doc finalize")),
            "normal closeout should still be requested after repo work: {:?}",
            plan.required_commands
        );
    }

    #[test]
    fn build_plan_allows_turn_after_recent_compaction_recovery() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
do #cmpclr. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(120).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(110).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(100).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(90).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(80).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(70).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(60).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(50).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(40).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(30).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();

        let plan = build(&doc).unwrap();

        assert!(
            plan.blockers.is_empty(),
            "recent exchange compaction should clear closeout-churn blockers: {:?}",
            plan.blockers
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do #cmpclr. spec-test-build-install-commit-push".to_string()]
        );
    }

    #[test]
    fn build_plan_allows_post_compaction_rerun_noop_closeouts() {
        let dir = setup_project();
        let doc = dir.path().join("plan.md");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let baseline = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
<!-- /agent:exchange -->
"#;

        let current = r#"---
agent_doc_session: test
agent_doc_format: template
agent_doc_write: crdt
---

## Exchange

<!-- agent:exchange patch=append -->
### Session Summary

Compacted.
do #aftercmp. spec-test-build-install-commit-push
<!-- /agent:exchange -->
"#;

        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();
        write_cycles_log(
            &doc,
            &[
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(5).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
                agent_doc_ops_log_io::CycleEntry {
                    op: "commit_noop".to_string(),
                    file: "plan.md".to_string(),
                    timestamp: now.saturating_sub(4).to_string(),
                    commit_hash: None,
                    snapshot_hash: None,
                    file_hash: None,
                },
            ],
        );

        let plan = build(&doc).unwrap();

        assert_eq!(
            plan.handoff,
            HandoffTarget::None,
            "preflight no-op closeouts immediately after compact must not trap the rerun in another compact handoff"
        );
        assert_eq!(
            plan.repo_actions,
            vec!["do #aftercmp. spec-test-build-install-commit-push".to_string()]
        );
    }
}
