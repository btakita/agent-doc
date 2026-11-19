//! Connected coordinator actions projected from the existing queue policies.
//!
//! The queue and lineage authorities still decide what can advance. The MCP
//! transport's durable receipts only fence replay of the same client action;
//! they never claim a queue slot or replace its generation fence.

use std::path::Path;
use std::process::Command;

use agent_doc_queue::queue_claim::{QueueClaimState, claim_item_handle};
use agent_doc_queue::reported_closeout::{
    ReportedCloseoutAction, ReportedCloseoutReadiness, ReportedCloseoutStep,
};
use agent_doc_queue_io::{queue_claim, queue_lineage, subagent_dispatch, subagent_pool};
use agent_doc_sqlite::state_store;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const WORKER_ID_SLOT: &str = "<worker-id>";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Claim,
    Start,
    Refresh,
    Report,
    Closeout,
    ExternalCloseout,
    DecideCloseout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Receipt {
    SpawnReserved,
    ExternalReserved,
    Started,
    Applied,
    Failed { reason: String },
}

#[derive(Debug, Serialize)]
struct Action {
    next_action_id: String,
    kind: Kind,
    item: String,
    next_action_argv: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handoff: Option<Value>,
    #[serde(skip)]
    spawn: Option<Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    awaiting_harness: bool,
    receipt: Option<Receipt>,
}

impl Action {
    fn new(
        file: &Path,
        kind: Kind,
        item: &str,
        argv: Option<Vec<String>>,
        evidence: Value,
    ) -> Result<Self> {
        let fingerprint = serde_json::to_string(&(file, kind, item, &argv, evidence))?;
        Ok(Self {
            next_action_id: agent_doc_hash::content_hash(&fingerprint),
            kind,
            item: item.to_string(),
            next_action_argv: argv,
            handoff: None,
            spawn: None,
            awaiting_harness: false,
            receipt: None,
        })
    }

    fn runnable(&self) -> bool {
        self.receipt.is_none() && self.next_action_argv.is_some() && !self.awaiting_harness
    }
}

fn receipt_key(file: &Path, id: &str) -> String {
    let doc = agent_doc_fs::document_state_hash_from_str(&file.to_string_lossy());
    format!("mcp_queue_action_v1:{doc}:{id}")
}

fn receipt(raw: Option<String>) -> Result<Option<Receipt>> {
    raw.map(|value| serde_json::from_str(&value).context("decode connected queue-action receipt"))
        .transpose()
}

fn agent_doc_argv(file: &Path, verb: &str, item: &str) -> Vec<String> {
    [
        "agent-doc",
        "queue",
        verb,
        &file.to_string_lossy(),
        "--item",
        item,
    ]
    .map(str::to_string)
    .to_vec()
}

