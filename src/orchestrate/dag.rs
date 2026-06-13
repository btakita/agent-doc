//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn run_with_dependencies(
    file: &Path,
    config: OrchestrateConfig,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
    parallel_runner: &impl ParallelRunner,
    allow_empty_parallel_tasks: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    match config.mode {
        OrchestrateMode::Sequential => {
            let batch = resolve_task_batch(file, &config)?;
            if batch.tasks.is_empty() {
                anyhow::bail!("no orchestration tasks found");
            }
            let prompt_preset_block = load_prompt_preset_block(file, &batch.requested_presets)?;
            eprintln!(
                "[orchestrate] mode: {}",
                config.mode.to_possible_value().unwrap().get_name()
            );
            eprintln!("[orchestrate] tasks: {}", batch.tasks.len());
            for (idx, task) in batch.tasks.iter().enumerate() {
                eprintln!("[orchestrate]   {}: {}", idx + 1, task);
            }
            if config.dry_run {
                eprintln!("[orchestrate] dry run — exiting without executing tasks");
                return Ok(());
            }
            if config.no_git {
                anyhow::bail!(
                    "`agent-doc orchestrate --mode sequential` requires git-backed finalize"
                );
            }
            let graph_evidence = collect_graph_evidence_for_tasks(file, &batch.tasks, false)?;
            let execution_tasks = batch
                .tasks
                .into_iter()
                .map(|task| ExecutionTask {
                    label: task.clone(),
                    prompt: apply_prompt_preset_block(&task, prompt_preset_block.as_deref()),
                })
                .collect::<Vec<_>>();
            if config.plan {
                print_plan(&execution_tasks);
                print_graph_plan(graph_evidence.as_ref(), &execution_tasks)?;
                return Ok(());
            }
            // #misfire-recovery-snapshot: before a queue-sourced auto-run mutates
            // the document/backlog, drop a lightweight pre-auto-run recovery tag
            // at HEAD (mirroring compact's pre-compact tag) so a misfiring
            // auto-run is recoverable without git/sidecar archaeology. Best-effort
            // and non-fatal — a tag failure must not block the run.
            if config.from_queue
                && let Err(e) = agent_doc_orchestration::compact::create_pre_mutation_tag(
                    file,
                    "pre-auto-run",
                    None,
                )
            {
                eprintln!(
                    "[orchestrate] Warning: could not create pre-auto-run recovery tag: {}",
                    e
                );
            }
            run_ordered_tasks_internal(
                file,
                &execution_tasks,
                OrderedTaskRunOptions {
                    exchange_source: batch.exchange_source.as_ref(),
                    agent_override: config.agent.as_deref(),
                    model_override: config.model.as_deref(),
                },
                global_config,
                lifecycle,
                agent_runner,
                graph_evidence.as_ref(),
            )
        }
        OrchestrateMode::Parallel => {
            let batch = resolve_task_batch(file, &config)?;
            if batch.tasks.is_empty() && !allow_empty_parallel_tasks {
                anyhow::bail!("no orchestration tasks found");
            }
            let prompt_preset_block = load_prompt_preset_block(file, &batch.requested_presets)?;
            eprintln!(
                "[orchestrate] mode: {}",
                config.mode.to_possible_value().unwrap().get_name()
            );
            eprintln!("[orchestrate] tasks: {}", batch.tasks.len());
            for (idx, task) in batch.tasks.iter().enumerate() {
                eprintln!("[orchestrate]   {}: {}", idx + 1, task);
            }
            let graph_evidence = if config.dry_run {
                None
            } else {
                collect_graph_evidence_for_tasks(file, &batch.tasks, true)?
            };
            let parallel_tasks = batch
                .tasks
                .into_iter()
                .map(|task| parallel::ParallelTask {
                    description: task.clone(),
                    prompt: apply_parallel_graph_context(
                        &task,
                        apply_prompt_preset_block(&task, prompt_preset_block.as_deref()),
                        graph_evidence.as_ref(),
                    ),
                })
                .collect::<Vec<_>>();
            if config.plan {
                let exec: Vec<ExecutionTask> = parallel_tasks
                    .iter()
                    .map(|t| ExecutionTask {
                        label: t.description.clone(),
                        prompt: t.prompt.clone(),
                    })
                    .collect();
                print_plan(&exec);
                print_graph_plan(graph_evidence.as_ref(), &exec)?;
                return Ok(());
            }
            parallel_runner.run(
                file,
                parallel::ParallelConfig {
                    tasks: parallel_tasks,
                    model: config.model,
                    no_git: config.no_git,
                    no_worktree: config.no_worktree,
                    timeout_secs: config.timeout_secs,
                    dry_run: config.dry_run,
                },
            )
        }
        OrchestrateMode::Dag => {
            let batch = resolve_task_batch(file, &config)?;
            let prompt_preset_block = load_prompt_preset_block(file, &batch.requested_presets)?;
            if config.from_queue || config.resume_schedule.is_some() {
                return run_auto_dag_mode(
                    file,
                    &config,
                    batch,
                    prompt_preset_block.as_deref(),
                    global_config,
                    lifecycle,
                    agent_runner,
                );
            }
            let dag_tasks = resolve_dag_tasks(&batch)?;
            if dag_tasks.is_empty() {
                anyhow::bail!("no orchestration tasks found");
            }
            eprintln!(
                "[orchestrate] mode: {}",
                config.mode.to_possible_value().unwrap().get_name()
            );
            eprintln!("[orchestrate] tasks: {}", dag_tasks.len());
            for (idx, task) in dag_tasks.iter().enumerate() {
                if task.deps.is_empty() {
                    eprintln!("[orchestrate]   {}: [{}] {}", idx + 1, task.id, task.prompt);
                } else {
                    eprintln!(
                        "[orchestrate]   {}: [{}] {} (after: {})",
                        idx + 1,
                        task.id,
                        task.prompt,
                        task.deps.join(", ")
                    );
                }
            }
            if config.dry_run {
                eprintln!("[orchestrate] dry run — exiting without executing tasks");
                return Ok(());
            }
            if config.no_git {
                anyhow::bail!("`agent-doc orchestrate --mode dag` requires git-backed finalize");
            }
            let graph_targets = dag_tasks
                .iter()
                .map(|task| task.prompt.clone())
                .collect::<Vec<_>>();
            let graph_evidence = collect_graph_evidence_for_tasks(file, &graph_targets, false)?;
            let execution_tasks = plan_dag_execution(&dag_tasks)?
                .into_iter()
                .map(|task| ExecutionTask {
                    label: task.label.clone(),
                    prompt: apply_prompt_preset_block(&task.prompt, prompt_preset_block.as_deref()),
                })
                .collect::<Vec<_>>();
            if config.plan {
                print_plan(&execution_tasks);
                print_graph_plan(graph_evidence.as_ref(), &execution_tasks)?;
                return Ok(());
            }
            // #misfire-recovery-snapshot: before a queue-sourced auto-run mutates
            // the document/backlog, drop a lightweight pre-auto-run recovery tag
            // at HEAD (mirroring compact's pre-compact tag) so a misfiring
            // auto-run is recoverable without git/sidecar archaeology. Best-effort
            // and non-fatal — a tag failure must not block the run.
            if config.from_queue
                && let Err(e) = agent_doc_orchestration::compact::create_pre_mutation_tag(
                    file,
                    "pre-auto-run",
                    None,
                )
            {
                eprintln!(
                    "[orchestrate] Warning: could not create pre-auto-run recovery tag: {}",
                    e
                );
            }
            run_ordered_tasks_internal(
                file,
                &execution_tasks,
                OrderedTaskRunOptions {
                    exchange_source: None,
                    agent_override: config.agent.as_deref(),
                    model_override: config.model.as_deref(),
                },
                global_config,
                lifecycle,
                agent_runner,
                graph_evidence.as_ref(),
            )
        }
    }
}

