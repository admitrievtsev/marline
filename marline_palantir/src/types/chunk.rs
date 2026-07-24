/// A wrapper around raw chunk byte data.
///
/// `Chunk` is used throughout the Palantir pipeline as the input type for
/// super-feature generation and delta encoding.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Chunk {
    data: Vec<u8>,
}

impl Chunk {
    /// Creates a new `Chunk` from a byte vector.
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Returns the underlying byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Returns the length of the chunk data in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the chunk contains no data.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