fn project_actions(file: &Path, content: &str) -> Result<Vec<Action>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(Vec::new());
    };
    let conn = state_store::open_state_db(&root)?;
    let with_receipt = |mut action: Action| -> Result<Action> {
        action.receipt = receipt(state_store::load_project_runtime_state_from_db(
            &conn,
            &receipt_key(file, &action.next_action_id),
        )?)?;
        Ok(action)
    };
    let pool = subagent_pool::observe_pool_refill_for_content(file, content)?;
    let claims = queue_claim::active_claims_for_content(file, content)?;
    let lineage = queue_lineage::load(file)?;
    let mut actions = Vec::new();
    // Pool warnings only show unbound claims after their grace period. A
    // successful connected claim needs its exact start action immediately.
    for entry in claims
        .iter()
        .filter(|entry| entry.state == QueueClaimState::Claimed && entry.worker.is_none())
    {
        let authorization =
            subagent_dispatch::subagent_authorization_for_content(file, content, &entry.item_text)?;
        let mut argv = agent_doc_argv(file, "start", &claim_item_handle(&entry.item_text));
        argv.extend([
            "--owner".into(),
            entry.owner.clone(),
            "--generation".into(),
            entry.generation.to_string(),
            "--worker-id".into(),
            WORKER_ID_SLOT.into(),
        ]);
        let mut action = Action::new(file, Kind::Start, &entry.item_text, Some(argv), Value::Null)?;
        // The ID names the reservation, so an in-place text edit after spawn
        // cannot mint a second spawn for the same claim generation.
        let initial_identity = lineage
            .attempts
            .iter()
            .find(|attempt| {
                attempt.owner == entry.owner
                    && attempt.generation == entry.generation
                    && (attempt.identity == entry.identity
                        || attempt.identity_history.contains(&entry.identity))
            })
            .and_then(|attempt| attempt.identity_history.first())
            .unwrap_or(&entry.identity);
        action.next_action_id = agent_doc_hash::content_hash(&serde_json::to_string(&(
            file,
            Kind::Start,
            initial_identity,
            &entry.owner,
            entry.generation,
            entry.claimed_at_secs,
        ))?);
        action.spawn = Some(json!({
            "spawn_preamble": authorization.subagent_prompt_preamble,
            "model": authorization.model,
            "effort": authorization.effort,
        }));
        actions.push(with_receipt(action)?);
    }
    for head in pool.next_heads {
        let authorization =
            subagent_dispatch::subagent_authorization_for_content(file, content, &head.item)?;
        let generation = queue_lineage::max_generation(file, &head.item)?;
        let evidence = json!([generation, authorization_evidence(&authorization)]);
        let mut argv = agent_doc_argv(file, "claim", &claim_item_handle(&head.item));
        let owner = agent_doc_hash::content_hash(&head.item);
        argv.extend([
            "--owner".into(),
            format!("subagent:action-{}", &owner[..12]),
        ]);
        actions.push(with_receipt(Action::new(
            file,
            Kind::Claim,
            &head.item,
            Some(argv),
            evidence,
        )?)?);
    }
    for entry in pool.refresh_due {
        let mut argv = agent_doc_argv(file, "claim", &claim_item_handle(&entry.item));
        argv.extend([
            "--owner".into(),
            entry.owner,
            "--refresh".into(),
            "--generation".into(),
            entry.generation.to_string(),
        ]);
        let evidence = json!([entry.worker_id, entry.last_heartbeat_secs]);
        let mut action = Action::new(file, Kind::Refresh, &entry.item, Some(argv), evidence)?;
        action.handoff = Some(json!({ "live_worker_id_required": entry.worker_id }));
        actions.push(with_receipt(action)?);
    }
    for claim in claims
        .iter()
        .filter(|claim| claim.state == QueueClaimState::Running)
    {
        let Some(worker) = &claim.worker else {
            continue;
        };
        let ended = pool.report_owed.iter().any(|report| {
            report.item == claim.item_text
                && report.owner == claim.owner
                && report.generation == claim.generation
                && report.worker_id == worker.worker_id
        });
        let mut argv = agent_doc_argv(
            file,
            "report-complete",
            &claim_item_handle(&claim.item_text),
        );
        argv.extend(["--generation".into(), claim.generation.to_string()]);
        let evidence = json!([claim.owner, worker.worker_id]);
        let mut action = Action::new(file, Kind::Report, &claim.item_text, Some(argv), evidence)?;
        action.awaiting_harness = !ended;
        action.handoff = Some(json!({ "worker_id": worker.worker_id }));
        actions.push(with_receipt(action)?);
    }
    for owed in queue_claim::reported_closeout_obligations_for_content(file, content)? {
        if owed.readiness == ReportedCloseoutReadiness::AwaitingOperator {
            continue;
        }
        let authorization =
            subagent_dispatch::subagent_authorization_for_content(file, content, &owed.item)?;
        let sink = observed_sink_ref(&owed.closeout.steps);
        let evidence = json!([
            owed.owner,
            owed.generation,
            owed.closeout,
            authorization.tracked_items,
            sink
        ]);
        let mut next = None;
        for step in &owed.closeout.steps {
            if !step.concrete || step.alternative.is_some() {
                break;
            }
            let kind = if step.argv.first().is_some_and(|name| name == "agent-doc")
                || step.action == ReportedCloseoutAction::VerifyIntegration
            {
                Kind::Closeout
            } else {
                Kind::ExternalCloseout
            };
            let mut action = Action::new(
                file,
                kind,
                &owed.item,
                Some(step.argv.clone()),
                evidence.clone(),
            )?;
            action.handoff = Some(json!({ "detail": step.detail }));
            let action = with_receipt(action)?;
            // A read-only step can succeed without changing queue or git
            // evidence. The receipt advances to the next ordered step.
            if action.receipt != Some(Receipt::Applied) {
                next = Some(action);
                break;
            }
            if step.action == ReportedCloseoutAction::Consume {
                next = Some(action);
            }
        }
        if let Some(action) = next {
            actions.push(action);
        } else if let Some(decision) = &owed.closeout.decision {
            let mut action = Action::new(file, Kind::DecideCloseout, &owed.item, None, evidence)?;
            action.handoff = Some(json!({
                "decision": decision,
                "operator_request_packet": owed.closeout.operator_request_packet,
            }));
            actions.push(action);
        }
    }
    Ok(actions)
}

