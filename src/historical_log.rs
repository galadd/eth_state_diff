//! Delta encoding for append-only historical logs.
//!
//! Both `historical_roots` (Phase0 to Bellatrix) and `historical_summaries`
//! (Capella+) are append-only logs that grow by exactly one item per 8,192 slots.
//! This module uses protocol math to calculate exactly how many items were
//! appended, rather than comparing base and target byte arrays.

use crate::types::{ArchivedHistoricalLogDiff, HistoricalLogDiff};

/// The protocol-defined period for historical calculations (8192 slots).
const SLOTS_PER_HISTORICAL_PERIOD: u64 = 8192;

/// Calculates the expected length of the historical log at a given slot.
#[inline]
fn calculate_log_count(slot: u64, activation_slot: Option<u64>) -> u64 {
    match activation_slot {
        Some(act_slot) if slot <= act_slot => 0,
        Some(act_slot) => (slot - act_slot) / SLOTS_PER_HISTORICAL_PERIOD,
        None => (slot + 1) / SLOTS_PER_HISTORICAL_PERIOD,
    }
}

/// Computes a historical log delta using protocol math.
pub fn diff_historical_log(
    base_slot: u64,
    target_slot: u64,
    target_ssz: &[u8],
    item_ssz_size: usize,
    activation_slot: Option<u64>,
) -> HistoricalLogDiff {
    let base_count = calculate_log_count(base_slot, activation_slot);
    let target_count = calculate_log_count(target_slot, activation_slot);

    let items_to_append = (target_count - base_count) as usize;

    if items_to_append == 0 {
        return HistoricalLogDiff::Unchanged;
    }

    let required_bytes = items_to_append * item_ssz_size;

    debug_assert!(
        target_ssz.len() >= required_bytes,
        "Historical log math expects {required_bytes} bytes, but target_ssz is too short",
    );

    let start_byte = target_ssz.len() - required_bytes;
    HistoricalLogDiff::Append(target_ssz[start_byte..].to_vec())
}

/// Applies a historical log delta in place.
pub fn apply_historical_log(base: &mut Vec<u8>, delta: &ArchivedHistoricalLogDiff) {
    match delta {
        ArchivedHistoricalLogDiff::Unchanged => {}
        ArchivedHistoricalLogDiff::Append(bytes) => {
            base.extend_from_slice(bytes.as_slice());
        }
    }
}
