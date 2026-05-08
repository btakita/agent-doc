//! Test-only deterministic workflow simulator.
//!
//! The simulator deliberately stays small: it models the closeout state that is
//! cheap to exercise in memory, and delegates document semantics to production
//! parsers/classifiers wherever possible.

use anyhow::{Result, anyhow, bail};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CyclePhase {
    Idle,
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimCommand {
    EditPrompt,
    EditLaterPrompt,
    AddMalformedBacklogItem,
    CaptureResponse,
    ApplyCapturedResponse,
    Commit,
    FailCommit,
    RepairBoundary,
    DuplicateVisibleResponse,
}

#[derive(Debug, Default)]
struct Coverage {
    unresolved_prompt_blocks: usize,
    malformed_backlog_blocks: usize,
    uncommitted_response_blocks: usize,
    duplicate_patchback_blocks: usize,
    boundary_repairs: usize,
    commits: usize,
}

impl Coverage {
    fn record_block(&mut self, message: &str) {
        if message.contains("unresolved prompt_target") {
            self.unresolved_prompt_blocks += 1;
        }
        if message.contains("malformed tracked checklist item") {
            self.malformed_backlog_blocks += 1;
        }
        if message.contains("response captured but not committed")
            || message.contains("response write applied but not committed")
        {
            self.uncommitted_response_blocks += 1;
        }
        if message.contains("duplicate response patchback") {
            self.duplicate_patchback_blocks += 1;
        }
    }

    fn merge(&mut self, other: Coverage) {
        self.unresolved_prompt_blocks += other.unresolved_prompt_blocks;
        self.malformed_backlog_blocks += other.malformed_backlog_blocks;
        self.uncommitted_response_blocks += other.uncommitted_response_blocks;
        self.duplicate_patchback_blocks += other.duplicate_patchback_blocks;
        self.boundary_repairs += other.boundary_repairs;
        self.commits += other.commits;
    }
}

#[derive(Debug)]
struct SimWorld {
    seed: u64,
    trace: Vec<SimCommand>,
    doc: String,
    snapshot: String,
    phase: CyclePhase,
    captured_response: Option<String>,
    next_prompt: usize,
    coverage: Coverage,
}

impl SimWorld {
    fn new(seed: u64) -> Self {
        let doc = template_doc("");
        Self {
            seed,
            trace: Vec::new(),
            snapshot: doc.clone(),
            doc,
            phase: CyclePhase::Idle,
            captured_response: None,
            next_prompt: 1,
            coverage: Coverage::default(),
        }
    }

    fn run_seed(seed: u64, steps: usize) -> Result<Coverage> {
        let mut rng = DeterministicRng::new(seed);
        let mut world = Self::new(seed);
        for _ in 0..steps {
            let command = match rng.next_usize(9) {
                0 => SimCommand::EditPrompt,
                1 => SimCommand::EditLaterPrompt,
                2 => SimCommand::AddMalformedBacklogItem,
                3 => SimCommand::CaptureResponse,
                4 => SimCommand::ApplyCapturedResponse,
                5 => SimCommand::Commit,
                6 => SimCommand::FailCommit,
                7 => SimCommand::RepairBoundary,
                _ => SimCommand::DuplicateVisibleResponse,
            };
            world.apply(command)?;
            world.assert_structural_invariants()?;
        }
        if let Err(err) = world.strict_closeout_invariants() {
            world.coverage.record_block(&err.to_string());
        }
        Ok(world.coverage)
    }

    fn apply(&mut self, command: SimCommand) -> Result<()> {
        self.trace.push(command);
        match command {
            SimCommand::EditPrompt => {
                let prompt = format!(
                    "❯ do #sim{}. spec-test-build-install-commit-push\n",
                    self.next_prompt
                );
                self.next_prompt += 1;
                self.append_to_exchange(&prompt)?;
                if matches!(self.phase, CyclePhase::Idle | CyclePhase::Committed) {
                    self.phase = CyclePhase::PreflightStarted;
                }
            }
            SimCommand::EditLaterPrompt => {
                let prompt = format!("❯ later follow-up #sim{}\n", self.next_prompt);
                self.next_prompt += 1;
                self.append_to_exchange(&prompt)?;
            }
            SimCommand::AddMalformedBacklogItem => {
                self.replace_component_content(
                    "backlog",
                    "_- [ ] [#tigersim] malformed prefix keeps the item parse-hidden\n",
                )?;
            }
            SimCommand::CaptureResponse => {
                self.captured_response = Some(response_patch("sim closeout"));
                self.phase = CyclePhase::ResponseCaptured;
            }
            SimCommand::ApplyCapturedResponse => {
                self.apply_captured_response()?;
            }
            SimCommand::Commit => match self.try_commit() {
                Ok(()) => self.coverage.commits += 1,
                Err(err) => self.coverage.record_block(&err.to_string()),
            },
            SimCommand::FailCommit => {
                if matches!(
                    self.phase,
                    CyclePhase::ResponseCaptured | CyclePhase::WriteApplied
                ) {
                    let message = self.strict_closeout_invariants().unwrap_err().to_string();
                    self.coverage.record_block(&message);
                }
            }
            SimCommand::RepairBoundary => {
                self.doc = crate::template::reposition_boundary_to_end_clean_with_id(
                    &self.doc,
                    Some("sim-boundary"),
                );
                self.coverage.boundary_repairs += 1;
            }
            SimCommand::DuplicateVisibleResponse => {
                let duplicate = "### Re: sim closeout — gpt-5\n\nDuplicate visible response.\n";
                self.append_to_exchange(duplicate)?;
            }
        }
        Ok(())
    }

    fn apply_captured_response(&mut self) -> Result<()> {
        let response = self.captured_response.clone().unwrap_or_default();
        if response.is_empty() {
            return Ok(());
        }
        let (patches, unmatched) = crate::template::parse_patches(&response)?;
        self.doc =
            crate::template::apply_patches(&self.doc, &patches, &unmatched, Path::new("sim.md"))?;
        self.phase = CyclePhase::WriteApplied;
        Ok(())
    }

    fn try_commit(&mut self) -> Result<()> {
        self.strict_closeout_invariants()?;
        self.doc =
            crate::template::reposition_boundary_to_end_clean_with_id(&self.doc, Some("committed"));
        self.snapshot = self.doc.clone();
        self.phase = CyclePhase::Committed;
        Ok(())
    }

    fn strict_closeout_invariants(&self) -> Result<()> {
        match self.phase {
            CyclePhase::ResponseCaptured => {
                bail!(
                    "response captured but not committed; seed={} trace={:?}",
                    self.seed,
                    self.trace
                )
            }
            CyclePhase::WriteApplied => {
                if self.has_duplicate_response_heading() {
                    bail!(
                        "duplicate response patchback before commit; seed={} trace={:?}",
                        self.seed,
                        self.trace
                    );
                }
            }
            _ => {}
        }

        let components = crate::component::parse(&self.doc)?;
        let malformed = components
            .iter()
            .filter(|component| crate::component::is_tracked_work_component(&component.name))
            .flat_map(|component| {
                crate::pending::detect_malformed_item_lines(component.content(&self.doc))
            })
            .collect::<Vec<_>>();
        if !malformed.is_empty() {
            let refs = malformed
                .iter()
                .map(|item| item.reference())
                .collect::<Vec<_>>()
                .join(", ");
            bail!("malformed tracked checklist item(s): {refs}");
        }

        if let Some(diff_text) = crate::diff::unified_diff_from_contents(&self.snapshot, &self.doc)
        {
            let prompt_targets = crate::diff::classify_prompt_bearing_changes(&diff_text)
                .into_iter()
                .filter(|change| {
                    matches!(
                        change.kind,
                        crate::diff::PromptBearingChangeKind::PromptTarget
                    )
                })
                .map(|change| change.text)
                .collect::<Vec<_>>();
            if !prompt_targets.is_empty() {
                bail!(
                    "unresolved prompt_target(s) after closeout: {}; seed={} trace={:?}",
                    prompt_targets.join(" | "),
                    self.seed,
                    self.trace
                );
            }
        }

        if matches!(self.phase, CyclePhase::WriteApplied) {
            bail!(
                "response write applied but not committed; seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        Ok(())
    }

    fn assert_structural_invariants(&self) -> Result<()> {
        let components = crate::component::parse(&self.doc)?;
        let exchange = components
            .iter()
            .find(|component| component.name == "exchange")
            .ok_or_else(|| anyhow!("sim document lost exchange component"))?;
        let exchange_body = exchange.content(&self.doc);
        let boundary_count = exchange_body.matches("<!-- agent:boundary:").count();
        if boundary_count > 1 {
            bail!(
                "exchange has multiple boundary markers: count={boundary_count}; seed={} trace={:?}",
                self.seed,
                self.trace
            );
        }
        Ok(())
    }

    fn has_duplicate_response_heading(&self) -> bool {
        self.doc.matches("### Re: sim closeout").count() > 1
    }

    fn append_to_exchange(&mut self, text: &str) -> Result<()> {
        let body = self.component_content("exchange")?.to_string();
        let boundary = body.find("<!-- agent:boundary:");
        let next = if let Some(pos) = boundary {
            format!("{}{}{}", &body[..pos], text, &body[pos..])
        } else {
            format!("{body}{text}")
        };
        self.replace_component_content("exchange", &next)
    }

    fn component_content(&self, name: &str) -> Result<&str> {
        crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == name)
            .map(|component| component.content(&self.doc))
            .ok_or_else(|| anyhow!("missing component `{name}`"))
    }

    fn replace_component_content(&mut self, name: &str, content: &str) -> Result<()> {
        let component = crate::component::parse(&self.doc)?
            .into_iter()
            .find(|component| component.name == name)
            .ok_or_else(|| anyhow!("missing component `{name}`"))?;
        self.doc = component.replace_content(&self.doc, content);
        Ok(())
    }
}

#[derive(Debug)]
struct DeterministicRng(u64);

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next_usize(&mut self, modulo: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 32) as usize) % modulo
    }
}