fn observed_sink_ref(steps: &[ReportedCloseoutStep]) -> Option<Value> {
    let (repo, reference) = steps.iter().find_map(|step| {
        let argument = |flag| {
            step.argv
                .windows(2)
                .find(|pair| pair[0] == flag)
                .map(|pair| pair[1].as_str())
        };
        let repo = argument("--sink-repo")?;
        let reference = argument("--sink-ref")?;
        (!(repo.starts_with('<') || reference.starts_with('<'))).then_some((repo, reference))
    })?;
    let key = agent_doc_git_io::ref_cas::GitRefKey::new(Path::new(repo), reference);
    Some(match agent_doc_git_io::ref_cas::observe_ref(&key) {
        Ok(oid) => json!({ "oid": oid }),
        Err(error) => json!({ "error": error.to_string() }),
    })
}

fn authorization_evidence(auth: &agent_doc_queue::subagent_brief::SubagentAuthorization) -> Value {
    // Full preambles carry sibling prompt context. An unrelated edit must not
    // clear a failure for this exact row and resolved authorization.
    json!([
        auth.status,
        auth.queue_id,
        auth.tracked_items,
        auth.presets,
        auth.model,
        auth.effort
    ])
}

fn attach_actions(result: &mut Value, actions: Vec<Action>) {
    let next = actions.iter().find(|action| action.runnable());
    result["next_action_id"] = json!(next.map(|action| &action.next_action_id));
    result["next_action_argv"] = json!(next.and_then(|action| action.next_action_argv.as_ref()));
    result["queue_actions"] = json!(actions);
}

pub(super) fn add_fields(result: &mut Value, file: &Path, content: &str) {
    let actions = file
        .canonicalize()
        .context("canonicalize queue-action document")
        .and_then(|file| project_actions(&file, content));
    match actions {
        Ok(actions) => attach_actions(result, actions),
        Err(error) => {
            // A projection failure cannot undo a successful admit/finalize.
            attach_actions(result, Vec::new());
            result["queue_action_error"] = json!(format!("{error:#}"));
        }
    }
}

fn update_receipt(
    file: &Path,
    id: &str,
    expected: Option<Receipt>,
    update: Receipt,
) -> Result<Option<Receipt>> {
    let root = agent_doc_fs::find_project_root(file).context("no agent-doc project root")?;
    let mut conn = state_store::open_state_db(&root)?;
    state_store::update_project_runtime_state_in_transaction(
        &mut conn,
        &receipt_key(file, id),
        |_, current| {
            let current = receipt(current)?;
            let next = (current == expected)
                .then(|| serde_json::to_string(&update))
                .transpose()?;
            Ok((next, current))
        },
    )
}

fn load_receipt(file: &Path, id: &str) -> Result<Option<Receipt>> {
    let root = agent_doc_fs::find_project_root(file).context("no agent-doc project root")?;
    let conn = state_store::open_state_db(&root)?;
    receipt(state_store::load_project_runtime_state_from_db(
        &conn,
        &receipt_key(file, id),
    )?)
}

