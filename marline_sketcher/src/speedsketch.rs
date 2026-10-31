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
use std::fmt;
use std::hash;
use std::sync::OnceLock;

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

/// Feature-class mask popcount selected for the default configuration.
pub const DEFAULT_FEATURE_POPCOUNT: u32 = 8;

/// Bloom-class mask popcount selected for the default configuration.
pub const DEFAULT_BLOOM_POPCOUNT: u32 = 5;

/// Seed selected for the default mask configuration.
pub const DEFAULT_MASK_SEED: u64 = 12345;

/// Number of low bits used to select the fingerprint class.
const CLASS_BITS: u32 = CLASS_NUM.trailing_zeros();

/// Lowest bit position that may be selected by a mask.
///
/// The low [`CLASS_BITS`] bits of a fingerprint are used to choose its class. A mask must not
/// include them, otherwise classes whose class bits intersect the mask could never produce a
/// proxy.
const FIRST_MASK_BIT: u32 = CLASS_BITS;
const AVAILABLE_MASK_BITS: u32 = 64 - FIRST_MASK_BIT;

/// Errors produced while constructing a [`MaskSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskError {
    /// The requested popcount cannot be represented with the available bits.
    InvalidPopcount { requested: u32, available: u32 },
}

impl fmt::Display for MaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MaskError::InvalidPopcount { requested, available } => {
                write!(f, "popcount {requested} is not in 1..={available}")
            }
        }
    }
}

impl std::error::Error for MaskError {}

/// A generated set of one sampling mask per fingerprint class.
///
/// Masks are the controls that decide how many fingerprints of a class become proxies. Every
/// mask has the configured popcount and leaves the low bits used for classification untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskSet {
    masks: [u64; CLASS_NUM],
}

impl MaskSet {
    /// Generates masks with separate popcounts for the feature classes `0..32` and the bloom
    /// classes `32..64`.
    fn asymmetric(
        feature_popcount: u32,
        bloom_popcount: u32,
        seed: u64,
    ) -> Result<Self, MaskError> {
        validate_popcount(feature_popcount)?;
        validate_popcount(bloom_popcount)?;

        let mut rng = SplitMix64::new(seed);
        let mut masks = [0u64; CLASS_NUM];
        for (class, mask) in masks.iter_mut().enumerate() {
            let popcount = if class < CLASS_NUM / 2 { feature_popcount } else { bloom_popcount };
            *mask = generate_mask(popcount, &mut rng);
        }

        Ok(MaskSet { masks })
    }

    /// Returns the masks as a fixed-size array reference.
    pub fn masks(&self) -> &[u64; CLASS_NUM] {
        &self.masks
    }

    /// Returns the popcount of every mask.
    pub fn popcounts(&self) -> [u32; CLASS_NUM] {
        let mut popcounts = [0u32; CLASS_NUM];
        for (dst, mask) in popcounts.iter_mut().zip(self.masks) {
            *dst = mask.count_ones();
        }
        popcounts
    }

    /// Stable fingerprint of the mask set, used to keep sketches from different configurations
    /// apart.
    pub fn id(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

        let mut hash = FNV_OFFSET;
        for &mask in &self.masks {
            hash ^= mask;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }
}

/// Configuration of the SpeedSketch masks.
///
/// A configuration is a pair of mask popcounts and a seed. The corresponding mask set is
/// generated once when the configuration is created and is then reused for every chunk.
///
/// The default configuration is `feature_popcount = 8`, `bloom_popcount = 5`,
/// `seed = 12345`, selected for FastCDC with 4/8/64 KiB minimum/average/maximum chunk sizes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpeedSketchConfig {
    feature_popcount: u32,
    bloom_popcount: u32,
    seed: u64,
    masks: MaskSet,
    id: u64,
}

impl SpeedSketchConfig {
    /// Builds a configuration and generates its mask set.
    pub fn new(feature_popcount: u32, bloom_popcount: u32, seed: u64) -> Result<Self, MaskError> {
        let masks = MaskSet::asymmetric(feature_popcount, bloom_popcount, seed)?;
        let id = masks.id();
        Ok(SpeedSketchConfig { feature_popcount, bloom_popcount, seed, masks, id })
    }

    /// Builds a configuration with the same popcount for every class.
    pub fn uniform(popcount: u32, seed: u64) -> Result<Self, MaskError> {
        Self::new(popcount, popcount, seed)
    }

    /// Builds a configuration with separate feature and bloom mask popcounts.
    pub fn asymmetric(
        feature_popcount: u32,
        bloom_popcount: u32,
        seed: u64,
    ) -> Result<Self, MaskError> {
        Self::new(feature_popcount, bloom_popcount, seed)
    }

    /// Popcount used by the feature classes `0..32`.
    pub fn feature_popcount(&self) -> u32 {
        self.feature_popcount
    }