pub(crate) fn run_ordered_tasks_internal(
    file: &Path,
    tasks: &[ExecutionTask],
    options: OrderedTaskRunOptions<'_>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
    graph_evidence: Option<&crate::tsift_graph::TsiftGraphEvidencePlan>,
) -> Result<()> {
    let mut effective_model: Option<String> = options.model_override.map(String::from);
    let dispatch_ctx = build_dispatch_context(file);
    agent_doc_orchestration::flow::orchestration_batch::log_queue_freeze_event(
        file,
        tasks.len(),
        options.exchange_source.is_some(),
    );

    for (idx, task) in tasks.iter().enumerate() {
        eprintln!(
            "[orchestrate] step {}/{}: {}",
            idx + 1,
            tasks.len(),
            task.label
        );

        let item = queue_dispatch::classify(&task.label);
        match item.kind {
            QueueItemKind::Command => {
                let result = queue_dispatch::dispatch_command(&item, &dispatch_ctx)?;
                if let queue_dispatch::DispatchResult::ModelOverride(tier) = result {
                    eprintln!("[orchestrate] model override updated to: {}", tier);
                    effective_model = Some(tier);
                }
            }
            QueueItemKind::Prompt => {
                let graph_context = graph_evidence
                    .and_then(|evidence| evidence.prompt_context_for_task(&task.label).transpose())
                    .transpose()?;
                let child_result = match run_ordered_task_step(
                    file,
                    &task.prompt,
                    OrderedTaskStepOptions {
                        agent_override: options.agent_override,
                        model_override: effective_model.as_deref(),
                        graph_context: graph_context.as_deref(),
                        graph_evidence,
                        task_label: &task.label,
                    },
                    global_config,
                    lifecycle,
                    agent_runner,
                ) {
                    Ok(()) => {
                        agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                            label: task.label.clone(),
                            outcome: agent_doc_orchestration::flow::types::FlowOutcome::Completed,
                            proof: Some(
                                graph_evidence
                                    .and_then(|evidence| {
                                        evidence.closeout_audit_proof_for_task(&task.label)
                                    })
                                    .unwrap_or_else(|| "finalize_session_check".to_string()),
                            ),
                        }
                    }
                    Err(err) => {
                        let child =
                            agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                                label: task.label.clone(),
                                outcome:
                                    agent_doc_orchestration::flow::types::FlowOutcome::FailedClosed,
                                proof: Some("child_step_error".to_string()),
                            };
                        agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(file, &child);
                        return Err(err);
                    }
                };
                agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(
                    file,
                    &child_result,
                );
                if idx + 1 < tasks.len()
                    && !agent_doc_orchestration::flow::orchestration_batch::batch_should_continue(
                        options
                            .exchange_source
                            .map(|source| exchange_task_source_changed(file, source))
                            .transpose()?
                            .unwrap_or(false),
                        &child_result,
                    )
                {
                    finalize_orchestration_batch_changed(file, idx + 1, tasks.len(), lifecycle)?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn run_auto_dag_mode(
    file: &Path,
    config: &OrchestrateConfig,
    batch: ResolvedTaskBatch,
    prompt_preset_block: Option<&str>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    if config.no_git {
        anyhow::bail!(
            "`agent-doc orchestrate --mode dag --from-queue` requires git-backed finalize"
        );
    }

    let graph_evidence = if config.resume_schedule.is_some() {
        None
    } else {
        collect_graph_evidence_for_tasks(file, &batch.tasks, false)?
    };
    let schedule = if let Some(schedule_id) = config.resume_schedule.as_deref() {
        crate::auto_dag::load_schedule(file, schedule_id)?
    } else {
        let guard = crate::auto_dag::session_review_guard_for_file(file)?;
        crate::auto_dag::build_schedule(
            file,
            &batch.tasks,
            graph_evidence.as_ref(),
            guard,
            "queue",
        )?
    };
    let schedule_path = crate::auto_dag::write_schedule(file, &schedule)?;
    eprintln!(
        "[orchestrate] auto-DAG schedule: {} ({})",
        schedule.schedule_id,
        schedule_path.display()
    );
    eprintln!("[orchestrate] auto-DAG batches: {}", schedule.batches.len());
    for (idx, batch) in schedule.batches.iter().enumerate() {
        eprintln!("[orchestrate]   batch {}: {}", idx + 1, batch.join(", "));
    }

    let jobs_index = crate::jobs::create_for_schedule(
        file,
        &schedule,
        crate::jobs::CreateOptions {
            operation_doc: true,
            audit: true,
            budget: 6000,
        },
    )?;
    eprintln!(
        "[orchestrate] auto-DAG job packets: {} job(s) cycle={}",
        jobs_index.jobs.len(),
        jobs_index.cycle_id
    );

    let schedule_blocker = crate::auto_dag::guard_blocker(&schedule);
    let schedule_decision = if schedule_blocker.is_some() {
        agent_doc_orchestration::flow::orchestration_batch::AutoDagScheduleDecision::SessionReviewBlocked
    } else {
        agent_doc_orchestration::flow::orchestration_batch::AutoDagScheduleDecision::Ready
    };
    agent_doc_orchestration::flow::orchestration_batch::log_auto_dag_schedule_event(
        file,
        schedule_decision,
        schedule.nodes.len(),
        schedule.batches.len(),
    );
    if let Some(blocker) = schedule_blocker {
        anyhow::bail!(blocker);
    }

    let execution_tasks = execution_tasks_from_schedule(&schedule, prompt_preset_block)?;
    if execution_tasks.is_empty() {
        eprintln!("[orchestrate] auto-DAG schedule already complete");
        return Ok(());
    }
    if config.plan || config.dry_run {
        print_plan(&execution_tasks);
        return Ok(());
    }

    run_scheduled_dag_tasks_internal(
        file,
        &schedule,
        ScheduledDagRunOptions {
            prompt_preset_block,
            ordered: OrderedTaskRunOptions {
                exchange_source: None,
                agent_override: config.agent.as_deref(),
                model_override: config.model.as_deref(),
            },
            graph_evidence: graph_evidence.as_ref(),
        },
        global_config,
        lifecycle,
        agent_runner,
    )
}

pub(crate) fn execution_tasks_from_schedule(
    schedule: &crate::auto_dag::AutoDagSchedule,
    prompt_preset_block: Option<&str>,
) -> Result<Vec<ExecutionTask>> {
    let mut tasks = Vec::new();
    for node in &schedule.nodes {
        match node.state {
            crate::auto_dag::AutoDagNodeState::Complete => continue,
            crate::auto_dag::AutoDagNodeState::Blocked
            | crate::auto_dag::AutoDagNodeState::Failed => {
                anyhow::bail!(
                    "auto-DAG schedule {} has gated/failed node {}; refusing to launch dependents",
                    schedule.schedule_id,
                    node.id
                );
            }
            crate::auto_dag::AutoDagNodeState::Pending
            | crate::auto_dag::AutoDagNodeState::Ready
            | crate::auto_dag::AutoDagNodeState::Running => {
                tasks.push(ExecutionTask {
                    label: node.label.clone(),
                    prompt: apply_prompt_preset_block(&node.prompt, prompt_preset_block),
                });
            }
        }
    }
    Ok(tasks)
}

pub(crate) fn run_scheduled_dag_tasks_internal(
    file: &Path,
    schedule: &crate::auto_dag::AutoDagSchedule,
    options: ScheduledDagRunOptions<'_>,
    global_config: &Config,
    lifecycle: &impl LifecycleOps,
    agent_runner: &impl FreshAgentRunner,
) -> Result<()> {
    let mut effective_model: Option<String> = options.ordered.model_override.map(String::from);
    let dispatch_ctx = build_dispatch_context(file);
    agent_doc_orchestration::flow::orchestration_batch::log_queue_freeze_event(
        file,
        schedule.nodes.len(),
        true,
    );

    for batch in &schedule.batches {
        eprintln!("[orchestrate] auto-DAG batch: {}", batch.join(", "));
        for node_id in batch {
            let Some(node) = schedule.nodes.iter().find(|node| &node.id == node_id) else {
                anyhow::bail!("auto-DAG schedule references missing node `{node_id}`");
            };
            if node.state == crate::auto_dag::AutoDagNodeState::Complete {
                continue;
            }
            if matches!(
                node.state,
                crate::auto_dag::AutoDagNodeState::Blocked
                    | crate::auto_dag::AutoDagNodeState::Failed
            ) {
                anyhow::bail!(
                    "auto-DAG node {} is {:?}; refusing to launch dependents",
                    node.id,
                    node.state
                );
            }

            crate::auto_dag::update_node_state(
                file,
                &schedule.schedule_id,
                &node.id,
                crate::auto_dag::AutoDagNodeState::Running,
            )?;
            let prompt = apply_prompt_preset_block(&node.prompt, options.prompt_preset_block);
            let item = queue_dispatch::classify(&node.label);
            let graph_context = options
                .graph_evidence
                .and_then(|evidence| evidence.prompt_context_for_task(&node.label).transpose())
                .transpose()?;
            let step_result = match item.kind {
                QueueItemKind::Command => {
                    let result = queue_dispatch::dispatch_command(&item, &dispatch_ctx)?;
                    if let queue_dispatch::DispatchResult::ModelOverride(tier) = result {
                        eprintln!("[orchestrate] model override updated to: {}", tier);
                        effective_model = Some(tier);
                    }
                    Ok(())
                }
                QueueItemKind::Prompt => run_ordered_task_step(
                    file,
                    &prompt,
                    OrderedTaskStepOptions {
                        agent_override: options.ordered.agent_override,
                        model_override: effective_model.as_deref(),
                        graph_context: graph_context.as_deref(),
                        graph_evidence: options.graph_evidence,
                        task_label: &node.label,
                    },
                    global_config,
                    lifecycle,
                    agent_runner,
                ),
            };

            match step_result {
                Ok(()) => {
                    crate::auto_dag::update_node_state(
                        file,
                        &schedule.schedule_id,
                        &node.id,
                        crate::auto_dag::AutoDagNodeState::Complete,
                    )?;
                    let child =
                        agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                            label: node.label.clone(),
                            outcome: agent_doc_orchestration::flow::types::FlowOutcome::Completed,
                            proof: Some(
                                options
                                    .graph_evidence
                                    .and_then(|evidence| {
                                        evidence.closeout_audit_proof_for_task(&node.label)
                                    })
                                    .unwrap_or_else(|| "finalize_session_check".to_string()),
                            ),
                        };
                    agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(
                        file, &child,
                    );
                }
                Err(err) => {
                    crate::auto_dag::update_node_state(
                        file,
                        &schedule.schedule_id,
                        &node.id,
                        crate::auto_dag::AutoDagNodeState::Failed,
                    )?;
                    let child =
                        agent_doc_orchestration::flow::orchestration_batch::BatchChildResult {
                            label: node.label.clone(),
                            outcome:
                                agent_doc_orchestration::flow::types::FlowOutcome::FailedClosed,
                            proof: Some("auto_dag_child_step_error".to_string()),
                        };
                    agent_doc_orchestration::flow::orchestration_batch::log_child_closeout_event(
                        file, &child,
                    );
                    return Err(err);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
use std::cell::RefCell;
use tempfile::TempDir;
#[test]
fn sequential_orchestration_injects_prompt_and_finalizes() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    fs::write(&doc, template_doc()).unwrap();
    fs::write(&baseline, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response:
            "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n"
                .to_string(),
        streaming_chunks: None,
    };

    let tasks = vec![ExecutionTask {
        label: "do #gkke".to_string(),
        prompt: "do #gkke".to_string(),
    }];

    run_ordered_tasks_internal(
        &doc,
        &tasks,
        OrderedTaskRunOptions {
            exchange_source: None,
            agent_override: None,
            model_override: Some("gpt-5"),
        },
        &Config::default(),
        &lifecycle,
        &agent,
        None,
    )
    .unwrap();

    let final_doc = fs::read_to_string(&doc).unwrap();
    assert!(final_doc.contains("❯ do #gkke"));
    assert!(final_doc.contains("### Re: task — gpt-5"));
    assert_eq!(*lifecycle.preflight_calls.borrow(), 1);
    assert_eq!(lifecycle.finalize_calls.borrow().len(), 1);
    assert_eq!(*lifecycle.session_checks.borrow(), 1);
    assert!(
        agent.prompts.borrow()[0].contains("<diff>"),
        "sequential prompt should include the document diff"
    );
    assert!(
        agent.prompts.borrow()[0].contains("❯ do #gkke"),
        "fresh agent prompt should include the injected task"
    );
    assert_eq!(*agent.fresh_calls.borrow(), 1);
    assert_eq!(*agent.streaming_calls.borrow(), 0);
}
#[test]
fn sequential_orchestration_attaches_tsift_graph_context_to_agent_prompt() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    fs::write(&doc, template_doc()).unwrap();
    fs::write(&baseline, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response:
            "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented.\n<!-- /patch:exchange -->\n"
                .to_string(),
        streaming_chunks: None,
    };
    let tasks = vec![ExecutionTask {
        label: "do #gkke".to_string(),
        prompt: "do #gkke".to_string(),
    }];
    let graph_evidence = test_graph_evidence_plan();

    run_ordered_tasks_internal(
        &doc,
        &tasks,
        OrderedTaskRunOptions {
            exchange_source: None,
            agent_override: None,
            model_override: Some("gpt-5"),
        },
        &Config::default(),
        &lifecycle,
        &agent,
        Some(&graph_evidence),
    )
    .unwrap();

    let prompt = &agent.prompts.borrow()[0];
    assert!(prompt.contains("<tsift_graph_evidence>"));
    assert!(prompt.contains("\"evidence_packet_id\": \"gevd-gkke\""));
    assert!(prompt.contains("Worker 1 owns gkke (#gkke)"));
    assert!(prompt.contains("\"context_pack\""));
    assert!(prompt.contains("\"candidates\""));
    assert!(prompt.contains("Fail closed if the task requires a forbidden/shared file"));
    assert!(prompt.contains("\"token_budget\""));
    assert!(prompt.contains("\"lower_agent_job_packet\""));
    assert!(prompt.contains("\"owned_files\": ["));
    assert!(prompt.contains("\"read_only_context\": ["));
    assert!(prompt.contains("\"forbidden_files\": []"));
    assert!(prompt.contains("\"expected_tests\": ["));
    assert!(prompt.contains("\"expansion_commands\": ["));
    assert!(prompt.contains("\"fail_closed_prompt\""));
    let finalize_calls = lifecycle.finalize_calls.borrow();
    assert_eq!(finalize_calls.len(), 1);
    assert!(
        finalize_calls[0].contains("worker_result: completed #gkke"),
        "child closeout should include a tsift-projectable worker_result line:\n{}",
        finalize_calls[0]
    );
    assert!(finalize_calls[0].contains("src/orchestrate.rs"));
    assert!(finalize_calls[0].contains("`cargo test orchestrate`"));
    let final_doc = fs::read_to_string(&doc).unwrap();
    assert!(!final_doc.contains("<tsift_graph_evidence>"));
}
#[test]
fn sequential_orchestration_always_reruns_preflight_after_injection() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    fs::write(&doc, template_doc()).unwrap();
    fs::write(&baseline, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response:
            "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n"
                .to_string(),
        streaming_chunks: None,
    };

    let tasks = vec![ExecutionTask {
        label: "do #opcc".to_string(),
        prompt: "do #opcc".to_string(),
    }];

    run_ordered_tasks_internal(
        &doc,
        &tasks,
        OrderedTaskRunOptions {
            exchange_source: None,
            agent_override: None,
            model_override: Some("gpt-5"),
        },
        &Config::default(),
        &lifecycle,
        &agent,
        None,
    )
    .unwrap();

    let final_doc = fs::read_to_string(&doc).unwrap();
    assert_eq!(
        *lifecycle.preflight_calls.borrow(),
        1,
        "sequential mode should always rerun preflight after prompt injection"
    );
    assert!(final_doc.contains("❯ do #opcc"));
    assert!(agent.prompts.borrow()[0].contains("❯ do #opcc"));
    assert_eq!(lifecycle.finalize_calls.borrow().len(), 1);
    assert_eq!(*lifecycle.session_checks.borrow(), 1);
    assert_eq!(*agent.fresh_calls.borrow(), 1);
    assert_eq!(*agent.streaming_calls.borrow(), 0);
}
#[test]
fn sequential_orchestration_expands_prompt_presets_into_task_prompt() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nprompt_presets:\n  \"#1\": |\n    Today is 2026-04-25.\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
    fs::write(&doc, content).unwrap();
    fs::write(&baseline, content).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response:
            "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nImplemented.\n<!-- /patch:exchange -->\n"
                .to_string(),
        streaming_chunks: None,
    };

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Sequential,
            tasks_explicit: Vec::new(),
            from_file: None,
            from_exchange: true,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: Some("gpt-5".to_string()),
            no_git: false,
            no_worktree: true,
            timeout_secs: 30,
            dry_run: false,
            plan: false,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &FakeParallelRunner::default(),
        false,
    )
    .unwrap();

    let prompt = agent.prompts.borrow()[0].clone();
    assert!(prompt.contains("(preset #1)\nToday is 2026-04-25.\nKeep the work tree clean."));
    assert!(prompt.contains("❯ (preset #1)\nToday is 2026-04-25."));
}
#[test]
fn sequential_orchestration_stops_when_exchange_task_list_changes() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsync orchestra\npreset #spec\n- do #first\n- do #second\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
    fs::write(&doc, content).unwrap();
    fs::write(&baseline, content).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = MutatingAgentRunner {
        fresh_calls: RefCell::new(0),
        response: "<!-- patch:exchange -->\n### Re: first — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
            .to_string(),
    };
    let tasks = vec![
        ExecutionTask {
            label: "do #first".to_string(),
            prompt: "do #first".to_string(),
        },
        ExecutionTask {
            label: "do #second".to_string(),
            prompt: "do #second".to_string(),
        },
    ];
    let source = ExchangeTaskSourceFingerprint {
        tasks: vec!["do #first".to_string(), "do #second".to_string()],
        requested_presets: vec!["#spec".to_string()],
    };

    let err = run_ordered_tasks_internal(
        &doc,
        &tasks,
        OrderedTaskRunOptions {
            exchange_source: Some(&source),
            agent_override: None,
            model_override: Some("gpt-5"),
        },
        &Config::default(),
        &lifecycle,
        &agent,
        None,
    )
    .unwrap_err()
    .to_string();

    let final_doc = fs::read_to_string(&doc).unwrap();
    assert!(err.contains("orchestration batch changed during run"));
    assert!(final_doc.contains("- do #inserted"));
    assert!(final_doc.contains("### Re: first — gpt-5"));
    assert!(final_doc.contains("### Re: orchestration batch changed — gpt-5"));
    assert!(!final_doc.contains("❯ do #second"));
    assert_eq!(*agent.fresh_calls.borrow(), 1);
    assert_eq!(lifecycle.finalize_calls.borrow().len(), 2);
    assert_eq!(*lifecycle.session_checks.borrow(), 2);
}
#[test]
fn sequential_orchestration_uses_streaming_backend_for_crdt_docs() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();
    fs::write(&doc, template_doc()).unwrap();
    fs::write(&baseline, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: String::new(),
        streaming_chunks: Some(vec![
            StreamChunk {
                text: "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n".to_string(),
                thinking: None,
                is_final: false,
                session_id: None,
            },
            StreamChunk {
                text: "<!-- patch:exchange -->\n### Re: streamed — gpt-5\n\nImplemented and verified.\n\nVerification:\n- `cargo test`\n<!-- /patch:exchange -->\n".to_string(),
                thinking: None,
                is_final: true,
                session_id: Some("sess-stream".to_string()),
            },
        ]),
    };

    let tasks = vec![ExecutionTask {
        label: "do #4qja".to_string(),
        prompt: "do #4qja".to_string(),
    }];

    run_ordered_tasks_internal(
        &doc,
        &tasks,
        OrderedTaskRunOptions {
            exchange_source: None,
            agent_override: None,
            model_override: Some("gpt-5"),
        },
        &Config::default(),
        &lifecycle,
        &agent,
        None,
    )
    .unwrap();

    let final_doc = fs::read_to_string(&doc).unwrap();
    assert!(final_doc.contains("❯ do #4qja"));
    assert!(final_doc.contains("### Re: streamed — gpt-5"));
    assert_eq!(final_doc.matches("### Re: streamed — gpt-5").count(), 1);
    assert_eq!(*agent.streaming_calls.borrow(), 1);
    assert_eq!(*agent.fresh_calls.borrow(), 0);
}
#[test]
fn dag_orchestration_runs_topological_order() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    fs::write(&doc, template_doc()).unwrap();
    fs::write(&baseline, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response:
            "<!-- patch:exchange -->\n### Re: task — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                .to_string(),
        streaming_chunks: None,
    };
    let dag_tasks = vec![
        DagTask {
            id: "#prep".to_string(),
            prompt: "do #prep".to_string(),
            deps: Vec::new(),
        },
        DagTask {
            id: "#report".to_string(),
            prompt: "do #report".to_string(),
            deps: vec!["#prep".to_string(), "#bench".to_string()],
        },
        DagTask {
            id: "#bench".to_string(),
            prompt: "do #bench".to_string(),
            deps: vec!["#prep".to_string()],
        },
    ];

    let execution = plan_dag_execution(&dag_tasks).unwrap();
    assert_eq!(
        execution
            .iter()
            .map(|task| task.prompt.as_str())
            .collect::<Vec<_>>(),
        vec!["do #prep", "do #bench", "do #report"]
    );

    run_ordered_tasks_internal(
        &doc,
        &execution,
        OrderedTaskRunOptions {
            exchange_source: None,
            agent_override: None,
            model_override: Some("gpt-5"),
        },
        &Config::default(),
        &lifecycle,
        &agent,
        None,
    )
    .unwrap();

    assert_eq!(lifecycle.finalize_calls.borrow().len(), 3);
    assert_eq!(*lifecycle.session_checks.borrow(), 3);
    let prompts = agent.prompts.borrow();
    assert!(prompts[0].contains("❯ do #prep"));
    assert!(prompts[1].contains("❯ do #bench"));
    assert!(prompts[2].contains("❯ do #report"));
}
#[test]
fn parallel_mode_uses_shared_parallel_runner() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };
    let parallel_runner = FakeParallelRunner::default();

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Parallel,
            tasks_explicit: vec!["  ❯ do #9pw9  ".to_string()],
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: Some("gpt-5".to_string()),
            no_git: true,
            no_worktree: true,
            timeout_secs: 45,
            dry_run: true,
            plan: false,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &parallel_runner,
        false,
    )
    .unwrap();

    let calls = parallel_runner.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].1,
        vec![parallel::ParallelTask {
            description: "do #9pw9".to_string(),
            prompt: "do #9pw9".to_string(),
        }]
    );
    assert_eq!(calls[0].2.as_deref(), Some("gpt-5"));
    assert!(calls[0].3);
    assert!(calls[0].4);
    assert_eq!(calls[0].5, 45);
    assert!(calls[0].6);
    assert!(lifecycle.finalize_calls.borrow().is_empty());
    assert!(agent.prompts.borrow().is_empty());
}
#[cfg(unix)]
#[test]
fn parallel_mode_continues_without_graph_evidence_when_tsift_is_stale() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
    std::fs::write(dir.path().join(".tsift/graph.db"), "fake").unwrap();
    let doc = dir.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();

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

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };
    let parallel_runner = FakeParallelRunner::default();

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Parallel,
            tasks_explicit: vec!["do #gkke".to_string()],
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: true,
            no_worktree: true,
            timeout_secs: 30,
            dry_run: false,
            plan: false,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &parallel_runner,
        false,
    )
    .unwrap();

    let calls = parallel_runner.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1[0].description, "do #gkke");
    assert_eq!(calls[0].1[0].prompt, "do #gkke");
    assert!(!calls[0].1[0].prompt.contains("<tsift_graph_evidence>"));
}
#[test]
fn parallel_mode_expands_prompt_presets_into_task_prompt_only() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    fs::write(
        &doc,
        "---\nprompt_presets:\n  \"#1\": |\n    Keep the work tree clean.\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\npreset #1\n- do #prep\n<!-- /agent:exchange -->\n",
    )
    .unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };
    let parallel_runner = FakeParallelRunner::default();

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Parallel,
            tasks_explicit: Vec::new(),
            from_file: None,
            from_exchange: true,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: true,
            no_worktree: true,
            timeout_secs: 30,
            dry_run: true,
            plan: false,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &parallel_runner,
        false,
    )
    .unwrap();

    let call = &parallel_runner.calls.borrow()[0];
    assert_eq!(call.1[0].description, "do #prep");
    assert_eq!(
        call.1[0].prompt,
        "(preset #1)\nKeep the work tree clean.\ndo #prep"
    );
}
#[test]
fn legacy_parallel_compat_allows_empty_task_list() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };
    let parallel_runner = FakeParallelRunner::default();

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Parallel,
            tasks_explicit: Vec::new(),
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: false,
            no_worktree: false,
            timeout_secs: 600,
            dry_run: false,
            plan: false,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &parallel_runner,
        true,
    )
    .unwrap();

    let calls = parallel_runner.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].1.is_empty());
}
#[test]
fn plan_flag_sequential_prints_expanded_prompts_without_executing() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Sequential,
            tasks_explicit: vec!["do #prep".to_string(), "do #report".to_string()],
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: false,
            no_worktree: false,
            timeout_secs: 30,
            dry_run: false,
            plan: true,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &FakeParallelRunner::default(),
        false,
    )
    .unwrap();

    assert!(lifecycle.finalize_calls.borrow().is_empty());
    assert!(agent.prompts.borrow().is_empty());
}
#[test]
fn plan_flag_sequential_expands_preset_in_output() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    let preset_doc = "---\nagent_doc_format: template\nagent_doc_write: crdt\nagent: claude\nprompt_presets:\n  \"#1\": \"Today is 2026-04-25.\\nKeep the work tree clean.\"\n---\n<!-- agent:exchange -->\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n<!-- agent:pending -->\n<!-- /agent:pending -->\n";
    fs::write(&doc, preset_doc).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Sequential,
            tasks_explicit: vec!["do #prep".to_string()],
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: false,
            no_worktree: false,
            timeout_secs: 30,
            dry_run: false,
            plan: true,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &FakeParallelRunner::default(),
        false,
    )
    .unwrap();

    assert!(lifecycle.finalize_calls.borrow().is_empty());
    assert!(agent.prompts.borrow().is_empty());
}
#[test]
fn plan_flag_parallel_exits_without_calling_runner() {
    let dir = TempDir::new().unwrap();
    let doc = dir.path().join("session.md");
    fs::write(&doc, template_doc()).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: "unused".to_string(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response: "unused".to_string(),
        streaming_chunks: None,
    };
    let parallel_runner = FakeParallelRunner::default();

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Parallel,
            tasks_explicit: vec!["do #a".to_string(), "do #b".to_string()],
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: false,
            no_worktree: false,
            timeout_secs: 30,
            dry_run: false,
            plan: true,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &parallel_runner,
        false,
    )
    .unwrap();

    assert!(parallel_runner.calls.borrow().is_empty());
}
#[test]
fn sequential_orchestration_adds_codex_network_override_to_child_env() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("session.md");
    let baseline = dir.path().join("baseline.md");
    let content = "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\nagent: codex\ncodex_args: \"-s danger-full-access\"\ncodex_network_access: enabled\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nsynchronous orchestra\n<!-- agent:boundary:keep -->\n<!-- /agent:exchange -->\n";
    fs::write(&doc, content).unwrap();
    fs::write(&baseline, content).unwrap();

    let lifecycle = FakeLifecycleOps {
        baseline_file: baseline.to_string_lossy().into_owned(),
        preflight_calls: RefCell::new(0),
        finalize_calls: RefCell::new(Vec::new()),
        session_checks: RefCell::new(0),
    };
    let agent = FakeAgentRunner {
        prompts: RefCell::new(Vec::new()),
        envs: RefCell::new(Vec::new()),
        fresh_calls: RefCell::new(0),
        streaming_calls: RefCell::new(0),
        response:
            "<!-- patch:exchange -->\n### Re: network — gpt-5\n\nDone.\n<!-- /patch:exchange -->\n"
                .to_string(),
        streaming_chunks: None,
    };

    run_with_dependencies(
        &doc,
        OrchestrateConfig {
            mode: OrchestrateMode::Sequential,
            tasks_explicit: vec!["do #net".to_string()],
            from_file: None,
            from_exchange: false,
            from_queue: false,
            resume_schedule: None,
            agent: None,
            model: None,
            no_git: false,
            no_worktree: false,
            timeout_secs: 30,
            dry_run: false,
            plan: false,
        },
        &Config::default(),
        &lifecycle,
        &agent,
        &FakeParallelRunner::default(),
        false,
    )
    .unwrap();

    let envs = agent.envs.borrow();
    assert_eq!(envs.len(), 1);
    assert!(envs[0].iter().any(|(key, value)| {
        key == agent_doc_orchestration::agent::CODEX_SANDBOX_NETWORK_DISABLED_ENV
            && value.is_none()
    }));
}
}
