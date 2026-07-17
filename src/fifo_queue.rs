//! Delta encoding for fixed-size SSZ FIFO queues.
//!
//! A queue transition is represented as the number of items consumed from the
//! front of the queue together with the raw SSZ bytes of newly appended items.
//!
//! This encoding is suitable for append-only FIFO structures such as the
//! Electra pending operation queues.

use crate::types::{ArchivedFifoQueueDiff, FifoQueueDiff};

/// Finds the first occurrence of an SSZ-encoded queue item within a byte slice.
///
/// Returns the byte offset of the first matching item, or `None` if no match is
/// found.
fn find_chunk(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Computes the delta between two serialized FIFO queues.
///
/// The queues are expected to contain fixed-size SSZ items.
///
/// The resulting delta records:
///
/// - the number of items consumed from the front of the base queue; and
/// - the SSZ bytes of items appended to the end of the queue.
///
/// If no overlap between the queues can be identified, the encoder assumes the
/// base queue was entirely consumed.
///
/// # Algorithm
///
/// The encoder identifies the overlap by locating the first item in the target
/// queue within the base queue. This assumes queue items are sufficiently
/// unique that the first match represents the correct continuation of the FIFO
/// sequence.
pub fn diff_fifo_queue(base_ssz: &[u8], target_ssz: &[u8], item_ssz_size: usize) -> FifoQueueDiff {
    if target_ssz.is_empty() {
        return FifoQueueDiff {
            consumed_count: base_ssz.len() as u32 / item_ssz_size as u32,
            appended_items: Vec::new(),
        };
    }

    // If target is larger than base, no items were consumed, it's pure append
    if target_ssz.len() >= base_ssz.len() {
        let appended = if base_ssz.is_empty() {
            target_ssz.to_vec()
        } else {
            target_ssz[base_ssz.len()..].to_vec()
        };
        return FifoQueueDiff {
            consumed_count: 0,
            appended_items: appended,
        };
    }

    // Target shrank. We need to find how many items were consumed.
    // Take the SSZ bytes of the first item in the target queue
    let target_head = &target_ssz[..item_ssz_size];

    match find_chunk(base_ssz, target_head) {
        Some(byte_offset) => {
            let consumed_count = (byte_offset / item_ssz_size) as u32;
            let remaining_base_items = (base_ssz.len() - byte_offset) as u32;
            let target_item_count = (target_ssz.len() / item_ssz_size) as u32;

            let new_item_count = target_item_count.saturating_sub(remaining_base_items);
            let new_items_start = target_ssz.len() - (new_item_count as usize * item_ssz_size);

            FifoQueueDiff {
                consumed_count,
                appended_items: target_ssz[new_items_start..].to_vec(),
            }
        }
        None => FifoQueueDiff {
            consumed_count: base_ssz.len() as u32 / item_ssz_size as u32,
            appended_items: target_ssz.to_vec(),
        },
    }
}

/// Applies a FIFO queue delta in place.
///
/// The destination queue is updated by:
///
/// 1. Removing `consumed_count` items from the front.
/// 2. Appending the recorded SSZ items to the end.
///
/// If `consumed_count` exceeds the current queue length, the destination queue
/// is cleared before appending.
///
/// # Complexity
///
/// O(queue size + appended bytes)
pub fn apply_fifo_queue(base: &mut Vec<u8>, delta: &ArchivedFifoQueueDiff, item_ssz_size: usize) {
    let bytes_to_drain = delta.consumed_count.to_native() as usize * item_ssz_size;
    if bytes_to_drain > base.len() {
        base.clear();
    } else {
        base.drain(..bytes_to_drain);
    }

    if !delta.appended_items.is_empty() {
        base.extend_from_slice(delta.appended_items.as_slice());
    }
}