    /// Popcount used by the bloom classes `32..64`.
    pub fn bloom_popcount(&self) -> u32 {
        self.bloom_popcount
    }

    /// Seed used to generate the mask set.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Returns the generated mask set.
    pub fn masks(&self) -> &MaskSet {
        &self.masks
    }

    /// Stable identifier of this configuration.
    pub fn id(&self) -> u64 {
        self.id
    }
}

impl Default for SpeedSketchConfig {
    fn default() -> Self {
        default_config().clone()
    }
}

/// Returns the process-wide default configuration, generating its masks only once.
fn default_config() -> &'static SpeedSketchConfig {
    static DEFAULT: OnceLock<SpeedSketchConfig> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        SpeedSketchConfig::new(DEFAULT_FEATURE_POPCOUNT, DEFAULT_BLOOM_POPCOUNT, DEFAULT_MASK_SEED)
            .expect("default SpeedSketch configuration is valid")
    })
}

fn validate_popcount(popcount: u32) -> Result<(), MaskError> {
    if popcount == 0 || popcount > AVAILABLE_MASK_BITS {
        return Err(MaskError::InvalidPopcount {
            requested: popcount,
            available: AVAILABLE_MASK_BITS,
        });
    }
    Ok(())
}

fn generate_mask(popcount: u32, rng: &mut SplitMix64) -> u64 {
    let mut positions: Vec<u8> = (FIRST_MASK_BIT..64).map(|bit| bit as u8).collect();
    for i in 0..popcount as usize {
        let remaining = positions.len() - i;
        let j = i + (rng.next() % remaining as u64) as usize;
        positions.swap(i, j);
    }

    let mut mask = 0u64;
    for &position in &positions[..popcount as usize] {
        mask |= 1u64 << position;
    }
    mask
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

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
    is_proxy_with(fp, default_config().masks())
}

/// Reports whether a fingerprint is a proxy under the given mask set.
#[inline(always)]
pub fn is_proxy_with(fp: u64, masks: &MaskSet) -> bool {
    fp & masks.masks()[class_of(fp) as usize] == 0
}

/// Computes the sketch of a chunk with the default masks.
///
/// Every word of `data` contributes at most one bit: the bit of its class, and only when the
/// word's fingerprint is a proxy. Chunks shorter than a word have no fingerprint and yield an
/// empty sketch.
pub fn sketch(data: &[u8]) -> u64 {
    sketch_with(data, default_config().masks())
}

/// Computes the sketch of a chunk with the given mask set.
pub fn sketch_with(data: &[u8], masks: &MaskSet) -> u64 {
    sketch_in(data, masks.masks())
}

