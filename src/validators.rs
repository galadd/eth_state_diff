//! Delta encoding for Ethereum validator records.
//!
//! This module operates directly on the canonical SSZ byte representation of
//! validators rather than materializing Rust validator structs.
//!
//! Working directly on SSZ bytes provides several advantages:
//!
//! - avoids per-validator deserialization,
//! - performs only fixed-offset memory comparisons,
//! - minimizes allocations, and
//! - integrates naturally with consensus clients that already store validator
//!   registries as contiguous SSZ data.
//!
//! Only mutable validator fields defined by the Ethereum consensus protocol are
//! encoded. Immutable fields such as the validator public key are never included
//! in generated patches.
//!
//! Certain protocol-derived fields are reconstructed deterministically during
//! application instead of being stored explicitly. For example,
//! `withdrawable_epoch` for non-slashed validators is recomputed from
//! `exit_epoch` according to the Ethereum consensus specification, reducing the
//! size of the encoded delta without losing information.

use crate::types::{
    ArchivedValidatorField, ArchivedValidatorsDiff, ValidatorField, ValidatorPatch, ValidatorsDiff,
    MIN_VALIDATOR_WITHDRAWABILITY_DELAY, VALIDATOR_SSZ_SIZE,
};

/// Computes a compact delta between two validator registry SSZ buffers.
///
/// Both inputs must contain contiguous serialized validators in canonical SSZ
/// form.
///
/// The encoder compares validators field-by-field using fixed protocol offsets.
/// Only fields whose values differ are emitted as patches.
///
/// Immutable validator fields are ignored, while protocol-derived fields are
/// omitted whenever they can be reconstructed deterministically during
/// application.
///
/// Validators appended to the end of the registry are copied verbatim into the
/// resulting delta.
///
/// # Arguments
///
/// * `base_bytes` - Base validator registry as contiguous SSZ bytes.
/// * `target_bytes` - Target validator registry as contiguous SSZ bytes.
///
/// # Complexity
///
/// O(n), where *n* is the number of validators.
///
/// # Panics
///
/// Never.
pub fn diff_validators(base_bytes: &[u8], target_bytes: &[u8]) -> ValidatorsDiff {
    let base_len = base_bytes.len() / VALIDATOR_SSZ_SIZE;
    let target_len = target_bytes.len() / VALIDATOR_SSZ_SIZE;
    let common_len = base_len.min(target_len);

    let mut patches = Vec::with_capacity(512);

    for i in 0..common_len {
        let b_start = i * VALIDATOR_SSZ_SIZE;
        let t_start = i * VALIDATOR_SSZ_SIZE;

        // Fast path: validator is identical.
        if base_bytes[b_start..b_start + VALIDATOR_SSZ_SIZE]
            == target_bytes[t_start..t_start + VALIDATOR_SSZ_SIZE]
        {
            continue;
        }

        // 1. Withdrawal Credentials (Offset 48, 32 bytes)
        let base_wc = &base_bytes[b_start + 48..b_start + 80];
        let target_wc = &target_bytes[t_start + 48..t_start + 80];
        if base_wc != target_wc {
            patches.push(ValidatorPatch {
                index: i as u32,
                field: ValidatorField::WithdrawalCredentials,
                value: target_wc.to_vec(),
            });
        }

        // 2. Effective Balance (Offset 80, 8 bytes)
        let base_eb = &base_bytes[b_start + 80..b_start + 88];
        let target_eb = &target_bytes[t_start + 80..t_start + 88];
        if base_eb != target_eb {
            patches.push(ValidatorPatch {
                index: i as u32,
                field: ValidatorField::EffectiveBalance,
                value: target_eb.to_vec(),
            });
        }

        // 3. Slashed (Offset 88, 1 byte)
        if base_bytes[b_start + 88] != target_bytes[t_start + 88] {
            patches.push(ValidatorPatch {
                index: i as u32,
                field: ValidatorField::Slashed,
                value: vec![target_bytes[t_start + 88]],
            });
        }

        // 4. Activation Eligibility Epoch (Offset 89, 8 bytes)
        let base_aee = &base_bytes[b_start + 89..b_start + 97];
        let target_aee = &target_bytes[t_start + 89..t_start + 97];
        if base_aee != target_aee {
            patches.push(ValidatorPatch {
                index: i as u32,
                field: ValidatorField::ActivationEligibilityEpoch,
                value: target_aee.to_vec(),
            });
        }

        // 5. Activation Epoch (Offset 97, 8 bytes)
        let base_ae = &base_bytes[b_start + 97..b_start + 105];
        let target_ae = &target_bytes[t_start + 97..t_start + 105];
        if base_ae != target_ae {
            patches.push(ValidatorPatch {
                index: i as u32,
                field: ValidatorField::ActivationEpoch,
                value: target_ae.to_vec(),
            });
        }

        // 6. Exit Epoch (Offset 105, 8 bytes)
        let base_ee = &base_bytes[b_start + 105..b_start + 113];
        let target_ee = &target_bytes[t_start + 105..t_start + 113];
        if base_ee != target_ee {
            patches.push(ValidatorPatch {
                index: i as u32,
                field: ValidatorField::ExitEpoch,
                value: target_ee.to_vec(),
            });
        }

        // 7. Withdrawable Epoch (only stored for slashed validators)
        if target_bytes[t_start + 88] != 0 {
            let base_we = &base_bytes[b_start + 113..b_start + 121];
            let target_we = &target_bytes[t_start + 113..t_start + 121];

            if base_we != target_we {
                patches.push(ValidatorPatch {
                    index: i as u32,
                    field: ValidatorField::WithdrawableEpochSlashed,
                    value: target_we.to_vec(),
                });
            }
        }
    }

    // Handle appended validators (raw SSZ tail).
    let appended_start = common_len * VALIDATOR_SSZ_SIZE;
    let appended_validators = if target_bytes.len() > appended_start {
        target_bytes[appended_start..].to_vec()
    } else {
        Vec::new()
    };

    ValidatorsDiff {
        patches,
        appended_validators,
    }
}

