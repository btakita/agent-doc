use agent_doc_cycle_state_io::inline_effect::{self, InlineEffectState};
use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

pub(super) fn tool(args: &Map<String, Value>) -> Result<Value> {
    super::ensure_mcp_binary_fresh_for_mutation()?;
    let file = super::required_path_arg(args, "file")?.canonicalize()?;
    let receipt_id = super::optional_string_arg(args, "receipt_id")?;
    let operation_id = super::optional_string_arg(args, "operation_id")?;
    let adapter = super::optional_string_arg(args, "adapter")?;
    let outcome = super::optional_string_arg(args, "outcome")?;
    let reason = super::optional_string_arg(args, "reason")?;
    let contract = args.get("contract").cloned();
    let observation = args.get("observation").cloned();

    let receipts = match (receipt_id, operation_id) {
        (Some(receipt_id), None) => {
            if adapter.is_some() || contract.is_some() {
                bail!("settling an inline effect cannot replace its adapter or contract");
            }
            let outcome = outcome
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("settling an inline effect requires outcome"))?;
            let state = match outcome {
                "applied" => InlineEffectState::Applied {
                    observation: observation.ok_or_else(|| {
                        anyhow::anyhow!(
                            "applied inline effect requires its operation-specific observation"
                        )
                    })?,
                },
                "failed" => InlineEffectState::Failed {
                    reason: reason
                        .filter(|reason| !reason.trim().is_empty())
                        .ok_or_else(|| anyhow::anyhow!("failed inline effect requires reason"))?,
                    observation,
                },
                other => {
                    bail!("unsupported inline effect outcome {other:?}; use applied or failed")
                }
            };
            vec![inline_effect::settle(&file, &receipt_id, state)?]
        }
        (None, Some(operation_id)) => {
            if outcome.is_some() || reason.is_some() || observation.is_some() {
                bail!("reserving an inline effect cannot include its future outcome");
            }
            let adapter = adapter
                .filter(|adapter| !adapter.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("inline effect reservation requires adapter"))?;
            let contract = contract
                .ok_or_else(|| anyhow::anyhow!("inline effect reservation requires contract"))?;
            vec![inline_effect::reserve(
                &file,
                &operation_id,
                &adapter,
                contract,
            )?]
        }
        (None, None) => {
            if adapter.is_some()
                || contract.is_some()
                || outcome.is_some()
                || reason.is_some()
                || observation.is_some()
            {
                bail!("inline effect inspection takes only file");
            }
            inline_effect::list(&file)?
        }
        (Some(_), Some(_)) => {
            bail!("use receipt_id to settle or operation_id to reserve, not both")
        }
    };

    Ok(super::tool_success_result(
        "inline effect receipt recorded".to_string(),
        json!({
            "ok": true,
            "file": file,
            "inline_effect_receipts": receipts,
        }),
    ))
}
