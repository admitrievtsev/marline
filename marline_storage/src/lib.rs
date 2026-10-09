pub trait ChunkHash: Clone + Eq + std::hash::Hash + Send + Sync + Default + 'static {}
impl ChunkHash  for [u8; 32] {}

pub trait Database<H: ChunkHash> {
    fn insert(&mut self, hash: H, data: &[u8]);
    fn get(&self, hash: &H) -> Option<Vec<u8>>;
}

pub struct  MetricsDatabase {
    pub total_saved_bytes: usize,
    pub unique_chunks_count: usize,
}

impl MetricsDatabase {
    pub fn new() -> Self {
        Self { total_saved_bytes: 0, unique_chunks_count: 0 }
    }
}

impl<H: ChunkHash> Database<H> for MetricsDatabase {
    fn insert(&mut self, _hash: H, data: &[u8]) {
        self.total_saved_bytes += data.len();
        self.unique_chunks_count += 1;
    }

    fn get(&self, _hash: &H) -> Option<Vec<u8>> {
        None
    }
}