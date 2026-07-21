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

#[allow(unused_variables)]
impl<H, S: Sketch> Store<H, S> for MockStore<H, S>
where
    H: Clone + Eq + std::hash::Hash + Send + Sync,
{
    fn get_sketch(&self, hash: &H) -> Result<Option<S>, IndexError> {
        let sketches = self
            .sketches
            .read()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(sketches.get(hash).cloned())
    }

    fn put_sketch(&self, hash: &H, sketch: &S) -> Result<(), IndexError> {
        let mut sketches = self
            .sketches
            .write()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        sketches.entry(hash.clone()).or_insert(sketch.clone());
        Ok(())
    }

    fn get_inverted(&self, tier: Tier, sf: u64) -> Result<Vec<H>, IndexError> {
        todo!()
    }

    fn len_inverted(&self, tier: Tier) -> Result<usize, IndexError> {
        todo!()
    }

    fn add_inverted(&self, tier: Tier, sf: u64, hash: &H) -> Result<(), IndexError> {
        todo!()
    }

    fn remove_inverted(&self, tier: Tier, sf: u64, hash: &H) -> Result<(), IndexError> {
        todo!()
    }

    fn len_sketches(&self) -> Result<usize, IndexError> {
        todo!()
    }
}
