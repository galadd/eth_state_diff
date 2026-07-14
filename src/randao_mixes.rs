//! Delta encoding for Ethereum RANDAO mix buffers.
//!
//! Ethereum consensus stores historical RANDAO mixes in a fixed-capacity
//! circular buffer indexed by epoch.
//!
//! Rather than storing the entire buffer, this module records only the sequence
//! of mixes written while advancing from one epoch to another. Applying the
//! delta replays those writes into another buffer using the same circular
//! indexing logic.
//!
//! The encoding is independent of the buffer capacity and relies only on the
//! starting slot and the destination buffer.

use crate::types::{ArchivedRandaoDiffs, RandaoDiffs};

/// Computes the sequence of RANDAO mixes written between two slots.
///
/// The returned delta contains one mix for every epoch in the inclusive range
/// from the epoch containing `base_slot` through the epoch containing
/// `target_slot`.
///
/// The target buffer is treated as a circular buffer, with its length defining
/// the modulo capacity.
///
/// # Arguments
///
/// * `base_slot` - Starting slot.
/// * `target_slot` - Ending slot.
/// * `target_buffer` - Circular RANDAO mix buffer.
/// * `slots_per_epoch` - Number of slots in a consensus epoch.
///
/// # Complexity
///
/// O(number of epochs)
///
/// # Panics
///
/// Panics if `target_slot < base_slot`.
pub fn diff_randao(
    base_slot: u64,
    target_slot: u64,
    target_buffer: &[[u8; 32]],
    slots_per_epoch: u64,
) -> RandaoDiffs {
    debug_assert!(
        target_slot >= base_slot,
        "target_slot must not precede base_slot"
    );

    let base_epoch = base_slot / slots_per_epoch;
    let target_epoch = target_slot / slots_per_epoch;
    let capacity = target_buffer.len() as u64;
    let mut mixes = Vec::with_capacity((target_epoch - base_epoch + 1) as usize);
    for epoch in base_epoch..=target_epoch {
        let idx = (epoch % capacity) as usize;
        mixes.push(target_buffer[idx]);
    }
    RandaoDiffs { mixes }
}

/// Applies a RANDAO delta to a circular mix buffer.
///
/// Each recorded mix is written back into the destination buffer beginning at
/// the epoch containing `base_slot`.
///
/// After successful application, the destination buffer contains the same
/// RANDAO mixes as the original target buffer for every epoch represented by
/// the delta.
///
/// # Complexity
///
/// O(number of encoded epochs)
pub fn apply_randao(
    base_slot: u64,
    base_buffer: &mut [[u8; 32]],
    delta: &ArchivedRandaoDiffs,
    slots_per_epoch: u64,
) {
    let capacity = base_buffer.len() as u64;
    let mut current_epoch = base_slot / slots_per_epoch;
    for mix in delta.mixes.iter() {
        let idx = (current_epoch % capacity) as usize;
        base_buffer[idx] = *mix;
        current_epoch += 1;
    }
}
