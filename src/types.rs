use rkyv::{Archive, Deserialize, Serialize};

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
