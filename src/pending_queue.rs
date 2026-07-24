//! Universal delta encoding for SSZ lists that behave as FIFO queues.
//!
//! This module handles both pure FIFO queues (e.g., consolidations, withdrawals)
//! and logically-FIFO queues with rare reordering (e.g., deposits).
//!
//! It guarantees correctness by strictly validating item boundaries during overlap
//! detection and ensuring the remaining base queue matches the target prefix exactly.

use crate::types::{ArchivedQueueDiff, QueueDiff};

/// Finds the first occurrence of an SSZ-encoded queue item within a byte slice,
/// strictly checking only at valid SSZ item boundaries to prevent false positives.
fn find_chunk_aligned(haystack: &[u8], needle: &[u8], item_ssz_size: usize) -> Option<usize> {
    if needle.len() != item_ssz_size {
        return None;
    }
    haystack
        .chunks_exact(item_ssz_size)
        .position(|chunk| chunk == needle)
        .map(|idx| idx * item_ssz_size)
}

/// Computes the delta between two serialized queue lists.
///
/// # Algorithm
///
/// 1. Locates the first item of the target queue within the base queue **only at
///    valid item boundaries**.
/// 2. **Strictly validates** that the remaining items in the base queue are
///    exactly identical to the prefix of the target queue.
/// 3. If valid, emits `QueueDiff::Fifo`. If invalid, emits `QueueDiff::FullReplacement`.
pub fn diff_queue(base_ssz: &[u8], target_ssz: &[u8], item_ssz_size: usize) -> QueueDiff {
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

            // CRITICAL VALIDATION: Ensure the rest of the base queue exactly matches
            // the prefix of the target queue. This catches any reordering edge cases.
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
                // Overlap found, but the tails don't match. Fallback to safe replacement.
                QueueDiff::FullReplacement(target_ssz.to_vec())
            }
        }
        None => {
            // The first item of the target does not exist anywhere in the base.
            // This happens if the base was entirely consumed. Fallback to safe replacement.
            QueueDiff::FullReplacement(target_ssz.to_vec())
        }
    }
}

/// Applies a universal queue delta in place.
///
/// # Complexity
///
/// - `Fifo`: O(consumed bytes + appended bytes)
/// - `FullReplacement`: O(target queue size)
pub fn apply_queue(base: &mut Vec<u8>, delta: &ArchivedQueueDiff, item_ssz_size: usize) {
    let delta: QueueDiff = rkyv::deserialize::<QueueDiff, rkyv::rancor::Error>(delta)
        .expect("Failed to deserialize PendingDepositsDiff");

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
