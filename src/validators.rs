//! Delta encoding and reconstruction for Ethereum validator registries.
//!
//! This module computes compact [`ValidatorsDiff`] values between two validator
//! registries and applies those deltas to reconstruct the target registry.
//!
//! Validator records are fixed-width in their consensus SSZ representation,
//! with [`VALIDATOR_SSZ_SIZE`] bytes per validator. The delta format avoids
//! storing complete validator records when only individual fields have
//! changed.
//!
//! ## What is encoded
//!
//! For validators present in both the base and target registries, the encoder
//! stores patches only for fields whose values changed:
//!
//! - [`ValidatorField::WithdrawalCredentials`]
//! - [`ValidatorField::EffectiveBalance`]
//! - [`ValidatorField::Slashed`]
//! - [`ValidatorField::ActivationEligibilityEpoch`]
//! - [`ValidatorField::ActivationEpoch`]
//! - [`ValidatorField::ExitEpoch`]
//! - [`ValidatorField::WithdrawableEpochSlashed`]
//!
//! Validators that exist only in the target registry are stored as their raw
//! SSZ representations in [`ValidatorsDiff::appended_validators`].
//!
//! The `pubkey` field is not patched because validator public keys are
//! immutable after registration.
//!
//! ## Withdrawable epoch
//!
//! `withdrawable_epoch` is handled specially because its value is derivable
//! for non-slashed validators. When `exit_epoch` changes on a non-slashed
//! validator, the application side reconstructs:
//!
//! ```text
//! withdrawable_epoch = exit_epoch + MIN_VALIDATOR_WITHDRAWABILITY_DELAY
//! ```
//!
//! For slashed validators, `withdrawable_epoch` is explicitly encoded because
//! it is not reconstructed from `exit_epoch` alone.
//!
//! This makes the delta smaller while preserving the target validator state.
//!
//! ## Two integration APIs
//!
//! The module provides two equivalent APIs for different validator storage
//! layouts.
//!
//! ### Contiguous SSZ storage
//!
//! [`diff_validators`] and [`apply_validators`] operate directly on
//! `Vec<u8>`/byte slices containing consecutive SSZ validator records.
//!
//! This is useful for clients that keep their validator registry in a flat
//! SSZ-compatible representation.
//!
//! ### Native client storage
//!
//! [`diff_validators_iter`] and [`apply_validators_iter`] operate through
//! [`ValidatorSnapshot`] and [`ValidatorMutTarget`].
//!
//! This allows a consensus client to diff and reconstruct validators without
//! first converting its native data structure into one large byte buffer.
//!
//! The iterator API is particularly useful for clients whose validator
//! registry is backed by persistent lists, trees, or other non-contiguous
//! structures.
//!
//! ## SSZ representation
//!
//! The flat representation assumed by this module is the canonical
//! consensus-layer validator SSZ layout:
//!
//! | Offset | Size | Field |
//! |---:|---:|---|
//! | `0` | `48` | `pubkey` |
//! | `48` | `32` | `withdrawal_credentials` |
//! | `80` | `8` | `effective_balance` |
//! | `88` | `1` | `slashed` |
//! | `89` | `8` | `activation_eligibility_epoch` |
//! | `97` | `8` | `activation_epoch` |
//! | `105` | `8` | `exit_epoch` |
//! | `113` | `8` | `withdrawable_epoch` |
//!
//! The total size is [`VALIDATOR_SSZ_SIZE`] bytes.
//!
//! ## Complexity
//!
//! Diffing is linear in the number of validators:
//!
//! ```text
//! O(min(base_len, target_len) + appended_validators)
//! ```
//!
//! Applying a delta is linear in the number of patches plus the number of
//! appended validators:
//!
//! ```text
//! O(patches + appended_validators)
//! ```
//!
//! The contiguous byte implementation performs in-place mutation and does not
//! require rebuilding the existing validator registry.
//!
//! ## Delta validity
//!
//! [`apply_validators`] and [`apply_validators_iter`] assume that the supplied
//! delta was produced for the corresponding base validator registry.
//! Application does not independently verify that every patch matches the
//! expected base value.
//!
//! Patch values are expected to have the correct width for their corresponding
//! [`ValidatorField`]. Invalid widths currently result in a panic during
//! reconstruction.
//!
//! Similarly, an out-of-range validator index is ignored by the trait-based
//! application API because [`ValidatorMutTarget::get_mut`] returns `None`.
//!
//! Therefore, deltas should generally be treated as trusted output from
//! [`diff_validators`] or [`diff_validators_iter`], rather than as an
//! independently validated interchange format.
//!
//! [`ValidatorsDiff`]: crate::types::ValidatorsDiff
//! [`ValidatorField`]: crate::types::ValidatorField
//! [`ValidatorSnapshot`]: crate::validators::ValidatorSnapshot
//! [`ValidatorMut`]: crate::validators::ValidatorMut
//! [`ValidatorMutTarget`]: crate::validators::ValidatorMutTarget
//! [`VALIDATOR_SSZ_SIZE`]: crate::types::VALIDATOR_SSZ_SIZE
//! [`MIN_VALIDATOR_WITHDRAWABILITY_DELAY`]: crate::types::MIN_VALIDATOR_WITHDRAWABILITY_DELAY

