//! Delta encoding for Ethereum RANDAO mix buffers.
//!
//! Ethereum consensus maintains historical RANDAO mixes in a fixed-capacity
//! circular buffer. Each epoch writes one mix, with the buffer index derived
//! from the epoch number modulo the buffer capacity.
//!
//! Rather than storing the complete RANDAO buffer, this module stores only the
//! sequence of mixes needed to advance a buffer from one slot to another.
//! Applying the delta replays those epoch writes using the same circular-buffer
//! indexing rule.
//!
//! The delta therefore contains no buffer indices. Indices are reconstructed
//! deterministically from the starting slot and the destination buffer's
//! capacity.
//!
//! # Representation
//!
//! [`RandaoDiff`] stores one 32-byte RANDAO mix for each epoch represented by
//! the transition. For an epoch `e` and buffer capacity `N`, the mix is stored
//! at:
//!
//! ```text
//! e % N
//! ```
//!
//! During application, the same calculation is performed starting from the
//! epoch containing `base_slot`.
//!
//! # Correctness
//!
//! The source and destination buffers must have the same capacity. The delta
//! does not store explicit buffer indices; instead, indices are reconstructed
//! using the circular-buffer capacity.
//!
//! Consequently, applying a delta to a buffer with a different capacity can
//! write mixes to different positions and will not reconstruct the original
//! target state.
//!
//! # Workflow
//!
//! The typical workflow is:
//!
//! 1. Call [`diff_randao`] with the base slot, target slot, and target RANDAO
//!    buffer.
//! 2. Serialize the resulting [`RandaoDiff`] using `rkyv`.
//! 3. Store or compress the serialized delta.
//! 4. Deserialize/access the archived delta and apply it with
//!    [`apply_randao`].
//!
//! # Complexity
//!
//! If `E` is the number of epochs covered by the transition:
//!
//! - [`diff_randao`] runs in O(E) time and uses O(E) additional space.
//! - [`apply_randao`] runs in O(E) time and uses O(1) additional space.

use crate::types::{ArchivedRandaoDiff, RandaoDiff, SLOTS_PER_EPOCH};

/// Computes the RANDAO delta between two consensus slots.
///
/// The returned delta contains one RANDAO mix for every epoch in the inclusive
/// range from the epoch containing `base_slot` through the epoch containing
/// `target_slot`.
///
/// Each mix is read from `target_buffer` using the circular-buffer indexing
/// rule:
///
/// ```text
/// buffer_index = epoch % target_buffer.len()
/// ```
///
/// Only the mixes corresponding to the covered epochs are stored. The complete
/// target buffer is not copied.
///
/// # Arguments
///
/// * `base_slot` - Starting consensus slot. The epoch containing this slot is
///   the first epoch represented in the delta.
/// * `target_slot` - Ending consensus slot. The epoch containing this slot is
///   the final epoch represented in the delta.
/// * `target_buffer` - RANDAO mix buffer belonging to the target state.
///
/// # Returns
///
/// A [`RandaoDiff`] containing one mix for each epoch from the base epoch
/// through the target epoch, inclusive.
///
/// # Panics
///
/// Panics if `target_slot < base_slot`.
///
/// Panics if `target_buffer` is empty because circular-buffer indexing requires
/// a non-zero capacity.
///
/// # Correctness
///
/// The delta stores mixes in chronological epoch order rather than storing
/// their circular-buffer indices. During application, indices are reconstructed
/// using modulo arithmetic and the capacity of the destination buffer.
///
/// The buffer used with [`apply_randao`] must therefore have the same capacity
/// as `target_buffer`.
///
/// # Example
///
/// ```
/// use eth_state_diff::randao_mixes::diff_randao;
///
/// let base_slot = 0;
/// let target_slot = 64;
///
/// let target_buffer = vec![[0u8; 32]; 4];
///
/// let delta = diff_randao(base_slot, target_slot, &target_buffer);
///
/// // Slots 0 and 64 belong to epochs 0 and 2 respectively, so the delta
/// // contains one mix for each epoch in the inclusive range 0..=2.
/// assert_eq!(delta.mixes.len(), 3);
/// ```
///
/// # Complexity
///
/// If `E` is the number of epochs from the base epoch through the target
/// epoch, inclusive:
///
/// - Time: O(E)
/// - Additional space: O(E)
pub fn diff_randao(base_slot: u64, target_slot: u64, target_buffer: &[[u8; 32]]) -> RandaoDiff {
    assert!(
        target_slot >= base_slot,
        "target_slot must be greater than or equal to base_slot"
    );

    assert!(!target_buffer.is_empty(), "RANDAO buffer must not be empty");

    let base_epoch = base_slot / SLOTS_PER_EPOCH;
    let target_epoch = target_slot / SLOTS_PER_EPOCH;
    let capacity = target_buffer.len() as u64;

    let mut mixes = Vec::with_capacity((target_epoch - base_epoch + 1) as usize);

    for epoch in base_epoch..=target_epoch {
        let idx = (epoch % capacity) as usize;
        mixes.push(target_buffer[idx]);
    }

    RandaoDiff { mixes }
}

