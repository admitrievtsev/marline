use crate::decoder::Decoder;
use marline_sketcher::SBCHash;
pub use sbc_scrubber::SBCScrubber;
use std::collections::HashMap;

pub mod clusterer;
pub mod decoder;
pub mod encoder;
mod sbc_scrubber;

/// Represents the type of a chunk stored in the filesystem.
///
/// # There are two variants:
/// - `Simple`: The chunk is stored in its entirety (raw data).
/// - `Delta`: The chunk is stored as a delta-encoded difference relative to a parent chunk.
///
/// # Type Parameters
///
/// * `Hash` - A type that implements the `SBCHash` trait, representing the hash of the parent chunk.
#[derive(Hash, PartialEq, Eq, Clone, Default, Debug)]
enum ChunkType<Hash: SBCHash> {
    /// The chunk is stored as a delta relative to a parent chunk.
    Delta {
        /// The hash of the parent chunk.
        parent_hash: Hash,
        /// The delta chunk's sequence number.
        number: u16,
    },
    /// The chunk is stored in full (non-delta).
    #[default]
    Simple,
}

/// A key identifying a chunk stored in the filesystem.
///
/// This structure uniquely represents a chunk by combining its content hash and its storage type.
///
/// # Type Parameters
///
/// * `H` - A hash type implementing the `SBCHash` trait, used to identify the chunk content.
///
/// # Fields
///
/// * `hash` - The hash of the chunk's content.
/// * `chunk_type` - The type of the chunk, indicating whether it is stored as a full chunk or as a delta.
#[derive(Hash, PartialEq, Eq, Clone, Default)]
pub struct SBCKey<H: SBCHash> {
    /// The hash identifying the chunk content.
    hash: H,

    /// The type of the chunk (simple or delta).
    chunk_type: ChunkType<H>,
}

/// A storage map for chunks in the filesystem.
///
/// `SBCMap` manages a collection of chunks identified by their keys (`SBCKey`),
/// storing the raw chunk data as byte vectors. It also holds a decoder instance
/// used for decoding chunk data when needed.
///
/// # Type Parameters
///
/// * `D` - The decoder type implementing the `Decoder` trait, responsible for decoding chunk bytes.
/// * `H` - The hash type implementing the `SBCHash` trait, used to identify chunks.
///
/// # Fields
///
/// * `sbc_hashmap` - A `HashMap` mapping each chunk's key to its raw byte data.
/// * `decoder` - An instance of the decoder used to interpret chunk data.
///
/// # Example
///
/// ```
/// use marline_scrub::decoder::LevenshteinDecoder;
/// use marline_sketcher::AronovichHash;
/// use marline_scrub::SBCMap;
///
/// let mut map: SBCMap<LevenshteinDecoder, AronovichHash> = SBCMap::new(LevenshteinDecoder::default());
/// ```
pub struct SBCMap<D: Decoder, H: SBCHash> {
    /// Internal storage mapping chunk keys to their raw byte content.
    sbc_hashmap: HashMap<SBCKey<H>, Vec<u8>>,

    /// Decoder instance used to decode chunk data.
    decoder: D,
}

impl<D: Decoder, H: SBCHash> SBCMap<D, H> {
    /// Creates a new, empty `SBCMap` with the given decoder.
    ///
    /// # Arguments
    ///
    /// * `_decoder` - An instance of a decoder implementing the `Decoder` trait.
    ///
    /// # Returns
    ///
    /// A new `SBCMap` ready to store chunks and decode them on demand.
    pub fn new(decoder: D) -> Self {
        SBCMap { sbc_hashmap: HashMap::new(), decoder }
    }
}