use crate::types::{
    ArchivedValidatorField, ArchivedValidatorsDiff, ValidatorField, ValidatorPatch, ValidatorsDiff,
    MIN_VALIDATOR_WITHDRAWABILITY_DELAY, VALIDATOR_SSZ_SIZE,
};

/// Read-only view of a validator used by the delta encoder.
///
/// Implementations provide access to the validator fields that can participate
/// in a delta. The trait deliberately does not require a concrete validator
/// type, allowing the same diff algorithm to operate on both flat SSZ records
/// and native consensus-client data structures.
///
/// [`to_ssz_bytes`](Self::to_ssz_bytes) is only required when a validator is
/// present in the target registry but not in the base registry.
pub trait ValidatorSnapshot {
    fn withdrawal_credentials(&self) -> &[u8; 32];
    fn effective_balance(&self) -> u64;
    fn is_slashed(&self) -> bool;
    fn activation_eligibility_epoch(&self) -> u64;
    fn activation_epoch(&self) -> u64;
    fn exit_epoch(&self) -> u64;
    fn withdrawable_epoch(&self) -> u64;

    /// Serializes the validator to its consensus SSZ representation.
    /// Only required for validators appended to the target registry.
    fn to_ssz_bytes(&self) -> Vec<u8>;
}

/// Mutable view of a validator used during delta reconstruction.
///
/// Implementations expose the fields that may be modified by a
/// [`ValidatorField`] patch.
///
/// The current slashed status is required so that applying an `exit_epoch`
/// patch can deterministically reconstruct `withdrawable_epoch` for
/// non-slashed validators.
pub trait ValidatorMut {
    /// Returns the current slashed status. Required for deterministic
    /// reconstruction of `withdrawable_epoch` when `exit_epoch` changes.
    fn is_slashed(&self) -> bool;

    fn set_withdrawal_credentials(&mut self, value: &[u8; 32]);
    fn set_effective_balance(&mut self, value: u64);
    fn set_slashed(&mut self, value: bool);
    fn set_activation_eligibility_epoch(&mut self, value: u64);
    fn set_activation_epoch(&mut self, value: u64);
    fn set_exit_epoch(&mut self, value: u64);
    fn set_withdrawable_epoch(&mut self, value: u64);
}

/// Mutable access to a validator collection used during reconstruction.
///
/// This trait abstracts over the client's validator storage representation.
/// Implementations may back the collection with a contiguous vector, a
/// persistent list, a tree, or another native data structure.
///
/// The collection is expected to preserve validator ordering: validator index
/// `i` in the delta must refer to the same validator index `i` in the target
/// collection.
///
/// New validators are supplied as complete consensus SSZ records through
/// [`push_from_ssz`](Self::push_from_ssz).
pub trait ValidatorMutTarget {
    type Validator<'a>: ValidatorMut
    where
        Self: 'a;

    /// Fetches a mutable validator view by index.
    fn get_mut(&mut self, index: usize) -> Option<Self::Validator<'_>>;

    /// Appends a newly seen validator from its raw SSZ bytes.
    fn push_from_ssz(&mut self, ssz_bytes: &[u8]);
}