/// Applies a validator delta to an SSZ validator registry in-place.
///
/// After successful application, `base` is byte-for-byte identical to the
/// original target registry used to produce `delta`.
///
/// For non-slashed validators, `withdrawable_epoch` is reconstructed
/// deterministically from `exit_epoch` according to the Ethereum consensus
/// specification rather than being stored explicitly in the delta.
///
/// Newly appended validators are copied directly to the end of the registry.
///
/// # Panics
///
/// Panics in debug builds if a patch references a validator outside the current
/// registry.
///
/// Supplying a delta that was not produced from the provided base registry
/// results in undefined reconstructed state.
pub fn apply_validators(base: &mut Vec<u8>, delta: &ArchivedValidatorsDiff) {
    for patch in delta.patches.iter() {
        let idx = patch.index.to_native() as usize;
        let start = idx * VALIDATOR_SSZ_SIZE;

        // Safety check: Ensure we don't write past the end of existing validators
        debug_assert!(
            start + VALIDATOR_SSZ_SIZE <= base.len(),
            "Patch index out of bounds"
        );

        let val_bytes = patch.value.as_slice();

        match &patch.field {
            ArchivedValidatorField::WithdrawalCredentials => {
                base[start + 48..start + 80].copy_from_slice(val_bytes);
            }

            ArchivedValidatorField::EffectiveBalance => {
                base[start + 80..start + 88].copy_from_slice(val_bytes);
            }

            ArchivedValidatorField::Slashed => {
                // val_bytes is guaranteed to be 1 byte from the diff logic
                base[start + 88] = val_bytes[0];
            }

            ArchivedValidatorField::ActivationEligibilityEpoch => {
                base[start + 89..start + 97].copy_from_slice(val_bytes);
            }

            ArchivedValidatorField::ActivationEpoch => {
                base[start + 97..start + 105].copy_from_slice(val_bytes);
            }

            ArchivedValidatorField::ExitEpoch => {
                base[start + 105..start + 113].copy_from_slice(val_bytes);

                // DETERMINISTIC RECONSTRUCTION
                // Check the SLASHED flag at offset 88 of the CURRENT state.
                // (It might have just been updated in this same loop, which is correct).
                let is_slashed = base[start + 88] != 0;

                if !is_slashed {
                    // Parse the new exit_epoch from the patch value (little-endian)
                    let exit_bytes: [u8; 8] = val_bytes
                        .try_into()
                        .expect("Exit epoch patch must be 8 bytes");
                    let exit_epoch = u64::from_le_bytes(exit_bytes);

                    // Calculate and write withdrawable_epoch
                    let withdrawable_epoch =
                        exit_epoch.saturating_add(MIN_VALIDATOR_WITHDRAWABILITY_DELAY);
                    base[start + 113..start + 121]
                        .copy_from_slice(&withdrawable_epoch.to_le_bytes());
                }
            }

            ArchivedValidatorField::WithdrawableEpochSlashed => {
                // Only reached if the validator is slashed. We just write the raw bytes.
                base[start + 113..start + 121].copy_from_slice(val_bytes);
            }
        }
    }

    // Append new validators
    if !delta.appended_validators.is_empty() {
        base.extend_from_slice(delta.appended_validators.as_slice());
    }
}
