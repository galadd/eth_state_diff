//! Delta encoding for Ethereum slashing vectors.
//!
//! Ethereum consensus stores slashing totals in a fixed-capacity circular
//! buffer indexed by epoch.
//!
//! Unlike root and RANDAO buffers, where every slot or epoch transition may
//! require recording a new value, slashing totals are comparatively sparse.
//! This module therefore records only ring-buffer entries whose values differ
//! between the base and target states.
//!
//! The delta stores `(ring_index, value)` pairs. Applying the delta writes only
//! those changed entries into the destination buffer; all other entries remain
//! untouched.
//!
//! # Epoch Mapping
//!
//! Ring-buffer indices are derived from the epoch number:
//!
//! ```text
//! ring_index = epoch % buffer_capacity
//! ```
//!
//! [`diff_slashings`] examines the epochs crossed between `base_slot` and
//! `target_slot`. The base epoch itself is not examined. For a transition from
//! `base_epoch` to `target_epoch`, the inspected epochs are:
//!
//! ```text
//! base_epoch + 1, ..., target_epoch
//! ```
//!
//! This means that when both slots belong to the same epoch, no entries are
//! examined and the resulting delta is empty.
//!
//! # Correctness
//!
//! The base and target buffers must have the same capacity and represent the
//! same slashing ring. The delta stores ring indices rather than epochs, so the
//! buffer capacity is part of the implicit representation.
//!
//! Applying a delta to a buffer with a different capacity, layout, or unrelated
//! state can write values to incorrect positions.
//!
//! # Complexity
//!
//! If `E` is the number of epoch boundaries crossed by the transition:
//!
//! - [`diff_slashings`] runs in O(E) time and O(U) additional space, where `U`
//!   is the number of changed ring entries.
//! - [`apply_slashings`] runs in O(U) time and O(1) additional space.

use crate::types::{ArchivedSlashingsDiff, SlashingsDiff, SLOTS_PER_EPOCH};

/// Computes a sparse delta between two slashing ring buffers.
///
/// The function compares the slashing value at each ring-buffer position
/// corresponding to an epoch boundary crossed between `base_slot` and
/// `target_slot`.
///
/// For a base epoch `B` and target epoch `T`, the examined epochs are:
///
/// ```text
/// B + 1, B + 2, ..., T
/// ```
///
/// For each examined epoch `E`, its ring-buffer position is:
///
/// ```text
/// index = E % buffer_capacity
/// ```
///
/// If the value at that position differs between `base_buffer` and
/// `target_buffer`, the target value is recorded in the returned
/// [`SlashingsDiff`].
///
/// Unchanged entries are omitted from the delta.
///
/// # Arguments
///
/// * `base_slot` - Slot belonging to the base state.
/// * `target_slot` - Slot belonging to the target state.
/// * `base_buffer` - Slashing ring buffer belonging to the base state.
/// * `target_buffer` - Slashing ring buffer belonging to the target state.
///
/// # Returns
///
/// A [`SlashingsDiff`] containing only the ring-buffer entries whose values
/// changed across the epoch boundaries represented by the transition.
///
/// If `base_slot` and `target_slot` belong to the same epoch, the returned
/// delta contains no updates.
///
/// # Panics
///
/// Panics if `target_slot < base_slot`.
///
/// Panics if `base_buffer` is empty or `target_buffer` is empty.
///
/// Panics if `base_buffer` and `target_buffer` have different lengths.
///
/// # Correctness
///
/// The two buffers must have the same capacity and correspond to the same
/// logical slashing ring.
///
/// The delta stores ring indices rather than epoch numbers. Consequently, the
/// buffer capacity is part of the implicit encoding and must remain unchanged
/// when applying the resulting delta.
///
/// # Example
///
/// ```
/// use eth_state_diff::slashings::diff_slashings;
///
/// let base_buffer = vec![0u64; 4];
/// let mut target_buffer = vec![0u64; 4];
///
/// // The transition crosses from epoch 0 to epoch 1.
/// target_buffer[1] = 100;
///
/// let delta = diff_slashings(
///     0,
///     eth_state_diff::types::SLOTS_PER_EPOCH,
///     &base_buffer,
///     &target_buffer,
/// );
///
/// assert_eq!(delta.updates.len(), 1);
/// assert_eq!(delta.updates[0], (1, 100));
/// ```
///
/// # Complexity
///
/// If `E` epoch boundaries are crossed and `U` entries change:
///
/// - Time: O(E)
/// - Additional space: O(U)
pub fn diff_slashings(
    base_slot: u64,
    target_slot: u64,
    base_buffer: &[u64],
    target_buffer: &[u64],
) -> SlashingsDiff {
    assert!(
        target_slot >= base_slot,
        "target_slot must be greater than or equal to base_slot"
    );

    assert!(!base_buffer.is_empty(), "slashing buffer must not be empty");

    assert!(
        !target_buffer.is_empty(),
        "slashing buffer must not be empty"
    );

    assert_eq!(
        base_buffer.len(),
        target_buffer.len(),
        "base and target slashing buffers must have the same capacity"
    );

    let base_epoch = base_slot / SLOTS_PER_EPOCH;
    let target_epoch = target_slot / SLOTS_PER_EPOCH;
    let capacity = base_buffer.len() as u64;

    let mut updates = Vec::new();

    let mut current_epoch = base_epoch;

    while current_epoch < target_epoch {
        current_epoch += 1;

        let idx = (current_epoch % capacity) as usize;

        let base_val = base_buffer[idx];
        let target_val = target_buffer[idx];

        if base_val != target_val {
            updates.push((idx as u16, target_val));
        }
    }

    SlashingsDiff { updates }
}

