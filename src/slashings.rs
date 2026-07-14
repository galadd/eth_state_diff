use crate::types::{ArchivedSlashingsDiffs, SlashingsDiffs};

/// Diffing operates on the raw u64 circular buffer.
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

// pub fn apply_slashings(
//     base_slot: u64,
//     buffer: &mut [u64],
//     delta: &ArchivedSlashingsDiffs,
//     slots_per_epoch: u64,
// ) {
//     let modulus = buffer.len() as u64;
//
//     for (idx, val) in delta.updates.iter() {
//         // In a 32-epoch window, we are writing the epoch that just finished.
//         // The math ensures we write to the exact correct slot in the circular buffer.
//         let epoch_to_write = (base_slot / slots_per_epoch) + 1;
//         let target_idx = (epoch_to_write % modulus) as usize;
//
//         // Debug assertion ensures the delta's implicit index matches our mathematical index
//         debug_assert_eq!(target_idx, *idx as usize, "Slashing epoch index mismatch");
//
//         buffer[target_idx] = *val;
//     }
// }
