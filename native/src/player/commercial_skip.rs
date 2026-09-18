//! Port of `VideoPlayer.tsx`'s auto-skip algorithm. Deliberately pure/
//! Player-agnostic — just "given these blocks, this position, and this set
//! of manually-overridden blocks, should we skip, and to where" — so it's
//! easy to reason about independent of mpv or the UI.
//!
//! The old app's per-block "manual override" tracking (re-enable auto-skip
//! once the user seeks well before a block, disable it if they seek back
//! into one) isn't ported yet: there's no seek/scrub UI in the overlay yet
//! for a user to trigger that distinction with. `disabled` is threaded
//! through regardless so that piece can slot in later without changing
//! this function's shape.

/// Parses the flat `[start, end, start, end, ...]` shape `NowPlaying`
/// carries (same as the old `nowPlayingCommercials: number[]`) into pairs.
pub fn ad_blocks(flat: &[f64]) -> Vec<(f64, f64)> {
    flat.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

/// Returns `Some((block_index, seek_to))` if `position` currently falls
/// inside a not-disabled ad block — `seek_to` is always the block's end.
pub fn block_to_skip(
    blocks: &[(f64, f64)],
    disabled: &std::collections::HashSet<usize>,
    position: f64,
) -> Option<(usize, f64)> {
    for (i, &(start, end)) in blocks.iter().enumerate() {
        if disabled.contains(&i) {
            continue;
        }
        if position >= start && position < end {
            return Some((i, end));
        }
    }
    None
}
