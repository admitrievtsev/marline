use std::sync::Arc;


pub type EntryId = u128;

// !add sketch trait bounds to S!
#[allow(dead_code)]
///Trait for a key-value index
pub trait SketchKvindex<S>: Send + Sync {
    type Value;
    type Error: std::error::Error + Send + Sync + 'static;

    fn len(&self) -> Result<usize, Self::Error>;

    fn get(&self, key: &S) -> Result<Option<Arc<Self::Value>>, Self::Error>;

    fn put(&self, key: &S, value: Arc<Self::Value>) -> Result<PutOutcome, Self::Error>;

    fn remove(&self, key: &S) -> Result<(), Self::Error>;

    fn nearest(
        &self,
        key: &S,
        search_options: SearchOptions,
    ) -> Result<Option<(Arc<Self::Value>, usize)>, Self::Error>;

    fn top_k(
        &self,
        key: &S,
        k: usize,
        search_options: SearchOptions,
    ) -> Result<Vec<(Arc<Self::Value>, usize)>, Self::Error>;

    fn clear() -> Result<(), Self::Error>;
}

#[allow(dead_code)]

//Special return type for put operation, to allow for more flexible queries.
pub enum PutOutcome {
    Inserted { entry_id: EntryId },
    Updated { entry_id: EntryId, previous_entry_id: EntryId },
}

#[allow(dead_code)]
//Special options for nearest neighbor search, to allow for more flexible queries.
#[derive(Default, Clone, Copy, Debug)]
pub struct SearchOptions {
    pub min_intersection: EntryId,
    pub return_zero_overlap: bool,
    pub exclude_exact: bool,
}

#[allow(dead_code)]
#[derive(Default, Clone, Debug)]
pub struct SearchMatch<S, V: Send + Sync> {
    entry_id: usize,
    key: S,
    value: Arc<V>,
    score: SimilarityScore,
}

#[allow(dead_code)]
#[derive(Default, Clone, Copy, Debug)]
pub struct SimilarityScore {
    intersaction: usize,
    union: usize,
}

#[allow(dead_code)]
impl SimilarityScore {
    fn jaccard_similarity(&self) -> f64 {
        self.intersaction as f64 / self.union as f64
    }
    fn from_similar_size_sketches(intersaction: usize, size: usize) -> Self {
        SimilarityScore { intersaction, union: size * 2 - intersaction }
    }
}

#[allow(dead_code)]

pub trait IterableSketchIndex: Iterator {}
