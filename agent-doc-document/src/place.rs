//! Contract for inline `agent:place` prompts.
//!
//! `mode=once` (the default) is due when the prompt is new or the operator
//! edits the prompt body. Writing `place:out` does not make it due again.
//! `mode=sync` follows the status-sync contract for that one place: the output
//! is derived, an editor undo demands a resync, and an optional `interval`
//! re-surfaces it on a live turn. The binary does not invent the answer.

use std::collections::BTreeMap;

use agent_doc_element::place::{PlaceRegion, PlaceStructure};
use serde::{Deserialize, Serialize};

/// Minimum opt-in cadence for a live `mode=sync` checkpoint.
pub const MIN_INTERVAL_MS: u64 = 60_000;

pub const PLACE_DIFF_TYPE: &str = "place";

pub const PLACES_GUIDANCE: &str = "\
Inline place prompts are operator instructions in `agent:place`. The prompt is the body text before `place:out`; a quoted `prompt` attribute is used only when that body is empty. Write each answer with `agent-doc place set <FILE> --id <id> --text \"<answer>\"`, which replaces only the bytes inside `place:out`. Do not edit the markers and do not paste the answer into the exchange as a second copy; a one-line receipt is enough to close the cycle. `mode=once` (the default) is answered when the prompt is new or the operator edits the prompt body; writing the output does not start another turn. `mode=sync` follows the status-sync contract for that place: the output is derived, an editor undo demands a resync even when the text matches the last write, an optional `interval` re-surfaces it during a live turn, and an unchanged answer performs no write. The binary does not invent the answer.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceMode {
    Once,
    Sync,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceAnswer {
    pub prompt_hash: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceContract {
    pub id: String,
    pub mode: PlaceMode,
    pub prompt: String,
    pub prompt_hash: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
    /// This cycle must replace `place:out`.
    pub needs_write: bool,
    /// `mode=sync` only: the output no longer matches the last write.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub revert: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceSeed {
    pub id: String,
    pub prompt_hash: String,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceAssessment {
    pub contracts: Vec<PlaceContract>,
    /// Remember a prefilled output so a later prompt edit reopens the place.
    pub seed: Vec<PlaceSeed>,
}

impl PlaceAssessment {
    pub fn needs_turn(&self) -> bool {
        self.contracts.iter().any(|contract| contract.needs_write)
    }

    /// Contracts the agent should see: due places, plus every `mode=sync` place.
    pub fn visible(&self) -> Vec<PlaceContract> {
        self.contracts
            .iter()
            .filter(|contract| contract.needs_write || contract.mode == PlaceMode::Sync)
            .cloned()
            .collect()
    }

    pub fn needs_write_fingerprint(&self) -> String {
        self.contracts
            .iter()
            .filter(|contract| contract.needs_write)
            .map(|contract| {
                format!(
                    "{}:{}:{}:{}",
                    contract.id,
                    contract.prompt_hash,
                    contract.revert,
                    agent_doc_hash::content_hash(&contract.output)
                        .chars()
                        .take(12)
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Shortest `mode=sync` interval, when any place opted into one.
    pub fn sync_interval_ms(&self) -> Option<u64> {
        self.contracts
            .iter()
            .filter(|contract| contract.mode == PlaceMode::Sync)
            .filter_map(|contract| contract.interval_ms)
            .min()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceFault {
    Structure(String),
    Contract(String),
}

impl std::fmt::Display for PlaceFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structure(message) | Self::Contract(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for PlaceFault {}

/// Read every place and decide which ones are due.
pub fn assess(
    doc: &str,
    answers: &BTreeMap<String, PlaceAnswer>,
) -> Result<PlaceAssessment, PlaceFault> {
    let regions = agent_doc_element::place::regions(doc)
        .map_err(|error| PlaceFault::Contract(error.to_string()))?;
    if let Some(message) = nested_message(&regions) {
        return Err(PlaceFault::Contract(message));
    }
    if let Some(region) = regions
        .iter()
        .find(|region| region.structure != PlaceStructure::WellFormed)
    {
        let id = region.id.as_deref().unwrap_or("(missing id)");
        return Err(PlaceFault::Structure(
            region.structure.message(id, region.open_line),
        ));
    }
    let mut contracts = Vec::new();
    let mut seed = Vec::new();
    let mut seen = BTreeMap::<String, usize>::new();
    for region in &regions {
        let contract = contract_for(region, answers)?;
        if let Some(line) = seen.insert(contract.id.clone(), region.open_line) {
            return Err(PlaceFault::Contract(format!(
                "`agent:place` id `{}` is duplicated (lines {line} and {})",
                contract.id, region.open_line
            )));
        }
        if answers.get(&contract.id).is_none()
            && !contract.prompt.is_empty()
            && !contract.output.trim().is_empty()
        {
            seed.push(PlaceSeed {
                id: contract.id.clone(),
                prompt_hash: contract.prompt_hash.clone(),
                output: contract.output.clone(),
            });
        }
        contracts.push(contract);
    }
    Ok(PlaceAssessment { contracts, seed })
}

pub fn render_checkpoint(file: &str, contracts: &[PlaceContract], lead: &str) -> String {
    let mut out = String::new();
    out.push_str(lead);
    out.push('\n');
    for contract in contracts {
        let mode = match contract.mode {
            PlaceMode::Once => "once",
            PlaceMode::Sync => "sync",
        };
        out.push_str(&format!(
            "- id `{id}` mode `{mode}`\n  Prompt: {prompt}\n  Current `place:out`: {output}\n  Write with `agent-doc place set '{file}' --id '{id}' --text \"<answer>\"`.\n",
            id = contract.id,
            prompt = contract.prompt,
            output = if contract.output.is_empty() {
                "(empty)".to_string()
            } else {
                contract.output.replace('\n', " ")
            },
        ));
        if contract.revert {
            out.push_str(
                "  The output is derived and no longer matches the last write (an editor undo does not stick). Re-derive it and write again even if the answer equals that last write.\n",
            );
        } else if contract.needs_write {
            out.push_str("  This place needs a write.\n");
        } else {
            out.push_str("  If the answer is unchanged, make no write.\n");
        }
    }
    out.push_str(PLACES_GUIDANCE);
    out
}

fn contract_for(
    region: &PlaceRegion,
    answers: &BTreeMap<String, PlaceAnswer>,
) -> Result<PlaceContract, PlaceFault> {
    let id = region
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            PlaceFault::Contract(format!(
                "`agent:place` at line {} requires an `id` attribute",
                region.open_line
            ))
        })?;
    if id.chars().any(char::is_whitespace) {
        return Err(PlaceFault::Contract(format!(
            "`agent:place` id `{id}` must be a single token"
        )));
    }
    let mode = match region.mode_attr.as_deref().map(str::trim) {
        None => PlaceMode::Once,
        Some("once") => PlaceMode::Once,
        Some("sync") => PlaceMode::Sync,
        Some("") => {
            return Err(PlaceFault::Contract(format!(
                "`agent:place` id `{id}` has a bare `mode`; expected `once` or `sync`"
            )));
        }
        Some(other) => {
            return Err(PlaceFault::Contract(format!(
                "`agent:place` id `{id}` has unsupported mode `{other}`; expected `once` or `sync`"
            )));
        }
    };
    let interval_ms = match region.interval_attr.as_deref() {
        None => None,
        Some(_raw) if mode != PlaceMode::Sync => {
            return Err(PlaceFault::Contract(format!(
                "`agent:place` id `{id}` has `interval` without `mode=\"sync\"`"
            )));
        }
        Some(raw) => Some(parse_interval_ms(raw).map_err(|message| {
            PlaceFault::Contract(format!("`agent:place` id `{id}` {message}"))
        })?),
    };
    let prompt = if !region.body_prompt.is_empty() {
        region.body_prompt.clone()
    } else {
        region
            .prompt_attr
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let output = region.output.clone().unwrap_or_default();
    let prompt_hash = prompt_hash(&prompt);
    let stored = answers.get(id);
    let prompt_changed = stored.is_some_and(|answer| answer.prompt_hash != prompt_hash);
    let unanswered = stored.is_none() && output.trim().is_empty() && !prompt.is_empty();
    let revert = mode == PlaceMode::Sync
        && stored.is_some_and(|answer| answer.output.trim() != output.trim());
    let needs_write = !prompt.is_empty() && (unanswered || prompt_changed || revert);
    Ok(PlaceContract {
        id: id.to_string(),
        mode,
        prompt,
        prompt_hash,
        output,
        interval_ms,
        needs_write,
        revert,
    })
}

fn nested_message(regions: &[PlaceRegion]) -> Option<String> {
    for region in regions {
        if let Some(inner) = regions.iter().find(|other| {
            other.open_start > region.open_start && other.close_end <= region.close_start
        }) {
            return Some(format!(
                "`agent:place` must not nest (line {} inside line {})",
                inner.open_line, region.open_line
            ));
        }
    }
    None
}

pub fn prompt_hash(prompt: &str) -> String {
    agent_doc_hash::content_hash(prompt.trim())
        .chars()
        .take(12)
        .collect()
}

fn parse_interval_ms(raw: &str) -> Result<u64, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("interval must contain a duration".to_string());
    }
    let seconds = if let Ok(seconds) = raw.parse::<u64>() {
        seconds
    } else {
        let mut total = 0u64;
        let mut digits = String::new();
        for ch in raw.chars() {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            let unit = match ch.to_ascii_lowercase() {
                'd' => 86_400u64,
                'h' => 3_600,
                'm' => 60,
                's' => 1,
                _ => {
                    return Err(format!(
                        "interval `{raw}` is not a duration; use e.g. `60`, `1m`, `5m`, or `1h30m`"
                    ));
                }
            };
            let value = digits.parse::<u64>().map_err(|_| {
                format!("interval `{raw}` is not a duration; use e.g. `60`, `1m`, `5m`, or `1h30m`")
            })?;
            digits.clear();
            total = total
                .checked_add(
                    value
                        .checked_mul(unit)
                        .ok_or_else(|| format!("interval `{raw}` is too long"))?,
                )
                .ok_or_else(|| format!("interval `{raw}` is too long"))?;
        }
        if !digits.is_empty() {
            return Err(format!(
                "interval `{raw}` is not a duration; use e.g. `60`, `1m`, `5m`, or `1h30m`"
            ));
        }
        total
    };
    let ms = seconds
        .checked_mul(1_000)
        .ok_or_else(|| format!("interval `{raw}` is too long"))?;
    if ms < MIN_INTERVAL_MS {
        return Err(format!(
            "interval `{raw}` is shorter than 1m; a faster cadence is refused"
        ));
    }
    Ok(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn once(body: &str, output: &str) -> String {
        format!(
            "<!-- agent:place id=\"step-1\" -->\n{body}\n<!-- place:out -->\n{output}<!-- /place:out -->\n<!-- /agent:place -->\n"
        )
    }

    #[test]
    fn an_empty_output_is_due_until_the_prompt_hash_is_recorded() {
        let doc = once("Rewrite the paragraph.", "");
        let due = assess(&doc, &BTreeMap::new()).unwrap();
        assert!(due.needs_turn());
        assert_eq!(due.contracts[0].mode, PlaceMode::Once);
        assert!(due.contracts[0].needs_write);

        let mut answers = BTreeMap::new();
        answers.insert(
            "step-1".to_string(),
            PlaceAnswer {
                prompt_hash: due.contracts[0].prompt_hash.clone(),
                output: "1. Read.".to_string(),
            },
        );
        let answered = once("Rewrite the paragraph.", "1. Read.\n");
        let settled = assess(&answered, &answers).unwrap();
        assert!(!settled.needs_turn());
        assert!(settled.visible().is_empty());
    }

    #[test]
    fn editing_the_prompt_reopens_once_and_an_output_edit_does_not() {
        let original = assess(&once("Rewrite it.", "done\n"), &BTreeMap::new()).unwrap();
        let mut answers = BTreeMap::new();
        answers.insert(
            "step-1".to_string(),
            PlaceAnswer {
                prompt_hash: original.contracts[0].prompt_hash.clone(),
                output: "done".to_string(),
            },
        );
        let edited = assess(&once("Summarize it.", "done\n"), &answers).unwrap();
        assert!(edited.needs_turn());
        let undone = assess(&once("Rewrite it.", "older\n"), &answers).unwrap();
        assert!(!undone.needs_turn());
    }

    #[test]
    fn sync_revert_demands_a_write_and_interval_requires_sync() {
        let doc = "\
<!-- agent:place id=\"step-1\" mode=\"sync\" interval=5m -->
Keep the note current.
<!-- place:out -->
v1
<!-- /place:out -->
<!-- /agent:place -->
";
        let first = assess(doc, &BTreeMap::new()).unwrap();
        assert_eq!(first.contracts[0].interval_ms, Some(300_000));
        assert!(
            !first.needs_turn(),
            "a prefilled output is not itself a turn"
        );
        assert_eq!(first.seed.len(), 1);
        let mut answers = BTreeMap::new();
        answers.insert(
            "step-1".to_string(),
            PlaceAnswer {
                prompt_hash: first.contracts[0].prompt_hash.clone(),
                output: "v1".to_string(),
            },
        );
        let reverted = doc.replace("v1", "undo");
        let due = assess(&reverted, &answers).unwrap();
        assert!(due.contracts[0].revert);
        assert!(due.needs_turn());

        let once_interval = doc.replace("mode=\"sync\" ", "");
        let error = assess(&once_interval, &BTreeMap::new()).unwrap_err();
        assert!(matches!(error, PlaceFault::Contract(_)), "{error}");
    }

    #[test]
    fn the_body_wins_when_a_prompt_attribute_is_also_set() {
        let doc = "\
<!-- agent:place id=\"step-1\" prompt=\"attribute wins only if the body is empty\" -->
body wins
<!-- place:out -->
<!-- /place:out -->
<!-- /agent:place -->
";
        let assessed = assess(doc, &BTreeMap::new()).unwrap();
        assert_eq!(assessed.contracts[0].prompt, "body wins");
    }
}
