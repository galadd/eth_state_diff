use rkyv::{Archive, Deserialize, Serialize};

pub const VALIDATOR_SSZ_SIZE: usize = 121;
pub const MIN_VALIDATOR_WITHDRAWABILITY_DELAY: u64 = 256;

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ValidatorField {
    WithdrawalCredentials,
    EffectiveBalance,
    Slashed,
    ActivationEligibilityEpoch,
    ActivationEpoch,
    ExitEpoch,
    WithdrawableEpochSlashed,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorPatch {
    pub index: u32,
    pub field: ValidatorField,
    pub value: Vec<u8>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ValidatorDiffs {
    pub patches: Vec<ValidatorPatch>,
    pub appended_validators: Vec<u8>,
}

/// Compact representation of the difference between two balance snapshots.
///
/// A `BalanceDiffs` stores only the information required to transform one
/// balance vector into another.
///
/// Small balance differences are encoded as zig-zag varints while uncommon
/// values are stored explicitly.
///
/// This structure is intended to be serialized with `rkyv`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BalanceDiffs {
    pub tags: BitTagVec,

    pub mode: i64,

    pub varint_payload: Vec<u8>,
    pub target_values: Vec<u64>,
    pub appended_balances: Vec<u64>,
}

/// Compact delta encoding for participation flags.
///
/// A participation delta stores only the information required to transform one
/// participation vector into another.
///
/// The encoder automatically selects the most compact representation:
///
/// - [`ParticipationDiff::AllZeros`] when the target vector contains only
///   zero-valued flags.
/// - [`ParticipationDiff::Sparse`] when only a subset of indices change.
///
/// This type is intended to be serialized using `rkyv`.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum ParticipationDiff {
    /// Represents a participation vector consisting entirely of zeros.
    ///
    /// During reconstruction the destination vector is resized to `len`
    /// and every entry is set to `0`.
    AllZeros(usize),

    /// Sparse delta representation.
    ///
    /// Only indices whose values differ from the base vector are stored.
    ///
    /// Changed indices are encoded as delta-varint gaps to reduce storage
    /// requirements for sparsely changing participation flags.
    Sparse {
        /// Delta-varint encoded gaps between successive modified indices.
        ///
        /// Each decoded gap is added to the previous modified index to recover
        /// the absolute index.
        sparse_indices: Vec<u8>,

        /// Replacement participation flags.
        ///
        /// Each entry corresponds one-to-one with a decoded index from
        /// `sparse_indices`.
        new_values: Vec<u8>,

        /// Participation flags for validators that exist only in the target
        /// vector.
        extension: Vec<u8>,
    },
}

/// Sequence of roots written while advancing a circular root buffer.
///
/// Roots are stored in chronological slot order beginning at the supplied
/// base slot used during reconstruction.
///
/// The buffer capacity is intentionally omitted from the encoding since it is
/// determined by the destination buffer.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RootsDiffs {
    /// Sequential list of 32-byte hashes added to the circular buffer
    /// between the base slot and target slot.
    pub roots: Vec<[u8; 32]>,
}

#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SlashingsDiffs {
    /// A sparse list of (index, new_slashing_amount).
    /// Index fits in u16 (max size is 8192).
    /// In a 32-epoch window, this Vec will have a length of 0 or 1.
    pub updates: Vec<(u16, u64)>,
}

/// Sequence of RANDAO mixes written while advancing through epochs.
///
/// Mixes are stored in chronological epoch order beginning with the epoch
/// containing the supplied base slot used during reconstruction.
///
/// The circular buffer capacity is intentionally omitted from the encoding, as
/// it is determined by the destination buffer during application.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RandaoDiffs {
    /// Sequential list of 32-byte randao reveals added during this window.
    pub mixes: Vec<[u8; 32]>,
}

#[derive(Eq, PartialEq, Debug, Clone, Default, Archive, Deserialize, Serialize)]
pub struct BitTagVec {
    pub data: Vec<u8>, // 4 entries per byte
    len: usize,
}

pub const SET_NO_CHANGE: u8 = 0b00;
pub const SET_TO_ZERO: u8 = 0b10;
pub const SET_TO_DIFF: u8 = 0b11;
pub const SET_TO_TARGET_VALUE: u8 = 0b01;

/// Dense 2-bit tag vector.
///
/// Four entries are packed into each byte.
///
/// Each entry describes how the corresponding validator balance should be
/// reconstructed.
///
/// Encoding:
///
/// - `00` — unchanged
/// - `01` — absolute target value
/// - `10` — set to zero
/// - `11` — apply encoded difference
impl BitTagVec {
    pub fn new(len: usize) -> Self {
        let bytes = len.div_ceil(4);
        Self {
            data: vec![0; bytes],
            len,
        }
    }

    #[inline]
    pub fn set(&mut self, idx: usize, tag: u8) {
        let byte = idx / 4;
        let shift = (idx % 4) * 2;

        self.data[byte] |= (tag & 0b11) << shift;
    }

    #[inline]
    pub fn get(&self, idx: usize) -> u8 {
        let byte = idx / 4;
        let shift = (idx % 4) * 2;
        (self.data[byte] >> shift) & 0b11
    }
}
