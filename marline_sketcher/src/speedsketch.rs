//! SpeedSketch: sketch generation for Similarity-Based Chunking.
//!
//! The sketch is a 64-bit vector. Every fingerprint of a chunk is assigned to one of 64 classes,
//! compared against the mask of its class and, if it is a proxy, sets the bit of that class in
//! the sketch. The last 32 bits of the sketch form the feature used to search for a similar base
//! chunk, the whole 64-bit sketch is reused by the delta encoder to decide whether a fingerprint
//! can be present in the base chunk at all.
//!
//! Fingerprints are computed with the very same rolling hash that Gdelta uses for its words, so
//! an identical word yields an identical fingerprint, class and proxy status in both chunks.

use crate::{GEAR, SBCHash, SBCHasher};
use std::hash;

/// Size of the word whose fingerprint is computed, in bytes.
///
/// Must stay in sync with `GdeltaEncoder`, otherwise a word of the base chunk would be
/// classified under a fingerprint that the delta encoder never computes.
pub const WORD_SIZE: usize = 16;

/// Number of bits discarded on every gear step, so that a byte leaves the fingerprint after
/// `WORD_SIZE` steps: 64 / WORD_SIZE.
const MOVE_BTS: u32 = 4;

/// Number of fingerprint classes and therefore the number of sketch bits.
pub const CLASS_NUM: usize = 64;

const CLASS_MASK: u64 = CLASS_NUM as u64 - 1;

/// Predefined masks, one 64-bit mask per fingerprint class, used to sample the class.
///
/// SpeedSketch generates 64 random 64-bit numbers as the masks.
pub const CLASS_MASKS: [u64; CLASS_NUM] = [
    12448694272,
    15669919744,
    51774488576,
    37782290432,
    60934848512,
    11257511936,
    14193524736,
    28487712768,
    66706210816,
    31977373696,
    15300820992,
    56992202752,
    17095983104,
    822083584,
    8629790720,
    27665629184,
    40282095616,
    51959037952,
    58200162304,
    46640660480,
    49023025152,
    63149441024,
    64307068928,
    25618808832,
    57797509120,
    26105348096,
    28890365952,
    54626615296,
    62679678976,
    63786975232,
    31574720512,
    55448698880,
    23051894784,
    9110028288,
    58032390144,
    64256737280,
    57310969856,
    55348035584,
    10636754944,
    16374562816,
    49056579584,
    14344519680,
    11962155008,
    25283264512,
    54660169728,
    67796729856,
    50482642944,
    48133832704,
    29142024192,
    45113933824,
    34024194048,
    40114323456,
    19746783232,
    19528679424,
    25182601216,
    15586033664,
    10619977728,
    53687091200,
    23521656832,
    65162706944,
    43637538816,
    61706600448,
    39409680384,
    21407727616,
];

/// Advances the rolling fingerprint by one byte.
#[inline(always)]
pub fn fp_step(fp: u64, byte: u8) -> u64 {
    (fp << MOVE_BTS).wrapping_add(GEAR[byte as usize])
}

/// Returns the class of a fingerprint.
#[inline(always)]
pub fn class_of(fp: u64) -> u32 {
    (fp & CLASS_MASK) as u32
}

/// Reports whether a fingerprint is a proxy of its class, that is whether all bits
/// selected by the mask of the class are zero.
#[inline(always)]
pub fn is_proxy(fp: u64) -> bool {
    fp & CLASS_MASKS[class_of(fp) as usize] == 0
}

/// Computes the sketch of a chunk.
///
/// Every word of `data` contributes at most one bit: the bit of its class, and only when the
/// word's fingerprint is a proxy. Chunks shorter than a word have no fingerprint and yield an
/// empty sketch.
pub fn sketch(data: &[u8]) -> u64 {
    if data.len() < WORD_SIZE {
        return 0;
    }

    let mut fp = 0u64;
    for &byte in &data[..WORD_SIZE - 1] {
        fp = fp_step(fp, byte);
    }

    let mut sketch = 0u64;
    for &byte in &data[WORD_SIZE - 1..] {
        fp = fp_step(fp, byte);
        if is_proxy(fp) {
            sketch |= 1u64 << class_of(fp);
            if sketch == u64::MAX {
                break;
            }
        }
    }
    sketch
}

/// Extracts the feature used for the similar base chunk search: the last 32 bits of the sketch.
#[inline(always)]
pub fn feature_of(sketch: u64) -> u32 {
    sketch as u32
}

/// Decides whether a word of the input chunk may be present in the base chunk.
///
/// Returns `true` when the class bit is equal in both sketches, in which case the presence of the
/// word has to be confirmed by the Fp table. Returns `false` when the bits differ, which proves
/// that the base chunk holds no proxy of this class and therefore no such word, so the lookup
/// can be skipped and the word encoded as an insertion.
#[inline(always)]
pub fn class_bit_matches(fp: u64, sketch_input: u64, sketch_base: u64) -> bool {
    ((sketch_input ^ sketch_base) & (1u64 << class_of(fp))) == 0
}

