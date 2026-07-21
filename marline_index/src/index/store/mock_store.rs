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
        sketches.insert(hash.clone(), sketch.clone());
        Ok(())
    }

    fn get_inverted(&self, tier: Tier, sf: u64) -> Result<Vec<H>, IndexError> {
        let data = match tier {
            Tier::One => self.inverted_t1.read().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Two => self.inverted_t2.read().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Three => self.inverted_t3.read().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
        };
        Ok(data.get(&sf).cloned().unwrap_or_default())
    }

    fn put_inverted(&self, tier: Tier, sf: u64, hash: &H) -> Result<(), IndexError> {
        let mut data = match tier {
            Tier::One => self.inverted_t1.write().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Two => self.inverted_t2.write().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Three => self.inverted_t3.write().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
        };
        data.entry(sf).or_default().push(hash.clone());
        Ok(())
    }

    fn len_inverted(&self, tier: Tier) -> Result<usize, IndexError> {
        let data = match tier {
            Tier::One => self.inverted_t1.read().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Two => self.inverted_t2.read().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Three => self.inverted_t3.read().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
        };
        Ok(data.len())
    }

    fn remove_inverted(&self, tier: Tier, sf: u64, hash: &H) -> Result<(), IndexError> {
        let mut data = match tier {
            Tier::One => self.inverted_t1.write().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Two => self.inverted_t2.write().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
            Tier::Three => self.inverted_t3.write().map_err(|_| {
                IndexError::InternalInvariantViolation(String::from("rwlock poisoned"))
            })?,
        };

        if let Some(hashes) = data.get_mut(&sf) {
            hashes.retain(|h| h != hash);
        }
        Ok(())
    }

    fn len_sketches(&self) -> Result<usize, IndexError> {
        let data = self
            .sketches
            .read()
            .map_err(|_| IndexError::InternalInvariantViolation(String::from("rwlock poisoned")))?;
        Ok(data.len())
    }
}
