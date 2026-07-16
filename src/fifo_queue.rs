use crate::types::{ArchivedFifoQueueDiff, FifoQueueDiff};

/// Finds the offset of `needle` in `haystack`. Returns None if not found.
fn find_chunk(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

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