/// Computes a compact validator delta from two contiguous SSZ byte buffers.
///
/// Both buffers must contain zero or more complete validator records, with
/// each record occupying exactly [`VALIDATOR_SSZ_SIZE`] bytes.
///
/// The resulting [`ValidatorsDiff`] contains:
///
/// - field-level patches for validators present in both buffers; and
/// - complete SSZ records for validators appended to the target buffer.
///
/// The function does not allocate or deserialize individual validator
/// structures while scanning the common portion of the buffers. Fields are
/// read directly from their fixed offsets in the SSZ representation.
///
/// # Panics
///
/// Panics if either input contains a trailing partial validator record.
///
/// # Examples
///
/// A delta can be applied to the original buffer to reconstruct the target:
///
/// ```ignore
/// let delta = diff_validators(&base, &target);
/// apply_validators(&mut base, &delta);
///
/// assert_eq!(base, target);
/// ```
///
/// [`VALIDATOR_SSZ_SIZE`]: crate::types::VALIDATOR_SSZ_SIZE
pub fn diff_validators(base_bytes: &[u8], target_bytes: &[u8]) -> ValidatorsDiff {
    diff_validators_impl(
        base_bytes
            .chunks_exact(VALIDATOR_SSZ_SIZE)
            .map(ByteValidator),
        target_bytes
            .chunks_exact(VALIDATOR_SSZ_SIZE)
            .map(ByteValidator),
    )
}

/// Computes a compact validator delta using client-provided validator views.
///
/// This is the native-storage counterpart to [`diff_validators`]. Instead of
/// requiring the validator registries to be represented as contiguous SSZ
/// buffers, the caller supplies iterators of [`ValidatorSnapshot`] values.
///
/// Only the fields exposed by [`ValidatorSnapshot`] are inspected. Validators
/// that occur only in the target iterator are serialized through
/// [`ValidatorSnapshot::to_ssz_bytes`] and stored in the resulting delta.
///
/// The iterators must report their exact validator counts through
/// [`ExactSizeIterator`].
///
/// This function is useful when a consensus client stores validators in a
/// persistent list, tree, or another non-contiguous data structure and wants
/// to avoid materializing the complete registry as SSZ bytes.
///
/// # Panics
///
/// Panics if an iterator's reported length is inconsistent with the number of
/// items it yields.
///
/// [`diff_validators`]: crate::validators::diff_validators
/// [`ValidatorSnapshot`]: crate::validators::ValidatorSnapshot
pub fn diff_validators_iter<I1, I2, V1, V2>(base: I1, target: I2) -> ValidatorsDiff
where
    I1: ExactSizeIterator<Item = V1>,
    I2: ExactSizeIterator<Item = V2>,
    V1: ValidatorSnapshot,
    V2: ValidatorSnapshot,
{
    diff_validators_impl(base, target)
}

/// The core diffing algorithm, agnostic to the input memory layout.
fn diff_validators_impl<I1, I2, V1, V2>(mut base: I1, mut target: I2) -> ValidatorsDiff
where
    I1: ExactSizeIterator<Item = V1>,
    I2: ExactSizeIterator<Item = V2>,
    V1: ValidatorSnapshot,
    V2: ValidatorSnapshot,
{
    let common_len = base.len().min(target.len());
    let mut patches = Vec::with_capacity(512);
    let mut appended_validators = Vec::new();

    for i in 0..common_len {
        let b = base.next().unwrap();
        let t = target.next().unwrap();

        let wc = t.withdrawal_credentials();
        let eb = t.effective_balance();
        let slashed = t.is_slashed();
        let aee = t.activation_eligibility_epoch();
        let ae = t.activation_epoch();
        let ee = t.exit_epoch();

        if b.withdrawal_credentials() == wc
            && b.effective_balance() == eb
            && b.is_slashed() == slashed
            && b.activation_eligibility_epoch() == aee
            && b.activation_epoch() == ae
            && b.exit_epoch() == ee
            && (!slashed || b.withdrawable_epoch() == t.withdrawable_epoch())
        {
            continue;
        }

        let index = i as u32;

        if b.withdrawal_credentials() != wc {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::WithdrawalCredentials,
                value: wc.to_vec(),
            });
        }
        if b.effective_balance() != eb {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::EffectiveBalance,
                value: eb.to_le_bytes().to_vec(),
            });
        }
        if b.is_slashed() != slashed {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::Slashed,
                value: vec![slashed as u8],
            });
        }
        if b.activation_eligibility_epoch() != aee {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::ActivationEligibilityEpoch,
                value: aee.to_le_bytes().to_vec(),
            });
        }
        if b.activation_epoch() != ae {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::ActivationEpoch,
                value: ae.to_le_bytes().to_vec(),
            });
        }
        if b.exit_epoch() != ee {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::ExitEpoch,
                value: ee.to_le_bytes().to_vec(),
            });
        }
        if slashed && b.withdrawable_epoch() != t.withdrawable_epoch() {
            patches.push(ValidatorPatch {
                index,
                field: ValidatorField::WithdrawableEpochSlashed,
                value: t.withdrawable_epoch().to_le_bytes().to_vec(),
            });
        }
    }

    for t_val in target {
        appended_validators.extend(t_val.to_ssz_bytes());
    }

    ValidatorsDiff {
        patches,
        appended_validators,
    }
}

