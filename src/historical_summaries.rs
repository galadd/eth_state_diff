//! Delta encoding for the historical summaries list.
//!
//! This module leverages the deterministic timing of `process_historical_summaries_update`
//! to calculate exactly how many items *must* have been appended between two slots,
//! rather than comparing base and target byte arrays.

use crate::types::{ArchivedHistoricalSummariesDiff, HistoricalSummariesDiff};

/// The protocol-defined period for historical root calculations.
const SLOTS_PER_HISTORICAL_ROOT: u64 = 8192;

/// The size of a single `HistoricalSummary` in SSZ bytes (two 32-byte roots).
const HISTORICAL_SUMMARY_SSZ_SIZE: usize = 64;

/// Calculates the exact number of historical summaries that should exist at a given slot.
#[inline]
fn calculate_hist_sum_count(slot: u64, capella_fork_slot: u64) -> u64 {
    if slot <= capella_fork_slot {
        return 0;
    }
    (slot - capella_fork_slot) / SLOTS_PER_HISTORICAL_ROOT
}

/// Computes the historical summaries delta using protocol math.
///
/// # Arguments
///
/// * `base_slot` - The slot of the base state.
/// * `target_slot` - The slot of the target state.
/// * `target_ssz` - The raw SSZ bytes of the *target* historical summaries list.
/// * `capella_fork_slot` - The slot at which Capella activated on this specific network.
///
/// # Panics
///
/// Panics if the calculated math dictates items were added, but `target_ssz`
/// is too short to contain them (indicating a corrupted state or wrong fork slot).
pub fn diff_historical_summaries(
    base_slot: u64,
    target_slot: u64,
    target_ssz: &[u8],
    capella_fork_slot: u64,
) -> HistoricalSummariesDiff {
    let base_count = calculate_hist_sum_count(base_slot, capella_fork_slot);
    let target_count = calculate_hist_sum_count(target_slot, capella_fork_slot);

    let items_to_append = (target_count - base_count) as usize;

    if items_to_append == 0 {
        return HistoricalSummariesDiff::Unchanged;
    }

    let required_bytes = items_to_append * HISTORICAL_SUMMARY_SSZ_SIZE;

    assert!(
        target_ssz.len() >= required_bytes,
        "Historical summaries math expects {required_bytes} bytes, but target_ssz is too short",
    );

    let start_byte = target_ssz.len() - required_bytes;
    HistoricalSummariesDiff::Append(target_ssz[start_byte..].to_vec())
}

/// Applies a historical summaries delta in place.
///
/// # Complexity
///
/// O(appended bytes)
pub fn apply_historical_summaries(base: &mut Vec<u8>, delta: &ArchivedHistoricalSummariesDiff) {
    match delta {
        ArchivedHistoricalSummariesDiff::Unchanged => {}
        ArchivedHistoricalSummariesDiff::Append(summary_bytes) => {
            base.extend_from_slice(summary_bytes.as_slice());
        }
    }
}
