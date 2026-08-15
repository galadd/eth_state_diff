//! Delta encoding for append-only historical logs.
//!
//! Both `historical_roots` (Phase0 through Bellatrix) and
//! `historical_summaries` (Capella and later) are append-only logs that grow
//! according to the historical-period boundary defined by the consensus
//! protocol.
//!
//! This module uses the base and target slots to determine how many historical
//! items must have been appended. It therefore avoids comparing the complete
//! serialized log contents.
//!
//! The resulting delta contains only the newly appended SSZ items.

use crate::types::{ArchivedHistoricalLogDiff, HistoricalLogDiff};

/// Number of slots in one historical period.
const SLOTS_PER_HISTORICAL_PERIOD: u64 = 8192;

/// Calculates the expected number of historical log items at a given slot.
///
/// When `activation_slot` is provided, the log is considered inactive through
/// that slot and begins accumulating items afterwards. When no activation slot
/// is provided, the calculation starts from genesis.
///
/// This helper follows the historical-period calculation used by the
/// corresponding consensus state field.
#[inline]
fn calculate_log_count(slot: u64, activation_slot: Option<u64>) -> u64 {
    match activation_slot {
        Some(act_slot) if slot <= act_slot => 0,
        Some(act_slot) => (slot - act_slot) / SLOTS_PER_HISTORICAL_PERIOD,
        None => (slot + 1) / SLOTS_PER_HISTORICAL_PERIOD,
    }
}

/// Computes a delta between two historical log states.
///
/// The number of appended items is derived from `base_slot` and `target_slot`
/// using the protocol-defined historical period rather than by comparing the
/// serialized base and target logs.
///
/// Only the newly appended portion of `target_ssz` is stored in the returned
/// [`HistoricalLogDiff`].
///
/// # Arguments
///
/// * `base_slot` - Slot corresponding to the base state.
/// * `target_slot` - Slot corresponding to the target state.
/// * `target_ssz` - Complete serialized SSZ representation of the target
///   historical log.
/// * `item_ssz_size` - Serialized SSZ size of one historical log item.
/// * `activation_slot` - Optional slot at which the historical log becomes
///   active. This is used for logs introduced by a later fork.
///
/// # Returns
///
/// - [`HistoricalLogDiff::Unchanged`] if the protocol-derived log length has
///   not increased between the two slots.
/// - [`HistoricalLogDiff::Append`] containing the newly appended serialized
///   items otherwise.
///
/// # Panics
///
/// This function does not intentionally panic in release builds. In debug
/// builds, it asserts that `target_ssz` contains enough bytes for the number
/// of items derived from the slot calculation.
///
/// The caller must provide a non-zero `item_ssz_size` and a target SSZ buffer
/// consistent with the protocol-derived log length.
///
/// # Complexity
///
/// O(k) time and O(k) additional space, where *k* is the number of newly
/// appended serialized bytes. The number of existing log entries does not
/// affect the computation.
///
/// # Example
///
/// ```
/// use eth_state_diff::historical_log::diff_historical_log;
/// use eth_state_diff::types::HistoricalLogDiff;
///
/// const ITEM_SIZE: usize = 32;
///
/// // One historical period has elapsed, so one item is appended.
/// let target = vec![0u8; ITEM_SIZE];
///
/// let delta = diff_historical_log(
///     0,
///     8192,
///     &target,
///     ITEM_SIZE,
///     None,
/// );
///
/// assert_eq!(
///     delta,
///     HistoricalLogDiff::Append(target),
/// );
/// ```
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

    let appended_bytes = target_ssz.get(start_byte..).expect(
        "Historical log SSZ buffer is too short for the items derived from slot math. \
     Expected at least {required_bytes} bytes for the new items, but the buffer only has {target_ssz.len()} bytes.",
    );

    HistoricalLogDiff::Append(appended_bytes.to_vec())
}

/// Applies a historical log delta to a serialized historical log in place.
///
/// [`HistoricalLogDiff::Unchanged`] leaves the existing log untouched.
///
/// [`HistoricalLogDiff::Append`] appends the serialized historical items
/// contained in the delta to the existing log.
///
/// The delta is assumed to have been generated for the supplied base state.
/// This function does not validate that the base log corresponds to the
/// state from which the delta was created.
///
/// # Complexity
///
/// - [`HistoricalLogDiff::Unchanged`][]: O(1).
/// - [`HistoricalLogDiff::Append`]: O(k), where *k* is the number of appended
///   bytes.
///
/// # Example
///
/// ```
/// use eth_state_diff::historical_log::{
///     apply_historical_log,
///     diff_historical_log,
/// };
/// use eth_state_diff::types::ArchivedHistoricalLogDiff;
///
/// const ITEM_SIZE: usize = 4;
///
/// let mut base = vec![1u8, 2, 3, 4];
/// let target = vec![1u8, 2, 3, 4, 5, 6, 7, 8];
///
/// let delta = diff_historical_log(
///     8191,
///     16384,
///     &target,
///     ITEM_SIZE,
///     None,
/// );
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedHistoricalLogDiff>(&bytes)
/// };
///
/// apply_historical_log(&mut base, archived);
///
/// assert_eq!(base, target);
/// ```
pub fn apply_historical_log(base: &mut Vec<u8>, delta: &ArchivedHistoricalLogDiff) {
    match delta {
        ArchivedHistoricalLogDiff::Unchanged => {}

        ArchivedHistoricalLogDiff::Append(bytes) => {
            base.extend_from_slice(bytes.as_slice());
        }
    }
}