/// Applies a validator delta to a contiguous SSZ byte buffer in place.
///
/// The buffer must represent the same base validator registry against which
/// the delta was generated.
///
/// Existing validators are modified according to the delta's field patches.
/// Newly appended validators are added from their raw SSZ representations.
///
/// This function does not require deserializing the validator registry into
/// Rust validator structures. Existing records are modified directly at their
/// fixed SSZ offsets.
///
/// # Panics
///
/// Panics if a patch contains a value with an invalid width for its field.
///
/// # Correctness
///
/// The delta is expected to have been generated from the same base registry.
/// This function does not verify that the current contents of the buffer
/// correspond to the original base state.
///
/// [`ArchivedValidatorsDiff`]: crate::types::ArchivedValidatorsDiff
pub fn apply_validators(base: &mut Vec<u8>, delta: &ArchivedValidatorsDiff) {
    apply_validators_iter(&mut ByteValidatorTarget(base), delta)
}

/// Applies a validator delta directly to a client's native validator
/// collection.
///
/// The target collection is accessed through [`ValidatorMutTarget`], allowing
/// the caller to mutate validators without converting the entire registry
/// into a contiguous SSZ byte buffer.
///
/// For every patch, the corresponding validator is obtained with
/// [`ValidatorMutTarget::get_mut`] and the specified field is updated.
///
/// Newly appended validators are passed to
/// [`ValidatorMutTarget::push_from_ssz`] as complete
/// [`VALIDATOR_SSZ_SIZE`]-byte SSZ records.
///
/// For a non-slashed validator whose `exit_epoch` is patched, the
/// `withdrawable_epoch` is reconstructed automatically as:
///
/// ```text
/// exit_epoch + MIN_VALIDATOR_WITHDRAWABILITY_DELAY
/// ```
///
/// For slashed validators, `withdrawable_epoch` is restored explicitly from
/// the [`ValidatorField::WithdrawableEpochSlashed`] patch.
///
/// # Missing validators
///
/// If [`ValidatorMutTarget::get_mut`] returns `None` for a patch index, that
/// patch is skipped.
///
/// Implementations should therefore ensure that the target collection has the
/// same base validator ordering and length expected by the delta.
///
/// # Panics
///
/// Panics if a patch contains a value with an invalid width for its field.
///
/// [`ValidatorMutTarget`]: crate::validators::ValidatorMutTarget
/// [`ValidatorField::WithdrawableEpochSlashed`]:
///     crate::types::ValidatorField::WithdrawableEpochSlashed
/// [`VALIDATOR_SSZ_SIZE`]: crate::types::VALIDATOR_SSZ_SIZE
/// [`MIN_VALIDATOR_WITHDRAWABILITY_DELAY`]:
///     crate::types::MIN_VALIDATOR_WITHDRAWABILITY_DELAY
pub fn apply_validators_iter<T: ValidatorMutTarget>(
    target: &mut T,
    delta: &ArchivedValidatorsDiff,
) {
    for patch in delta.patches.iter() {
        let idx = patch.index.to_native() as usize;
        let val_bytes = patch.value.as_slice();

        if let Some(mut validator) = target.get_mut(idx) {
            match &patch.field {
                ArchivedValidatorField::WithdrawalCredentials => {
                    let bytes: [u8; 32] = val_bytes.try_into().unwrap();
                    validator.set_withdrawal_credentials(&bytes);
                }
                ArchivedValidatorField::EffectiveBalance => {
                    let eb = u64::from_le_bytes(val_bytes.try_into().unwrap());
                    validator.set_effective_balance(eb);
                }
                ArchivedValidatorField::Slashed => {
                    validator.set_slashed(val_bytes[0] != 0);
                }
                ArchivedValidatorField::ActivationEligibilityEpoch => {
                    let epoch = u64::from_le_bytes(val_bytes.try_into().unwrap());
                    validator.set_activation_eligibility_epoch(epoch);
                }
                ArchivedValidatorField::ActivationEpoch => {
                    let epoch = u64::from_le_bytes(val_bytes.try_into().unwrap());
                    validator.set_activation_epoch(epoch);
                }
                ArchivedValidatorField::ExitEpoch => {
                    let ee = u64::from_le_bytes(val_bytes.try_into().unwrap());
                    validator.set_exit_epoch(ee);

                    // Deterministic reconstruction for non-slashed validators
                    if !validator.is_slashed() {
                        let we = ee.saturating_add(MIN_VALIDATOR_WITHDRAWABILITY_DELAY);
                        validator.set_withdrawable_epoch(we);
                    }
                }
                ArchivedValidatorField::WithdrawableEpochSlashed => {
                    let we = u64::from_le_bytes(val_bytes.try_into().unwrap());
                    validator.set_withdrawable_epoch(we);
                }
            }
        }
    }

    debug_assert_eq!(
        delta.appended_validators.len() % VALIDATOR_SSZ_SIZE,
        0,
        "appended validator data must contain complete SSZ records",
    );

    for chunk in delta
        .appended_validators
        .as_slice()
        .chunks_exact(VALIDATOR_SSZ_SIZE)
    {
        target.push_from_ssz(chunk);
    }
}