fn template_doc(exchange_body: &str) -> String {
    format!(
        "---\nagent_doc_session: sim\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
         ## Exchange\n\n\
         <!-- agent:exchange patch=append -->\n\
         {exchange_body}<!-- agent:boundary:initial -->\n\
         <!-- /agent:exchange -->\n\n\
         ## Pending / Not Built\n\n\
         <!-- agent:backlog -->\n\
         - [ ] [#tigersim] Implement the simulator MVP\n\
         <!-- /agent:backlog -->\n\n\
         <!-- agent:icebox -->\n\
         <!-- /agent:icebox -->\n"
    )
}

fn response_patch(topic: &str) -> String {
    format!(
        "<!-- patch:exchange -->\n### Re: {topic} — gpt-5\n\nImplemented and verified.\n<!-- /patch:exchange -->\n"
    )
}

#[test]
fn closeout_sim_fixed_seed_corpus_exercises_recent_failure_classes() {
    let mut coverage = Coverage::default();
    for seed in 0..512 {
        let seed_coverage = SimWorld::run_seed(seed, 24).unwrap_or_else(|err| {
            panic!("seed {seed} failed structurally: {err}");
        });
        coverage.merge(seed_coverage);
    }

    assert!(
        coverage.commits > 0,
        "seed corpus must include valid committed closeouts"
    );
    assert!(
        coverage.boundary_repairs > 0,
        "seed corpus must exercise deterministic boundary repair"
    );
    assert!(
        coverage.unresolved_prompt_blocks > 0,
        "seed corpus must exercise unresolved prompt closeout blocks"
    );
    assert!(
        coverage.malformed_backlog_blocks > 0,
        "seed corpus must exercise malformed backlog closeout blocks"
    );
    assert!(
        coverage.uncommitted_response_blocks > 0,
        "seed corpus must exercise captured/write-applied uncommitted closeout blocks"
    );
    assert!(
        coverage.duplicate_patchback_blocks > 0,
        "seed corpus must exercise duplicate visible response blocks"
    );
}

#[test]
fn closeout_sim_blocks_later_prompt_after_response_write() {
    let mut world = SimWorld::new(42);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::EditLaterPrompt).unwrap();

    let err = world.try_commit().unwrap_err();
    assert!(
        err.to_string().contains("unresolved prompt_target"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_blocks_malformed_tracked_backlog_line() {
    let mut world = SimWorld::new(7);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();
    world.apply(SimCommand::ApplyCapturedResponse).unwrap();
    world.apply(SimCommand::AddMalformedBacklogItem).unwrap();

    let err = world.try_commit().unwrap_err();
    assert!(
        err.to_string().contains("malformed tracked checklist item"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_requires_captured_response_to_cross_commit_boundary() {
    let mut world = SimWorld::new(11);
    world.apply(SimCommand::EditPrompt).unwrap();
    world.apply(SimCommand::CaptureResponse).unwrap();

    let err = world.strict_closeout_invariants().unwrap_err();
    assert!(
        err.to_string()
            .contains("response captured but not committed"),
        "unexpected closeout error: {err}"
    );
}

#[test]
fn closeout_sim_collapses_boundary_drift_to_single_marker() {
    let mut world = SimWorld::new(99);
    world
        .append_to_exchange("<!-- agent:boundary:stale-one -->\n")
        .unwrap();
    world
        .append_to_exchange("<!-- agent:boundary:stale-two -->\n")
        .unwrap();

    world.apply(SimCommand::RepairBoundary).unwrap();
    world.assert_structural_invariants().unwrap();
    assert!(world.doc.contains("<!-- agent:boundary:sim-boundary -->"));
}
