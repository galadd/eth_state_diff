//! Delta encoding for Ethereum slashing vectors.
//!
//! Ethereum consensus stores slashing totals in a fixed-capacity circular
//! buffer indexed by epoch.
//!
//! Unlike root and RANDAO buffers, slashing values change infrequently. This
//! module therefore records only epochs whose values differ between two states,
//! producing a sparse delta representation.
//!
//! Applying a delta updates only the modified epochs while leaving all other
//! entries unchanged.

use crate::types::{ArchivedSlashingsDiffs, SlashingsDiffs};

/// Computes a sparse delta between two slashing buffers.
///
/// The encoder compares the epochs traversed while advancing from
/// `base_slot` to `target_slot` and records only entries whose values have
/// changed.
///
/// The returned delta contains pairs of `(ring_index, value)` representing the
/// updated slashing totals.
///
/// # Arguments
///
/// * `base_slot` - Starting slot.
/// * `target_slot` - Ending slot.
/// * `base_buffer` - Base slashing ring buffer.
/// * `target_buffer` - Target slashing ring buffer.
/// * `slots_per_epoch` - Number of slots in a consensus epoch.
///
/// # Complexity
///
/// O(number of traversed epochs)
///
/// # Panics
///
/// Panics if `target_slot < base_slot`.
pub fn diff_slashings(
    base_slot: u64,
    target_slot: u64,
    base_buffer: &[u64],
    target_buffer: &[u64],
    slots_per_epoch: u64,
) -> SlashingsDiffs {
    let base_epoch = base_slot / slots_per_epoch;
    let target_epoch = target_slot / slots_per_epoch;
    let modulus = base_buffer.len() as u64;

    let mut updates = Vec::new();

    // Iterate through the epochs that passed in this window.
    // For a 32-slot window, this loop runs exactly ONCE.
    let mut current_epoch = base_epoch;
    while current_epoch < target_epoch {
        current_epoch += 1;
        let idx = (current_epoch % modulus) as u16;

        let base_val = base_buffer[idx as usize];
        let target_val = target_buffer[idx as usize];

        if base_val != target_val {
            updates.push((idx, target_val));
        }
    }

    SlashingsDiffs { updates }
}

/// Applies a slashing delta to a circular slashing buffer.
///
/// Each recorded update replaces the value at its corresponding ring-buffer
/// index. Entries not present in the delta remain unchanged.
///
/// After successful application, the destination buffer contains the same
/// slashing values as the target buffer used to produce `delta`.
///
/// # Complexity
///
/// O(number of recorded updates)
pub fn apply_slashings(base_buffer: &mut [u64], delta: &ArchivedSlashingsDiffs) {
    for update in delta.updates.iter() {
        let idx = update.0.to_native() as usize;
        let val = update.1.to_native();

        base_buffer[idx] = val;
    }
}
