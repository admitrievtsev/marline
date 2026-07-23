#[derive(Clone, Default, PartialEq, Eq)]
pub struct Chunk {
    data: Vec<u8>,
}

impl Chunk {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