/// Applies a sparse slashing delta to a circular slashing buffer in place.
///
/// Each update in `delta` contains a ring-buffer index and its replacement
/// value. Only the recorded indices are modified; all other entries in
/// `base_buffer` remain unchanged.
///
/// This operation does not perform any epoch calculation because the delta
/// already contains the ring-buffer indices produced by [`diff_slashings`].
///
/// # Arguments
///
/// * `base_buffer` - Destination slashing ring buffer. It is modified in place.
/// * `delta` - Archived [`SlashingsDiff`] containing the sparse updates.
///
/// # Correctness
///
/// The destination buffer must have the same capacity and logical ring layout
/// as the buffer used to create the delta.
///
/// After successful application, every ring-buffer entry represented by the
/// delta contains its target value. Entries omitted from the delta retain
/// their existing values.
///
/// # Panics
///
/// Panics if an update contains an index outside `base_buffer`.
///
/// # Example
///
/// ```
/// use eth_state_diff::slashings::{apply_slashings, diff_slashings};
/// use eth_state_diff::types::ArchivedSlashingsDiff;
///
/// let base_buffer = vec![0u64; 4];
/// let mut target_buffer = vec![0u64; 4];
/// target_buffer[1] = 100;
///
/// let delta = diff_slashings(
///     0,
///     eth_state_diff::types::SLOTS_PER_EPOCH,
///     &base_buffer,
///     &target_buffer,
/// );
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedSlashingsDiff>(&bytes)
/// };
///
/// let mut reconstructed = base_buffer.clone();
/// apply_slashings(&mut reconstructed, archived);
///
/// assert_eq!(reconstructed, target_buffer);
/// ```
///
/// # Complexity
///
/// If `U` updates are stored in the delta:
///
/// - Time: O(U)
/// - Additional space: O(1)
pub fn apply_slashings(base_buffer: &mut [u64], delta: &ArchivedSlashingsDiff) {
    for update in delta.updates.iter() {
        let idx = update.0.to_native() as usize;
        let val = update.1.to_native();

        base_buffer[idx] = val;
    }
}
