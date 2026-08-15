//! Delta encoding for SSZ lists that behave as FIFO queues.
//!
//! This module computes compact deltas between serialized SSZ queues by
//! identifying items that have been consumed from the front of the queue and
//! items that have been appended to the back.
//!
//! The encoding supports two representations:
//!
//! - [`QueueDiff::Fifo`] records the number of items consumed from the front
//!   and the serialized items appended to the back.
//! - [`QueueDiff::FullReplacement`] stores the complete target queue when the
//!   FIFO relationship cannot be established safely.
//!
//! The FIFO representation is suitable for consensus-layer queues whose
//! logical behavior is append-at-the-back and consume-from-the-front, such as
//! pending withdrawals and consolidations. It also supports queues such as
//! pending deposits where reordering may occur: when the FIFO relationship
//! cannot be proven from the serialized representation, the algorithm falls
//! back to [`QueueDiff::FullReplacement`] rather than producing an unsafe
//! delta.
//!
//! ## Candidate detection and validation
//!
//! To identify a candidate overlap, the encoder searches for the first item of
//! the target queue within the base queue. Matching is performed only at valid
//! SSZ item boundaries determined by `item_ssz_size`.
//!
//! Finding an item alone is not sufficient to establish a FIFO transition.
//! After a candidate overlap is found, the encoder verifies that the remaining
//! bytes of the base queue exactly match the corresponding prefix of the target
//! queue. Only after this validation succeeds is a [`QueueDiff::Fifo`] emitted.
//!
//! If no valid overlap is found, or the remaining queue contents do not match,
//! the encoder emits [`QueueDiff::FullReplacement`] containing the complete
//! target queue.
//!
//! This conservative fallback ensures that an ambiguous or reordered queue
//! is never represented as an incorrect FIFO delta.
//!
//! ## Representation
//!
//! For a valid FIFO transition:
//!
//! ```text
//! base:   [A, B, C, D]
//! target: [C, D, E, F]
//!                 ^--- appended
//!
//! consumed_count = 2
//! appended_items = [E, F]
//! ```
//!
//! Applying the delta removes `A` and `B`, then appends `E` and `F`.
//!
//! ## Requirements
//!
//! `item_ssz_size` must be the fixed serialized SSZ size of one queue item
//! and must be greater than zero. The input buffers are expected to contain
//! complete items, so their lengths should be exact multiples of
//! `item_ssz_size`.
//!
//! The module operates directly on serialized SSZ bytes and does not require
//! deserializing individual queue items during diff generation.
//!
//! ## Complexity
//!
//! [`diff_queue`] performs a linear scan of the base queue for the target head,
//! followed by a linear validation of the candidate overlap. The resulting
//! algorithm is O(n) in the size of the serialized queues.
//!
//! [`apply_queue`] performs O(n) work proportional to the bytes consumed and
//! appended for a FIFO delta, or O(n) in the target queue size for a full
//! replacement.
//!
//! # Example
//!
//! ```
//! # use eth_state_diff::types::QueueDiff;
//! # use eth_state_diff::pending_queue::diff_queue;
//!
//! const ITEM_SIZE: usize = 4;
//!
//! let base = b"AAAABBBBCCCC";
//! let target = b"CCCCDDDDEEEE";
//!
//! let delta = diff_queue(base, target, ITEM_SIZE);
//!
//! assert_eq!(
//!     delta,
//!     QueueDiff::Fifo {
//!         consumed_count: 2,
//!         appended_items: b"DDDDEEEE".to_vec(),
//!     }
//! );
//!

use crate::types::{ArchivedQueueDiff, QueueDiff};

/// Finds the first occurrence of an SSZ-encoded queue item within `haystack`,
/// considering only valid item boundaries.
///
/// The search is performed in `item_ssz_size`-byte chunks rather than with a
/// byte-level substring search. This prevents a sequence of bytes occurring
/// inside one SSZ item from being incorrectly interpreted as a queue-item
/// boundary.
///
/// Returns the byte offset of the first matching item, or `None` if no aligned
/// match exists.
///
/// # Requirements
///
/// `needle.len()` must equal `item_ssz_size`. If it does not, the function
/// returns `None`.
fn find_chunk_aligned(haystack: &[u8], needle: &[u8], item_ssz_size: usize) -> Option<usize> {
    if needle.len() != item_ssz_size {
        return None;
    }

    haystack
        .chunks_exact(item_ssz_size)
        .position(|chunk| chunk == needle)
        .map(|idx| idx * item_ssz_size)
}

