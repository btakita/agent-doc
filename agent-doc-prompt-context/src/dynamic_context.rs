//! Pure dynamic-context projection for prompt assembly.
//!
//! Callers own durable storage and tsift invocation. This module only projects
//! already-loaded document state, candidate pack metadata, and context-ledger
//! rows into a rendered manifest with duplicate decisions.

use std::cell::Cell as StdCell;
use std::collections::HashSet;
use std::rc::Rc;

use lazily::{Compute, Computed, Context, Source};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComponentHash {
    pub name: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromptTargetInputs {
    pub preflight_targets: Vec<String>,
    pub plan_targets: Vec<String>,
    pub queue_heads: Vec<String>,
    pub backlog_heads: Vec<String>,
    pub review_heads: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextChunk {
    pub pack_id: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub source_uri: String,
    pub range_start: Option<usize>,
    pub range_end: Option<usize>,
    pub token_count: usize,
    pub stale: bool,
}

impl ContextChunk {
    pub fn new(pack_id: &str, chunk_id: &str, content_hash: &str, source_uri: &str) -> Self {
        Self {
            pack_id: pack_id.to_string(),
            chunk_id: chunk_id.to_string(),
            content_hash: content_hash.to_string(),
            source_uri: source_uri.to_string(),
            range_start: None,
            range_end: None,
            token_count: 0,
            stale: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextInjectionRecord {
    pub document_id: String,
    pub session_id: String,
    pub cycle_id: String,
    pub pack_id: String,
    pub chunk_id: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InjectionLedgerSnapshot {
    pub records: Vec<ContextInjectionRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionMode {
    Expanded,
    Referenced,
    SkippedDuplicate,
    StaleIgnored,
}

impl InjectionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InjectionMode::Expanded => "expanded",
            InjectionMode::Referenced => "referenced",
            InjectionMode::SkippedDuplicate => "skipped_duplicate",
            InjectionMode::StaleIgnored => "stale_ignored",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextChunkDecision {
    pub pack_id: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub source_uri: String,
    pub range_start: Option<usize>,
    pub range_end: Option<usize>,
    pub token_count: usize,
    pub injection_mode: InjectionMode,
}

impl ContextChunkDecision {
    fn from_chunk(chunk: ContextChunk, injection_mode: InjectionMode) -> Self {
        Self {
            pack_id: chunk.pack_id,
            chunk_id: chunk.chunk_id,
            content_hash: chunk.content_hash,
            source_uri: chunk.source_uri,
            range_start: chunk.range_start,
            range_end: chunk.range_end,
            token_count: chunk.token_count,
            injection_mode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedContextManifest {
    pub prompt_targets: Vec<String>,
    pub decisions: Vec<ContextChunkDecision>,
    pub prompt_fingerprint: String,
    pub token_count: usize,
}

impl RenderedContextManifest {
    pub fn expanded_chunk_ids(&self) -> Vec<String> {
        self.decisions
            .iter()
            .filter(|decision| decision.injection_mode == InjectionMode::Expanded)
            .map(|decision| decision.chunk_id.clone())
            .collect()
    }

    pub fn as_manifest_text(&self) -> String {
        let mut lines = vec![
            format!(
                "context_manifest fingerprint={} token_count={}",
                self.prompt_fingerprint, self.token_count
            ),
            "prompt_targets:".to_string(),
        ];
        for target in &self.prompt_targets {
            lines.push(format!("- {}", target));
        }
        lines.push("chunks:".to_string());
        for decision in &self.decisions {
            let range = match (decision.range_start, decision.range_end) {
                (Some(start), Some(end)) => format!(":{start}-{end}"),
                _ => String::new(),
            };
            lines.push(format!(
                "- {} {}{} mode={} hash={}",
                decision.chunk_id,
                decision.source_uri,
                range,
                decision.injection_mode.as_str(),
                decision.content_hash
            ));
        }
        lines.join("\n")
    }
}

type ComponentHashesCell = Source<Vec<ComponentHash>>;
type PromptInputsCell = Source<PromptTargetInputs>;
type CandidateChunksCell = Source<Vec<ContextChunk>>;
type InjectionLedgerCell = Source<InjectionLedgerSnapshot>;
type PromptTargetsSlot = Computed<Vec<String>>;
type CandidateChunksSlot = Computed<Vec<ContextChunk>>;
type DecisionsSlot = Computed<Vec<ContextChunkDecision>>;
type RenderedManifestSlot = Computed<RenderedContextManifest>;

pub struct DynamicContextProjection {
    ctx: Context,
    component_hashes: ComponentHashesCell,
    prompt_inputs: PromptInputsCell,
    candidate_chunks: CandidateChunksCell,
    ledger_snapshot: InjectionLedgerCell,
    prompt_targets: PromptTargetsSlot,
    candidate_context_chunks: CandidateChunksSlot,
    duplicate_decisions: DecisionsSlot,
    rendered_manifest: RenderedManifestSlot,
    render_count: Rc<StdCell<usize>>,
}

impl DynamicContextProjection {
    pub fn new(
        component_hashes: Vec<ComponentHash>,
        prompt_inputs: PromptTargetInputs,
        candidate_chunks: Vec<ContextChunk>,
        ledger_snapshot: InjectionLedgerSnapshot,
    ) -> Self {
        Self::with_render_count(
            component_hashes,
            prompt_inputs,
            candidate_chunks,
            ledger_snapshot,
            Rc::new(StdCell::new(0)),
        )
    }

    fn with_render_count(
        component_hashes: Vec<ComponentHash>,
        prompt_inputs: PromptTargetInputs,
        candidate_chunks: Vec<ContextChunk>,
        ledger_snapshot: InjectionLedgerSnapshot,
        render_count: Rc<StdCell<usize>>,
    ) -> Self {
        let ctx = Context::new();
        let component_hashes_cell = ctx.source(component_hashes);
        let prompt_inputs_cell = ctx.source(prompt_inputs);
        let candidate_chunks_cell = ctx.source(candidate_chunks);
        let ledger_snapshot_cell = ctx.source(ledger_snapshot);

        let prompt_targets = ctx.computed({
            let input = prompt_inputs_cell;
            move |ctx: &Compute| derive_prompt_targets(&ctx.get(&input))
        });

        let candidate_context_chunks = ctx.computed({
            let candidates = candidate_chunks_cell;
            move |ctx: &Compute| normalize_candidate_chunks(ctx.get(&candidates))
        });

        let duplicate_decisions = ctx.computed({
            let candidates = candidate_context_chunks;
            let ledger = ledger_snapshot_cell;
            move |ctx: &Compute| {
                decide_context_injections(ctx.get(&candidates), &ctx.get(&ledger))
            }
        });

        let rendered_manifest = ctx.computed({
            let components = component_hashes_cell;
            let targets = prompt_targets;
            let decisions = duplicate_decisions;
            let render_count = Rc::clone(&render_count);
            move |ctx: &Compute| {
                render_count.set(render_count.get() + 1);
                render_context_manifest(
                    ctx.get(&components),
                    ctx.get(&targets),
                    ctx.get(&decisions),
                )
            }
        });

        Self {
            ctx,
            component_hashes: component_hashes_cell,
            prompt_inputs: prompt_inputs_cell,
            candidate_chunks: candidate_chunks_cell,
            ledger_snapshot: ledger_snapshot_cell,
            prompt_targets,
            candidate_context_chunks,
            duplicate_decisions,
            rendered_manifest,
            render_count,
        }
    }

    pub fn set_component_hashes(&self, component_hashes: Vec<ComponentHash>) {
        self.ctx.set(
            &self.component_hashes,
            normalize_component_hashes(component_hashes),
        );
    }

    pub fn set_prompt_inputs(&self, prompt_inputs: PromptTargetInputs) {
        self.ctx.set(&self.prompt_inputs, prompt_inputs);
    }

    pub fn set_candidate_chunks(&self, candidate_chunks: Vec<ContextChunk>) {
        self.ctx.set(&self.candidate_chunks, candidate_chunks);
    }

    pub fn set_ledger_snapshot(&self, ledger_snapshot: InjectionLedgerSnapshot) {
        self.ctx.set(&self.ledger_snapshot, ledger_snapshot);
    }

    pub fn prompt_targets(&self) -> Vec<String> {
        self.ctx.get(&self.prompt_targets)
    }

    pub fn candidate_context_chunks(&self) -> Vec<ContextChunk> {
        self.ctx.get(&self.candidate_context_chunks)
    }

    pub fn duplicate_decisions(&self) -> Vec<ContextChunkDecision> {
        self.ctx.get(&self.duplicate_decisions)
    }

    pub fn rendered_manifest(&self) -> RenderedContextManifest {
        self.ctx.get(&self.rendered_manifest)
    }

    pub fn rendered_manifest_count(&self) -> usize {
        self.render_count.get()
    }
}

fn derive_prompt_targets(input: &PromptTargetInputs) -> Vec<String> {
    let mut targets = if !input.preflight_targets.is_empty() {
        input.preflight_targets.clone()
    } else if !input.plan_targets.is_empty() {
        input.plan_targets.clone()
    } else {
        input
            .queue_heads
            .iter()
            .chain(input.backlog_heads.iter())
            .chain(input.review_heads.iter())
            .cloned()
            .collect()
    };
    normalize_strings(&mut targets);
    targets
}

fn normalize_candidate_chunks(mut chunks: Vec<ContextChunk>) -> Vec<ContextChunk> {
    chunks.sort_by(|a, b| {
        a.pack_id
            .cmp(&b.pack_id)
            .then_with(|| a.chunk_id.cmp(&b.chunk_id))
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });
    chunks.dedup_by(|a, b| {
        a.pack_id == b.pack_id
            && a.chunk_id == b.chunk_id
            && a.content_hash == b.content_hash
            && a.source_uri == b.source_uri
    });
    chunks
}

fn normalize_component_hashes(mut hashes: Vec<ComponentHash>) -> Vec<ComponentHash> {
    hashes.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.content_hash.cmp(&b.content_hash))
    });
    hashes.dedup();
    hashes
}

fn normalize_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    let normalized = values
        .iter()
        .filter_map(|value| {
            let normalized = value.trim().to_string();
            if normalized.is_empty() || !seen.insert(normalized.clone()) {
                None
            } else {
                Some(normalized)
            }
        })
        .collect();
    *values = normalized;
}

fn decide_context_injections(
    chunks: Vec<ContextChunk>,
    ledger: &InjectionLedgerSnapshot,
) -> Vec<ContextChunkDecision> {
    let chunk_ids = ledger
        .records
        .iter()
        .map(|record| record.chunk_id.as_str())
        .collect::<HashSet<_>>();
    let content_hashes = ledger
        .records
        .iter()
        .map(|record| record.content_hash.as_str())
        .collect::<HashSet<_>>();

    chunks
        .into_iter()
        .map(|chunk| {
            let mode = if chunk.stale || chunk.content_hash.trim().is_empty() {
                InjectionMode::StaleIgnored
            } else if chunk_ids.contains(chunk.chunk_id.as_str()) {
                InjectionMode::Referenced
            } else if content_hashes.contains(chunk.content_hash.as_str()) {
                InjectionMode::SkippedDuplicate
            } else {
                InjectionMode::Expanded
            };
            ContextChunkDecision::from_chunk(chunk, mode)
        })
        .collect()
}

fn render_context_manifest(
    component_hashes: Vec<ComponentHash>,
    prompt_targets: Vec<String>,
    decisions: Vec<ContextChunkDecision>,
) -> RenderedContextManifest {
    let token_count = decisions
        .iter()
        .filter(|decision| decision.injection_mode == InjectionMode::Expanded)
        .map(|decision| decision.token_count)
        .sum();
    let fingerprint_input = format!(
        "components={component_hashes:?}\nprompt_targets={prompt_targets:?}\ndecisions={decisions:?}"
    );
    RenderedContextManifest {
        prompt_targets,
        decisions,
        token_count,
        prompt_fingerprint: agent_doc_hash::content_hash(&fingerprint_input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component_hash(name: &str, content_hash: &str) -> ComponentHash {
        ComponentHash {
            name: name.to_string(),
            content_hash: content_hash.to_string(),
        }
    }

    fn record(chunk_id: &str, content_hash: &str) -> ContextInjectionRecord {
        ContextInjectionRecord {
            document_id: "doc".to_string(),
            session_id: "session".to_string(),
            cycle_id: "cycle".to_string(),
            pack_id: "pack".to_string(),
            chunk_id: chunk_id.to_string(),
            content_hash: content_hash.to_string(),
        }
    }

    fn prompt_inputs(targets: &[&str]) -> PromptTargetInputs {
        PromptTargetInputs {
            preflight_targets: targets.iter().map(|target| target.to_string()).collect(),
            ..PromptTargetInputs::default()
        }
    }

    fn projection_with_count(
        component_hashes: Vec<ComponentHash>,
        prompt_inputs: PromptTargetInputs,
        candidate_chunks: Vec<ContextChunk>,
        ledger_snapshot: InjectionLedgerSnapshot,
        render_count: Rc<StdCell<usize>>,
    ) -> DynamicContextProjection {
        DynamicContextProjection::with_render_count(
            component_hashes,
            prompt_inputs,
            candidate_chunks,
            ledger_snapshot,
            render_count,
        )
    }

    #[test]
    fn unchanged_component_hashes_do_not_recompute_rendered_manifest() {
        let count = Rc::new(StdCell::new(0));
        let projection = projection_with_count(
            vec![component_hash("exchange", "abc")],
            prompt_inputs(&["do #ctx"]),
            vec![ContextChunk::new("pack", "chunk-a", "hash-a", "src/a.md")],
            InjectionLedgerSnapshot::default(),
            Rc::clone(&count),
        );

        let first = projection.rendered_manifest();
        assert_eq!(count.get(), 1);

        projection.set_component_hashes(vec![component_hash("exchange", "abc")]);
        let second = projection.rendered_manifest();

        assert_eq!(second, first);
        assert_eq!(
            count.get(),
            1,
            "same component hash input must not invalidate the rendered manifest"
        );
    }

    #[test]
    fn changed_candidate_chunk_hash_changes_only_affected_decision() {
        let projection = DynamicContextProjection::new(
            vec![component_hash("exchange", "abc")],
            prompt_inputs(&["do #ctx"]),
            vec![
                ContextChunk::new("pack", "chunk-a", "hash-a", "src/a.md"),
                ContextChunk::new("pack", "chunk-b", "hash-b", "src/b.md"),
            ],
            InjectionLedgerSnapshot {
                records: vec![record("old-other-id", "hash-b")],
            },
        );

        let first = projection.duplicate_decisions();
        assert_eq!(
            first
                .iter()
                .find(|decision| decision.chunk_id == "chunk-b")
                .map(|decision| decision.injection_mode),
            Some(InjectionMode::SkippedDuplicate)
        );

        projection.set_candidate_chunks(vec![
            ContextChunk::new("pack", "chunk-a", "hash-a", "src/a.md"),
            ContextChunk::new("pack", "chunk-b", "hash-c", "src/b.md"),
        ]);
        let second = projection.duplicate_decisions();

        let changed = first
            .iter()
            .zip(second.iter())
            .filter(|(before, after)| before != after)
            .map(|(_, after)| after.chunk_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(changed, vec!["chunk-b"]);
        assert_eq!(
            second
                .iter()
                .find(|decision| decision.chunk_id == "chunk-a")
                .map(|decision| decision.injection_mode),
            Some(InjectionMode::Expanded)
        );
        assert_eq!(
            second
                .iter()
                .find(|decision| decision.chunk_id == "chunk-b")
                .map(|decision| decision.injection_mode),
            Some(InjectionMode::Expanded)
        );
    }

    #[test]
    fn equal_derived_prompt_targets_preserve_memoized_manifest() {
        let count = Rc::new(StdCell::new(0));
        let projection = projection_with_count(
            vec![component_hash("exchange", "abc")],
            prompt_inputs(&["do #ctx"]),
            vec![ContextChunk::new("pack", "chunk-a", "hash-a", "src/a.md")],
            InjectionLedgerSnapshot::default(),
            Rc::clone(&count),
        );

        let first = projection.rendered_manifest();
        assert_eq!(count.get(), 1);

        projection.set_prompt_inputs(PromptTargetInputs {
            preflight_targets: vec![" do #ctx ".to_string(), "do #ctx".to_string()],
            plan_targets: vec!["ignored because preflight wins".to_string()],
            ..PromptTargetInputs::default()
        });
        let second = projection.rendered_manifest();

        assert_eq!(second, first);
        assert_eq!(
            count.get(),
            1,
            "memoized equal prompt targets must not recompute the rendered manifest"
        );
    }
}
