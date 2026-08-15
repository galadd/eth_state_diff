//! Delta encoding for fixed-size Ethereum consensus root buffers.
//!
//! Ethereum consensus stores historical roots, such as block roots and state
//! roots, in fixed-capacity circular buffers. As new slots are processed,
//! entries are written at positions derived from their slot number, eventually
//! wrapping around and overwriting older entries.
//!
//! Rather than diffing the complete root buffer, this module records only the
//! roots written during a requested slot range. Applying the delta replays
//! those writes into another root buffer using the same slot-to-index mapping.
//!
//! The delta contains no explicit buffer indices. Each index is reconstructed
//! from the slot number and the buffer capacity:
//!
//! ```text
//! buffer_index = slot % buffer_capacity
//! ```
//!
//! # Representation
//!
//! [`RootsDiff`] stores one 32-byte root for every slot in the half-open range
//! `[base_slot, target_slot)`.
//!
//! For example, a transition from slot `100` to slot `103` records the roots
//! for slots:
//!
//! ```text
//! 100, 101, 102
//! ```
//!
//! The root for `target_slot` itself is not included.
//!
//! # Correctness
//!
//! The destination buffer must have the same capacity as the buffer supplied
//! to [`diff_roots`]. The delta does not store explicit indices, so changing
//! the capacity changes the modulo mapping and can cause roots to be written
//! to different positions.
//!
//! This representation is independent of the *absolute* buffer contents. Only
//! the slot range, buffer capacity, and recorded roots are required to replay
//! the writes.
//!
//! # Complexity
//!
//! If `N = target_slot - base_slot`:
//!
//! - [`diff_roots`] runs in O(N) time and uses O(N) additional space.
//! - [`apply_roots`] runs in O(N) time and uses O(1) additional space.

use crate::types::{ArchivedRootsDiff, RootsDiff};

/// Computes the sequence of roots written during a slot range.
///
/// The returned delta contains one root for every slot in the half-open range
/// `[base_slot, target_slot)`.
///
/// For each slot `s`, the root is read from the circular buffer at:
///
/// ```text
/// buffer_index = s % buffer.len()
/// ```
///
/// The root corresponding to `target_slot` is not included.
///
/// # Arguments
///
/// * `base_slot` - The first slot whose root is included in the delta.
/// * `target_slot` - The slot immediately following the final recorded root.
/// * `buffer` - Circular root buffer belonging to the target state.
///
/// # Returns
///
/// A [`RootsDiff`] containing the target root for every slot in
/// `[base_slot, target_slot)`.
///
/// If `base_slot == target_slot`, the returned delta contains no roots.
///
/// # Panics
///
/// Panics if `target_slot < base_slot`.
///
/// Panics if `buffer` is empty, because calculating a circular-buffer index
/// requires a non-zero capacity.
///
/// # Correctness
///
/// The buffer's capacity is part of the implicit representation because root
/// indices are reconstructed using modulo arithmetic.
///
/// The buffer supplied to [`apply_roots`] must therefore have the same capacity
/// as `buffer`.
///
/// # Example
///
/// ```
/// use eth_state_diff::recent_roots::diff_roots;
///
/// let mut target_buffer = vec![[0u8; 32]; 4];
/// target_buffer[0] = [1u8; 32];
/// target_buffer[1] = [2u8; 32];
/// target_buffer[2] = [3u8; 32];
///
/// let delta = diff_roots(0, 3, &target_buffer);
///
/// assert_eq!(delta.roots.len(), 3);
/// assert_eq!(delta.roots[0], [1u8; 32]);
/// assert_eq!(delta.roots[1], [2u8; 32]);
/// assert_eq!(delta.roots[2], [3u8; 32]);
/// ```
///
/// # Complexity
///
/// Let `N = target_slot - base_slot`.
///
/// - Time: O(N)
/// - Additional space: O(N)
pub fn diff_roots(base_slot: u64, target_slot: u64, buffer: &[[u8; 32]]) -> RootsDiff {
    assert!(
        target_slot >= base_slot,
        "target_slot must be greater than or equal to base_slot"
    );

    assert!(!buffer.is_empty(), "root buffer must not be empty");

    let span = target_slot - base_slot;
    let capacity = buffer.len() as u64;

    let mut roots = Vec::with_capacity(span as usize);

    for i in 0..span {
        let slot = base_slot + i;
        let idx = (slot % capacity) as usize;
        roots.push(buffer[idx]);
    }

    RootsDiff { roots }
}

/// Applies a root delta to a circular root buffer in place.
///
/// Each root stored in `delta` is written to the destination buffer using the
/// same slot-to-index mapping used by [`diff_roots`]:
///
/// ```text
/// buffer_index = slot % buffer_capacity
/// ```
///
/// The first root in the delta corresponds to `base_slot`. Each subsequent
/// root corresponds to the next slot.
///
/// # Arguments
///
/// * `base_slot` - The slot corresponding to the first root stored in `delta`.
/// * `base_buffer` - Destination circular root buffer. It is modified in place.
/// * `delta` - Archived [`RootsDiff`] containing the roots to replay.
///
/// # Correctness
///
/// This function is the application counterpart to [`diff_roots`].
///
/// For correct reconstruction, `base_buffer` must have the same capacity as
/// the buffer that was supplied to [`diff_roots`].
///
/// The delta does not contain explicit buffer indices. Indices are derived
/// from `base_slot` and the destination buffer capacity.
///
/// # Panics
///
/// Panics if `base_buffer` is empty, because circular-buffer indexing requires
/// a non-zero capacity.
///
/// # Example
///
/// ```
/// use eth_state_diff::recent_roots::{apply_roots, diff_roots};
/// use eth_state_diff::types::ArchivedRootsDiff;
///
/// let mut target_buffer = vec![[0u8; 32]; 4];
/// target_buffer[0] = [1u8; 32];
/// target_buffer[1] = [2u8; 32];
/// target_buffer[2] = [3u8; 32];
///
/// let delta = diff_roots(0, 3, &target_buffer);
///
/// let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe {
///     rkyv::access_unchecked::<ArchivedRootsDiff>(&bytes)
/// };
///
/// let mut reconstructed = vec![[0u8; 32]; 4];
/// apply_roots(0, &mut reconstructed, archived);
///
/// assert_eq!(reconstructed, target_buffer);
/// ```
///
/// # Complexity
///
/// If `N` roots are stored in `delta`:
///
/// - Time: O(N)
/// - Additional space: O(1)
pub fn apply_roots(base_slot: u64, base_buffer: &mut [[u8; 32]], delta: &ArchivedRootsDiff) {
    assert!(!base_buffer.is_empty(), "root buffer must not be empty");

    let capacity = base_buffer.len() as u64;

    for (i, root) in delta.roots.iter().enumerate() {
        let slot = base_slot + i as u64;
        let idx = (slot % capacity) as usize;
        base_buffer[idx] = *root;
    }
}