fn sketch_in(data: &[u8], masks: &[u64; CLASS_NUM]) -> u64 {
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
        let class = class_of(fp);
        if fp & masks[class as usize] == 0 {
            sketch |= 1u64 << class;
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
/// Equality and hashing are defined on the feature and the mask configuration, so every chunk
/// with the same feature under the same configuration is treated as the same key by the
/// clusterers and the target map.
#[derive(Debug, Clone)]
pub struct SpeedSketchHash {
    /// Full 64-bit sketch, reused by the delta encoder.
    sketch: u64,
    /// Last 32 bits of the sketch, used as the similarity feature.
    feature: [u64; 1],
    /// Identifier of the mask configuration that produced the sketch.
    config_id: u64,
}

impl SpeedSketchHash {
    fn from_sketch(sketch: u64, config_id: u64) -> Self {
        SpeedSketchHash { sketch, feature: [feature_of(sketch) as u64], config_id }
    }
}

impl Default for SpeedSketchHash {
    fn default() -> Self {
        Self::from_sketch(0, default_config().id())
    }
}

impl hash::Hash for SpeedSketchHash {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        // Chunks are compared and looked up by feature, not by the whole sketch.
        self.feature.hash(state);
        self.config_id.hash(state);
    }
}

impl PartialEq for SpeedSketchHash {
    fn eq(&self, other: &Self) -> bool {
        self.feature == other.feature && self.config_id == other.config_id
    }
}

impl Eq for SpeedSketchHash {}

impl SBCHash for SpeedSketchHash {
    fn new_with_u32(key: u32) -> Self {
        SpeedSketchHash::from_sketch(key as u64, default_config().id())
    }

    fn new_with_u32_like(&self, key: u32) -> Self {
        SpeedSketchHash::from_sketch(key as u64, self.config_id)
    }

    /// Returns the adjacent *lookup key* used to probe for a free cell in the target map,
    /// not a neighbouring chunk by similarity.
    ///
    /// The sketch is a bitmask, so there is no notion of a "next" sketch: this only derives
    /// an adjacent key (the sketch and its feature are incremented). The resulting sketch is
    /// meaningless as a similarity value and must only be used as a key.
    fn next_hash(&self) -> Self {
        SpeedSketchHash::from_sketch(self.sketch.wrapping_add(1), self.config_id)
    }

    /// Returns the previous adjacent *lookup key*, see [`Self::next_hash`].
    ///
    /// Note that wrapping below zero yields `u64::MAX` (all bits set); that value is again
    /// only a key, not a sketch.
    fn last_hash(&self) -> Self {
        SpeedSketchHash::from_sketch(self.sketch.wrapping_sub(1), self.config_id)
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

    fn sketch_config_id(&self) -> Option<u64> {
        Some(self.config_id)
    }
}

/// Generates the sketch of a chunk, see [`sketch`].
#[derive(Default, Debug, Clone)]
pub struct SpeedSketchHasher {
    config: SpeedSketchConfig,
}

impl SpeedSketchHasher {
    /// Creates a hasher with the default mask configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a hasher with the given mask configuration.
    pub fn with_config(config: SpeedSketchConfig) -> Self {
        SpeedSketchHasher { config }
    }

    /// Returns the configuration used by this hasher.
    pub fn config(&self) -> &SpeedSketchConfig {
        &self.config
    }
}

impl SBCHasher for SpeedSketchHasher {
    type Hash = SpeedSketchHash;

    fn calculate_hash(&self, chunk_data: &[u8]) -> SpeedSketchHash {
        SpeedSketchHash::from_sketch(sketch_with(chunk_data, self.config.masks()), self.config.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn default_config_matches_experiment() {
        let default = SpeedSketchConfig::default();
        assert_eq!(default.feature_popcount(), DEFAULT_FEATURE_POPCOUNT);
        assert_eq!(default.bloom_popcount(), DEFAULT_BLOOM_POPCOUNT);
        assert_eq!(default.seed(), DEFAULT_MASK_SEED);
        assert_eq!(
            default,
            SpeedSketchConfig::asymmetric(
                DEFAULT_FEATURE_POPCOUNT,
                DEFAULT_BLOOM_POPCOUNT,
                DEFAULT_MASK_SEED
            )
            .unwrap()
        );
        assert_eq!(default.id(), 0xf22df301964ee3a5);

        let popcounts = default.masks().popcounts();
        assert!(popcounts[..CLASS_NUM / 2].iter().all(|&p| p == DEFAULT_FEATURE_POPCOUNT));
        assert!(popcounts[CLASS_NUM / 2..].iter().all(|&p| p == DEFAULT_BLOOM_POPCOUNT));

        let unique: HashSet<&u64> = default.masks().masks().iter().collect();
        assert_eq!(unique.len(), CLASS_NUM, "default configuration contains duplicate masks");
    }

    #[test]
    fn default_config_is_cached() {
        assert!(std::ptr::eq(default_config(), default_config()));
    }

    #[test]
    fn uniform_masks_have_exact_popcount_and_are_deterministic() {
        for popcount in [1, 4, 8, 16, AVAILABLE_MASK_BITS] {
            let first = SpeedSketchConfig::uniform(popcount, 42).unwrap();
            let second = SpeedSketchConfig::uniform(popcount, 42).unwrap();
            assert_eq!(first, second);
            assert_eq!(first.masks().popcounts(), [popcount; CLASS_NUM]);
            assert_eq!(first.id(), second.id());
        }
    }

    #[test]
    fn asymmetric_masks_split_the_classes() {
        let config = SpeedSketchConfig::asymmetric(9, 5, 7).unwrap();
        let popcounts = config.masks().popcounts();
        assert!(popcounts[..CLASS_NUM / 2].iter().all(|&p| p == 9));
        assert!(popcounts[CLASS_NUM / 2..].iter().all(|&p| p == 5));
    }

    #[test]
    fn mask_generation_rejects_invalid_popcounts() {
        assert!(SpeedSketchConfig::uniform(0, 0).is_err());
        assert!(SpeedSketchConfig::uniform(AVAILABLE_MASK_BITS + 1, 0).is_err());
    }

    #[test]
    fn hashes_with_different_configs_are_not_equal() {
        let data = pattern(4096);
        let default = SpeedSketchHasher::new().calculate_hash(&data);
        let other = SpeedSketchHasher::with_config(SpeedSketchConfig::uniform(8, 1).unwrap())
            .calculate_hash(&data);
        assert_ne!(default, other);
        assert_ne!(default.config_id, other.config_id);
    }

    #[test]
    fn rekeying_preserves_config_id() {
        let config = SpeedSketchConfig::uniform(7, 99).unwrap();
        let hash = SpeedSketchHasher::with_config(config.clone()).calculate_hash(&pattern(64));
        let rekeyed = hash.new_with_u32_like(42);
        assert_eq!(rekeyed.config_id, config.id());
        assert_eq!(rekeyed.get_key_for_graph_clusterer(), 42);
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