/// Applies a RANDAO delta to a circular mix buffer in place.
///
/// Each mix stored in `delta` is written to the destination buffer at the
/// position corresponding to its epoch. The first mix is written to the epoch
/// containing `base_slot`; each subsequent mix advances by one epoch.
///
/// The destination index is reconstructed using:
///
/// ```text
/// buffer_index = epoch % base_buffer.len()
/// ```
///
/// No allocation is performed while applying the delta.
///
/// # Arguments
///
/// * `base_slot` - Starting consensus slot corresponding to the first mix in
///   `delta`.
/// * `base_buffer` - Destination RANDAO circular buffer. It is modified in
///   place.
/// * `delta` - Archived [`RandaoDiff`] containing the mixes to replay.
///
/// # Correctness
///
/// This function is the application counterpart to [`diff_randao`].
///
/// For correct reconstruction, `base_buffer` must have the same capacity as
/// the target buffer that was supplied to [`diff_randao`].
///
/// The delta does not contain explicit buffer indices. The indices are derived
/// from the starting epoch and the buffer capacity. Changing either value
/// changes where the recorded mixes are written.
///
/// # Panics
///
/// Panics if `base_buffer` is empty because circular-buffer indexing requires
/// a non-zero capacity.
///
/// # Example
///
/// ```
/// use eth_state_diff::randao_mixes::{apply_randao, diff_randao};
/// use eth_state_diff::types::ArchivedRandaoDiff;
///
/// let base_slot = 0;
/// let target_slot = 64;
///
/// let mut target_buffer = vec![[0u8; 32]; 4];
/// target_buffer[0] = [1u8; 32];
/// target_buffer[1] = [2u8; 32];
///
/// let delta = diff_randao(base_slot, target_slot, &target_buffer);
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedRandaoDiff>(&bytes)
/// };
///
/// let mut reconstructed = vec![[0u8; 32]; 4];
/// apply_randao(base_slot, &mut reconstructed, archived);
///
/// assert_eq!(reconstructed, target_buffer);
/// ```
///
/// # Complexity
///
/// If `E` is the number of mixes stored in `delta`:
///
/// - Time: O(E)
/// - Additional space: O(1)
pub fn apply_randao(base_slot: u64, base_buffer: &mut [[u8; 32]], delta: &ArchivedRandaoDiff) {
    assert!(!base_buffer.is_empty(), "RANDAO buffer must not be empty");

    let capacity = base_buffer.len() as u64;
    let mut current_epoch = base_slot / SLOTS_PER_EPOCH;

    for mix in delta.mixes.iter() {
        let idx = (current_epoch % capacity) as usize;
        base_buffer[idx] = *mix;
        current_epoch += 1;
    }
}
