//! Shared per-item priority syntax for managed lists.

/// Lower ranks sort first. An unnumbered item follows every numbered item.
pub const PRIORITY_RANK_UNSET: u8 = 10;

/// The first valid whitespace-delimited `priority=<1..9>` token wins.
pub fn item_priority_rank(text: &str) -> u8 {
    for token in text.split_whitespace() {
        if let Some(value) = token.strip_prefix("priority=")
            && let Ok(rank) = value.parse::<u8>()
            && (1..=9).contains(&rank)
        {
            return rank;
        }
    }
    PRIORITY_RANK_UNSET
}