fn existing_action(actions: Vec<Action>, id: &str) -> Result<Action> {
    actions
        .into_iter()
        .find(|action| action.next_action_id == id)
        .context(
            "queue action is stale or does not belong to this document; use the current action",
        )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handback {
    response: String,
    #[serde(default)]
    prose_only: bool,
    source_repo: Option<String>,
    source_sha: Option<String>,
    sink_repo: Option<String>,
    sink_ref: Option<String>,
}

impl Handback {
    fn append(self, argv: &mut Vec<String>) -> Result<()> {
        if self.response.trim().is_empty() {
            bail!("worker report requires the complete nonempty response");
        }
        let repo_args = [
            ("--source-repo", self.source_repo),
            ("--source-sha", self.source_sha),
            ("--sink-repo", self.sink_repo),
            ("--sink-ref", self.sink_ref),
        ];
        if self.prose_only {
            if repo_args.iter().any(|(_, value)| value.is_some()) {
                bail!("a prose-only worker report cannot include repository evidence");
            }
            argv.push("--prose-only".into());
        } else if repo_args
            .iter()
            .any(|(_, value)| value.as_deref().is_none_or(|value| value.trim().is_empty()))
        {
            bail!(
                "repository worker reports require source_repo, source_sha, sink_repo, and sink_ref"
            );
        }
        argv.extend(
            repo_args
                .into_iter()
                .filter_map(|(flag, value)| value.map(|value| [flag.into(), value]))
                .flatten(),
        );
        argv.extend(["--response".into(), self.response]);
        Ok(())
    }
}

fn render_command(
    action: &Action,
    worker_id: Option<&str>,
    report: Option<Handback>,
) -> Result<Vec<String>> {
    let mut argv = action
        .next_action_argv
        .clone()
        .context("action requires a closeout decision")?;
    match action.kind {
        Kind::Start => {
            let worker_id = worker_id
                .filter(|id| !id.trim().is_empty())
                .context("start action requires the native worker_id")?;
            *argv.last_mut().expect("start has worker id slot") = worker_id.into();
        }
        Kind::Refresh => {
            let expected = action
                .handoff
                .as_ref()
                .and_then(|handoff| handoff["live_worker_id_required"].as_str());
            if worker_id.is_none() || worker_id != expected {
                bail!(
                    "refresh action requires the exact worker_id after the harness confirms it is live"
                );
            }
        }
        Kind::Report => {
            let expected = action
                .handoff
                .as_ref()
                .and_then(|handoff| handoff["worker_id"].as_str());
            if worker_id.is_none() || worker_id != expected {
                bail!("report action requires the exact native worker_id");
            }
            report
                .context("report action requires the native worker's complete handback")?
                .append(&mut argv)?;
        }
        _ if worker_id.is_some() => bail!("this queue action does not take worker_id"),
        _ => {}
    }
    Ok(argv)
}

fn run_command(argv: &[String]) -> Result<()> {
    let executable = match argv.first().map(String::as_str) {
        Some("agent-doc")
            if argv.get(1).is_some_and(|arg| arg == "queue")
                && matches!(
                    argv.get(2).map(String::as_str),
                    Some(
                        "claim"
                            | "start"
                            | "report-complete"
                            | "lineage"
                            | "lineage-record"
                            | "consume"
                    )
                ) =>
        {
            std::env::current_exe()?.into_os_string()
        }
        Some("git")
            if argv.get(1).is_some_and(|arg| arg == "-C")
                && argv.get(3).is_some_and(|arg| arg == "merge-base")
                && argv.get(4).is_some_and(|arg| arg == "--is-ancestor") =>
        {
            "git".into()
        }
        _ => bail!("connected queue action has no internal executor"),
    };
    let output = Command::new(executable).args(&argv[1..]).output()?;
    if !output.status.success() {
        bail!(
            "{}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub(super) fn tool(args: &serde_json::Map<String, Value>) -> Result<Value> {
    super::ensure_mcp_binary_fresh_for_mutation()?;
    let file = super::required_path_arg(args, "file")?.canonicalize()?;
    let id = super::optional_string_arg(args, "next_action_id")?;
    let worker_id = super::optional_string_arg(args, "worker_id")?;
    let outcome = super::optional_string_arg(args, "outcome")?;
    let reason = super::optional_string_arg(args, "reason")?;
    let report: Option<Handback> = args
        .get("report")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?;
    if outcome.is_some() && (worker_id.is_some() || report.is_some()) {
        bail!("an action outcome cannot include a new worker binding or report");
    }
    let mut result = json!({ "ok": true, "file": file });
    if let Some(id) = id {
        if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid next_action_id");
        }
        let previous = load_receipt(&file, &id)?;
        let receipt = if let Some(outcome) = outcome.as_deref() {
            record_outcome(&file, &id, previous, outcome, reason)?
        } else {
            run_action(
                &file,
                &id,
                previous,
                worker_id.as_deref(),
                report,
                &mut result,
            )?
        };
        result["receipt"] = json!(receipt);
    } else if worker_id.is_some() || outcome.is_some() || reason.is_some() || report.is_some() {
        bail!("queue action inputs require next_action_id");
    }
    match super::read_document(&file, None) {
        Ok(content) => add_fields(&mut result, &file, &content),
        Err(error) => result["queue_action_error"] = json!(format!("{error:#}")),
    }
    Ok(super::tool_success_result(
        serde_json::to_string_pretty(&result)?,
        result,
    ))
}

fn settle(file: &Path, id: &str, previous: Receipt, next: Receipt) -> Result<Receipt> {
    let seen = update_receipt(file, id, Some(previous.clone()), next.clone())?;
    Ok(seen.filter(|seen| *seen != previous).unwrap_or(next))
}

fn record_outcome(
    file: &Path,
    id: &str,
    previous: Option<Receipt>,
    outcome: &str,
    reason: Option<String>,
) -> Result<Receipt> {
    let Some(previous) = previous else {
        bail!("outcome must settle a reserved action");
    };
    if matches!(previous, Receipt::Applied | Receipt::Failed { .. }) {
        return Ok(previous);
    }
    match (&previous, outcome) {
        (Receipt::ExternalReserved, "applied") => settle(file, id, previous, Receipt::Applied),
        (Receipt::ExternalReserved | Receipt::SpawnReserved, "failed") => {
            let reason = reason
                .filter(|reason| !reason.trim().is_empty())
                .context("a failed queue action needs its observed reason")?;
            settle(file, id, previous, Receipt::Failed { reason })
        }
        _ => {
            bail!("outcome must settle a reserved external command or record a failed native spawn")
        }
    }
}

fn run_action(
    file: &Path,
    id: &str,
    previous: Option<Receipt>,
    worker_id: Option<&str>,
    report: Option<Handback>,
    result: &mut Value,
) -> Result<Receipt> {
    if let Some(existing) = &previous
        && (*existing != Receipt::SpawnReserved || worker_id.is_none())
    {
        return Ok(existing.clone());
    }
    let content = super::read_document(file, None)?;
    let action = existing_action(project_actions(file, &content)?, id)?;
    if report.is_some() && action.kind != Kind::Report {
        bail!("this queue action does not accept a worker report");
    }
    if action.kind == Kind::Start && worker_id.is_none() {
        if agent_doc_queue_io::address_effect::github_enabled()
            && agent_doc_queue::address_completion_proof::address_review_target(&action.item)
                .is_some()
        {
            let ledger = queue_claim::load_ledger_following_read_only(file, &content)?;
            let claim = ledger
                .resolve_selector(&action.item)?
                .context("address evidence requires the exact reserved queue claim")?;
            agent_doc_queue_io::address_effect::require_worker_observation(file, claim)?;
        }
        if let Some(receipt) = update_receipt(file, id, None, Receipt::SpawnReserved)? {
            return Ok(receipt);
        }
        result["spawn"] = json!(action.spawn);
        return Ok(Receipt::SpawnReserved);
    }
    if action.kind == Kind::Start && previous != Some(Receipt::SpawnReserved) {
        bail!("reserve the start action before spawning a native worker");
    }
    let argv = render_command(&action, worker_id, report)?;
    if action.kind == Kind::ExternalCloseout {
        if let Some(receipt) = update_receipt(file, id, None, Receipt::ExternalReserved)? {
            return Ok(receipt);
        }
        // The caller presents outward operations to its own permission system
        // and reports their result. Replays cannot acquire the command again.
        result["execute_argv"] = json!(argv);
        return Ok(Receipt::ExternalReserved);
    }
    let seen = update_receipt(file, id, previous.clone(), Receipt::Started)?;
    if seen != previous {
        return seen.context("queue action receipt disappeared");
    }
    let next = match run_command(&argv) {
        Ok(()) => Receipt::Applied,
        Err(error) => Receipt::Failed {
            reason: format!("{error:#}"),
        },
    };
    settle(file, id, Receipt::Started, next)
}