/// A zero-cost wrapper that allows byte slices to implement `ValidatorSnapshot`.
struct ByteValidator<'a>(&'a [u8]);

impl<'a> ValidatorSnapshot for ByteValidator<'a> {
    #[inline]
    fn withdrawal_credentials(&self) -> &[u8; 32] {
        self.0[48..80].try_into().unwrap()
    }
    #[inline]
    fn effective_balance(&self) -> u64 {
        u64::from_le_bytes(self.0[80..88].try_into().unwrap())
    }
    #[inline]
    fn is_slashed(&self) -> bool {
        self.0[88] != 0
    }
    #[inline]
    fn activation_eligibility_epoch(&self) -> u64 {
        u64::from_le_bytes(self.0[89..97].try_into().unwrap())
    }
    #[inline]
    fn activation_epoch(&self) -> u64 {
        u64::from_le_bytes(self.0[97..105].try_into().unwrap())
    }
    #[inline]
    fn exit_epoch(&self) -> u64 {
        u64::from_le_bytes(self.0[105..113].try_into().unwrap())
    }
    #[inline]
    fn withdrawable_epoch(&self) -> u64 {
        u64::from_le_bytes(self.0[113..121].try_into().unwrap())
    }
    #[inline]
    fn to_ssz_bytes(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

/// Mutable byte slice wrapper implementing `ValidatorMut`.
struct ByteValidatorMut<'a>(&'a mut [u8]);

impl<'a> ValidatorMut for ByteValidatorMut<'a> {
    #[inline]
    fn is_slashed(&self) -> bool {
        self.0[88] != 0
    }

    #[inline]
    fn set_withdrawal_credentials(&mut self, v: &[u8; 32]) {
        self.0[48..80].copy_from_slice(v);
    }
    #[inline]
    fn set_effective_balance(&mut self, v: u64) {
        self.0[80..88].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn set_slashed(&mut self, v: bool) {
        self.0[88] = v as u8;
    }
    #[inline]
    fn set_activation_eligibility_epoch(&mut self, v: u64) {
        self.0[89..97].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn set_activation_epoch(&mut self, v: u64) {
        self.0[97..105].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn set_exit_epoch(&mut self, v: u64) {
        self.0[105..113].copy_from_slice(&v.to_le_bytes());
    }
    #[inline]
    fn set_withdrawable_epoch(&mut self, v: u64) {
        self.0[113..121].copy_from_slice(&v.to_le_bytes());
    }
}

/// Collection wrapper implementing `ValidatorMutTarget` for `Vec<u8>`.
struct ByteValidatorTarget<'a>(&'a mut Vec<u8>);

impl<'a> ValidatorMutTarget for ByteValidatorTarget<'a> {
    type Validator<'b>
        = ByteValidatorMut<'b>
    where
        Self: 'b;

    fn get_mut(&mut self, index: usize) -> Option<Self::Validator<'_>> {
        let start = index.checked_mul(VALIDATOR_SSZ_SIZE)?;
        let end = start.checked_add(VALIDATOR_SSZ_SIZE)?;

        if end > self.0.len() {
            return None;
        }

        let slice = unsafe {
            let ptr = self.0.as_mut_ptr().add(start);
            std::slice::from_raw_parts_mut(ptr, VALIDATOR_SSZ_SIZE)
        };

        Some(ByteValidatorMut(slice))
    }

    fn push_from_ssz(&mut self, ssz_bytes: &[u8]) {
        self.0.extend_from_slice(ssz_bytes);
    }
}