/// Computes a delta between two serialized SSZ queues.
///
/// The encoder first attempts to represent the transition as a FIFO operation:
///
/// 1. The first item of `target_ssz` is located in `base_ssz`.
/// 2. The search is restricted to valid item boundaries using
///    `item_ssz_size`.
/// 3. The remaining bytes of the base queue are compared with the corresponding
///    prefix of the target queue.
/// 4. If they match exactly, the transition is represented as
///    [`QueueDiff::Fifo`].
/// 5. Otherwise, the complete target queue is stored as
///    [`QueueDiff::FullReplacement`].
///
/// This validation is important for queues that may occasionally reorder
/// items. An overlap by itself does not prove that the target is a continuation
/// of the base queue.
///
/// # Arguments
///
/// * `base_ssz` - Serialized SSZ representation of the base queue.
/// * `target_ssz` - Serialized SSZ representation of the target queue.
/// * `item_ssz_size` - Fixed serialized SSZ size, in bytes, of one queue item.
///
/// # Returns
///
/// [`QueueDiff::Fifo`] when the target can be safely represented as consumed
/// items followed by appended items. Otherwise returns
/// [`QueueDiff::FullReplacement`] containing the complete target queue.
///
/// # Edge cases
///
/// - If `target_ssz` is empty, all items in the base queue are considered
///   consumed and no items are appended.
/// - If `base_ssz` is empty, the target queue is represented as a pure append.
/// - If the target head cannot be found at an item boundary, the encoder falls
///   back to full replacement.
/// - If the target head is found but the remaining base queue does not exactly
///   match the target prefix, the encoder falls back to full replacement.
///
/// # Panics
///
/// Panics if `item_ssz_size` is zero.
///
/// # Complexity
///
/// O(n) time, where *n* is the combined size of the queues in bytes, with
/// O(m) additional space for the encoded appended or replacement bytes.
///
/// # Example
///
/// ```
/// # use eth_state_diff::pending_queue::diff_queue;
/// # use eth_state_diff::types::QueueDiff;
///
/// const ITEM_SIZE: usize = 4;
///
/// let base = b"AAAABBBBCCCC";
/// let target = b"CCCCDDDDEEEE";
///
/// let delta = diff_queue(base, target, ITEM_SIZE);
///
/// assert_eq!(
///     delta,
///     QueueDiff::Fifo {
///         consumed_count: 2,
///         appended_items: b"DDDDEEEE".to_vec(),
///     }
/// );
/// ```
pub fn diff_queue(base_ssz: &[u8], target_ssz: &[u8], item_ssz_size: usize) -> QueueDiff {
    assert!(item_ssz_size > 0, "item_ssz_size must be greater than 0");

    // Edge case: target is empty, everything was consumed.
    if target_ssz.is_empty() {
        return QueueDiff::Fifo {
            consumed_count: base_ssz.len() as u32 / item_ssz_size as u32,
            appended_items: Vec::new(),
        };
    }

    // Edge case: base is empty, everything is an append.
    if base_ssz.is_empty() {
        return QueueDiff::Fifo {
            consumed_count: 0,
            appended_items: target_ssz.to_vec(),
        };
    }

    let target_head = &target_ssz[..item_ssz_size];

    match find_chunk_aligned(base_ssz, target_head, item_ssz_size) {
        Some(byte_offset) => {
            let remaining_base_bytes = &base_ssz[byte_offset..];
            let expected_target_prefix_len = remaining_base_bytes.len();

            // Validate that the overlapping portion is identical.
            if expected_target_prefix_len <= target_ssz.len()
                && &target_ssz[..expected_target_prefix_len] == remaining_base_bytes
            {
                let consumed_count = (byte_offset / item_ssz_size) as u32;
                let appended_items = target_ssz[expected_target_prefix_len..].to_vec();

                QueueDiff::Fifo {
                    consumed_count,
                    appended_items,
                }
            } else {
                QueueDiff::FullReplacement(target_ssz.to_vec())
            }
        }
        None => QueueDiff::FullReplacement(target_ssz.to_vec()),
    }
}

/// Applies a queue delta to a serialized SSZ queue in place.
///
/// For [`QueueDiff::Fifo`], the specified number of items are removed from the
/// front of `base`, after which the appended serialized items are added to the
/// back.
///
/// For [`QueueDiff::FullReplacement`], the existing queue is cleared and
/// replaced with the serialized target queue stored in the delta.
///
/// # Arguments
///
/// * `base` - Mutable serialized SSZ representation of the queue to update.
/// * `delta` - Archived queue delta previously produced by [`diff_queue`] and
///   serialized with `rkyv`.
/// * `item_ssz_size` - Fixed serialized SSZ size of one queue item.
///
/// # Behavior
///
/// After successful execution, `base` represents the target queue from which
/// the delta was originally generated.
///
/// # Panics
///
/// Panics if the archived delta cannot be deserialized.
///
/// # Complexity
///
/// For [`QueueDiff::Fifo`], the operation is O(n) in the number of bytes
/// removed and appended. Removing bytes from the front may require shifting
/// the remaining contents of the `Vec`.
///
/// For [`QueueDiff::FullReplacement`], the operation is O(n) in the size of
/// the replacement queue.
///
/// # Example
///
/// ```
/// # use eth_state_diff::pending_queue::{apply_queue, diff_queue};
///
/// const ITEM_SIZE: usize = 4;
///
/// let mut base = b"AAAABBBBCCCC".to_vec();
/// let target = b"CCCCDDDDEEEE";
///
/// let delta = diff_queue(&base, target, ITEM_SIZE);
/// let archived = rkyv::to_bytes::<rkyv::rancor::Error>(&delta).unwrap();
/// let archived = unsafe { rkyv::access_unchecked::<
///     eth_state_diff::types::ArchivedQueueDiff
/// >(&archived) };
///
/// apply_queue(&mut base, archived, ITEM_SIZE);
///
/// assert_eq!(base, target);
/// ```
pub fn apply_queue(base: &mut Vec<u8>, delta: &ArchivedQueueDiff, item_ssz_size: usize) {
    let delta: QueueDiff = rkyv::deserialize::<QueueDiff, rkyv::rancor::Error>(delta)
        .expect("Failed to deserialize QueueDiff");

    match delta {
        QueueDiff::Fifo {
            consumed_count,
            appended_items,
        } => {
            let bytes_to_drain = consumed_count as usize * item_ssz_size;

            if bytes_to_drain > base.len() {
                base.clear();
            } else {
                base.drain(..bytes_to_drain);
            }

            if !appended_items.is_empty() {
                base.extend_from_slice(&appended_items);
            }
        }

        QueueDiff::FullReplacement(replacement) => {
            base.clear();

            if !replacement.is_empty() {
                base.extend_from_slice(&replacement);
            }
        }
    }
}
