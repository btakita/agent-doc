//! File-backed prompt-cache effectiveness history I/O.
//!
//! Pure prompt-cache boundary, replay-key, ranking, and trend policy lives in
//! `agent_doc_prompt_cache`. This crate only adapts those types to JSONL
//! history files.

use agent_doc_prompt_cache::PromptCacheEffectivenessSample;
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

pub fn append_prompt_cache_effectiveness_sample(
    path: impl AsRef<Path>,
    sample: &PromptCacheEffectivenessSample,
) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create prompt-cache history dir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open prompt-cache history {}", path.display()))?;
    serde_json::to_writer(&mut file, sample)
        .with_context(|| format!("serialize prompt-cache sample for {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("write prompt-cache history newline to {}", path.display()))?;
    Ok(())
}

pub fn load_prompt_cache_effectiveness_history(
    path: impl AsRef<Path>,
) -> Result<Vec<PromptCacheEffectivenessSample>> {
    let path = path.as_ref();
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("open prompt-cache effectiveness history {}", path.display())
            });
        }
    };
    let mut samples = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line =
            line.with_context(|| format!("read prompt-cache history line {}", line_number + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let sample: PromptCacheEffectivenessSample =
            serde_json::from_str(&line).with_context(|| {
                format!(
                    "parse prompt-cache effectiveness sample {}:{}",
                    path.display(),
                    line_number + 1
                )
            })?;
        samples.push(sample);
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_prompt_cache::PromptCacheSessionCostSample;

    #[test]
    fn prompt_cache_history_persists_real_provider_pairs_as_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let history_path = dir.path().join("prompt-cache-history.jsonl");
        let codex_openai = effectiveness_sample(
            "OpenAI",
            "Codex",
            "codex-openai-real-transcript",
            "stable",
            "resume:codex-session",
            "agent=codex;model=gpt-5;mode=template",
            Some(120_000),
            Some(1_000),
        )
        .observed_at_unix_ms(1_700_000_001);
        let claude_anthropic = effectiveness_sample(
            "Anthropic",
            "Claude",
            "claude-anthropic-real-transcript",
            "stable",
            "resume:claude-session",
            "agent=claude;model=opus;mode=template",
            Some(90_000),
            Some(2_000),
        )
        .observed_at_unix_ms(1_700_000_002);

        append_prompt_cache_effectiveness_sample(&history_path, &codex_openai).unwrap();
        append_prompt_cache_effectiveness_sample(&history_path, &claude_anthropic).unwrap();

        let loaded = load_prompt_cache_effectiveness_history(&history_path).unwrap();

        assert_eq!(loaded, vec![codex_openai, claude_anthropic]);
        assert_eq!(loaded[0].provider, "openai");
        assert_eq!(loaded[0].harness, "codex");
        assert_eq!(loaded[1].provider, "anthropic");
        assert_eq!(loaded[1].harness, "claude");
    }

    fn effectiveness_sample(
        provider: &str,
        harness: &str,
        transcript_id: &str,
        stable_prefix_sha256: &str,
        adapter_state: &str,
        routing_affinity: &str,
        cached_input_tokens: Option<u64>,
        creation_tokens: Option<u64>,
    ) -> PromptCacheEffectivenessSample {
        PromptCacheEffectivenessSample::new(
            provider,
            harness,
            transcript_id,
            PromptCacheSessionCostSample {
                stable_prefix_sha256: stable_prefix_sha256.to_string(),
                adapter_state: adapter_state.to_string(),
                routing_affinity: routing_affinity.to_string(),
                cached_input_tokens,
                creation_tokens,
            },
        )
    }
}
