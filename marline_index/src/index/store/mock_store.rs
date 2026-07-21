use crate::index::error::IndexError;
use crate::index::store::{Store, Tier};
use crate::sketch::Sketch;
use std::collections::HashMap;
use std::sync::RwLock;

#[allow(dead_code)]
pub struct MockStore<H, S: Sketch> {
    sketches: RwLock<HashMap<H, S>>,
    inverted_t1: RwLock<HashMap<u64, Vec<H>>>,
    inverted_t2: RwLock<HashMap<u64, Vec<H>>>,
    inverted_t3: RwLock<HashMap<u64, Vec<H>>>,
}

#[allow(dead_code)]
impl<H, S: Sketch> MockStore<H, S> {
    pub fn new() -> Self {
        todo!()
    }
}

impl<H, S: Sketch> Store<H, S> for MockStore<H, S>
where
    H: Clone + Eq + std::hash::Hash + Send + Sync,
{
    fn get_sketch(&self, _hash: &H) -> Result<Option<S>, IndexError> {
        todo!()
    }

    fn put_sketch(&self, _hash: &H, _sketch: &S) -> Result<(), IndexError> {
        todo!()
    }

    fn get_inverted(&self, _tier: Tier, _sf: u64) -> Result<Vec<H>, IndexError> {
        todo!()
    }

    fn add_inverted(&self, _tier: Tier, _sf: u64, _hash: &H) -> Result<(), IndexError> {
        todo!()
    }

    fn remove_inverted(&self, _tier: Tier, _sf: u64, _hash: &H) -> Result<(), IndexError> {
        todo!()
    }

    fn len_sketches(&self) -> Result<usize, IndexError> {
        todo!()
    }

    fn len_inverted(&self, _tier: Tier) -> Result<usize, IndexError> {
        todo!()
    }
}