/// A similarity hash carrying the full 64-bit sketch of a chunk.
///
/// Two parts are kept side by side: the whole sketch, which the delta encoder uses to
/// prove that a word cannot be present in the base chunk, and the 32-bit feature extracted from
/// it, which is the actual similarity key used for the similar chunk search.
///
/// Equality and hashing are defined on the feature only, so every chunk with the same feature is
/// treated as the same key by the clusterers and the target map.
#[derive(Default, Debug, Clone)]
pub struct SpeedSketchHash {
    /// Full 64-bit sketch, reused by the delta encoder.
    sketch: u64,
    /// Last 32 bits of the sketch, used as the similarity feature.
    feature: [u64; 1],
}

impl SpeedSketchHash {
    fn from_sketch(sketch: u64) -> Self {
        SpeedSketchHash { sketch, feature: [feature_of(sketch) as u64] }
    }
}

impl hash::Hash for SpeedSketchHash {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Chunks are compared and looked up by feature, not by the whole sketch.
        self.feature.hash(state);
    }
}

impl PartialEq for SpeedSketchHash {
    fn eq(&self, other: &Self) -> bool {
        self.feature == other.feature
    }
}

impl Eq for SpeedSketchHash {}

impl SBCHash for SpeedSketchHash {
    fn new_with_u32(key: u32) -> Self {
        SpeedSketchHash::from_sketch(key as u64)
    }

    /// Returns the adjacent *lookup key* used to probe for a free cell in the target map,
    /// not a neighbouring chunk by similarity.
    ///
    /// The sketch is a bitmask, so there is no notion of a "next" sketch: this only derives
    /// an adjacent key (the sketch and its feature are incremented). The resulting sketch is
    /// meaningless as a similarity value and must only be used as a key.
    fn next_hash(&self) -> Self {
        SpeedSketchHash::from_sketch(self.sketch.wrapping_add(1))
    }

    /// Returns the previous adjacent *lookup key*, see [`Self::next_hash`].
    ///
    /// Note that wrapping below zero yields `u64::MAX` (all bits set); that value is again
    /// only a key, not a sketch.
    fn last_hash(&self) -> Self {
        SpeedSketchHash::from_sketch(self.sketch.wrapping_sub(1))
    }

    fn get_key_for_graph_clusterer(&self) -> u32 {
        feature_of(self.sketch)
    }

    fn as_slice(&self) -> &[u64] {
        &self.feature
    }

    fn sketch(&self) -> Option<u64> {
        Some(self.sketch)
    }
}

/// Generates the sketch of a chunk, see [`sketch`].
#[derive(Default, Debug, Clone, Copy)]
pub struct SpeedSketchHasher;

impl SpeedSketchHasher {
    pub fn new() -> Self {
        SpeedSketchHasher
    }
}

impl SBCHasher for SpeedSketchHasher {
    type Hash = SpeedSketchHash;

    fn calculate_hash(&self, chunk_data: &[u8]) -> SpeedSketchHash {
        SpeedSketchHash::from_sketch(sketch(chunk_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn class_masks_are_unique() {
        let unique: HashSet<&u64> = CLASS_MASKS.iter().collect();
        assert_eq!(unique.len(), CLASS_NUM, "CLASS_MASKS contains duplicate masks");
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(7)).collect()
    }

    #[test]
    fn sketch_of_empty_and_subword_chunks_is_zero() {
        assert_eq!(sketch(&[]), 0);
        assert_eq!(sketch(&[0u8; WORD_SIZE - 1]), 0);
    }

    #[test]
    fn sketch_of_word_sized_chunk_is_single_word_contribution() {
        let data = [42u8; WORD_SIZE];
        let mut fp = 0u64;
        for &byte in &data {
            fp = fp_step(fp, byte);
        }
        let expected = if is_proxy(fp) { 1u64 << class_of(fp) } else { 0 };
        assert_eq!(sketch(&data), expected);
    }

    #[test]
    fn sketch_is_deterministic() {
        let data = pattern(4096);
        assert_eq!(sketch(&data), sketch(&data.clone()));
    }

    #[test]
    fn sketch_of_identical_bytes_collapses_to_one_class() {
        // A constant chunk has the same fingerprint in every window, so the sketch can hold
        // at most the single class bit of that fingerprint.
        for byte in [0u8, 1, 0x41, 0xff] {
            let data = vec![byte; 4096];
            let s = sketch(&data);
            assert!(s == 0 || s.is_power_of_two(), "byte {byte:#x} produced {s:#018x}");
        }
    }

    #[test]
    fn single_byte_change_keeps_shared_class_bits() {
        let base = pattern(4096);
        let mut changed = base.clone();
        changed[1234] ^= 0xff;
        assert_ne!(sketch(&base) & sketch(&changed), 0, "similar chunks must share class bits");
    }

    #[test]
    fn class_bit_matches_equal_sketches() {
        let fp = 0xdead_beef_1234_5678;
        let sketch = 1u64 << class_of(fp);
        assert!(class_bit_matches(fp, sketch, sketch));
    }

    #[test]
    fn class_bit_difference_is_detected() {
        let fp = 0xdead_beef_1234_5678;
        let bit = 1u64 << class_of(fp);
        assert!(!class_bit_matches(fp, bit, 0));
        assert!(class_bit_matches(fp, 0, 0));
    }
}
