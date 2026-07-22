use crate::index::error::IndexError;
use crate::index::store::{Store, Tier};
use crate::sketch::Sketch;
use std::collections::HashMap;
use std::sync::RwLock;

#[allow(dead_code)]
pub struct IndexStorage<H, S: Sketch> {
    sketches: RwLock<HashMap<H, S>>,
    inverted_t1: RwLock<HashMap<u32, Vec<H>>>,
    inverted_t2: RwLock<HashMap<u32, Vec<H>>>,
    inverted_t3: RwLock<HashMap<u32, Vec<H>>>,
}

#[allow(dead_code)]
impl<H, S: Sketch> IndexStorage<H, S> {
    pub fn new() -> Self {
        Self {
            sketches: RwLock::new(HashMap::new()),
            inverted_t1: RwLock::new(HashMap::new()),
            inverted_t2: RwLock::new(HashMap::new()),
            inverted_t3: RwLock::new(HashMap::new()),
        }
    }
}

#[allow(unused_variables)]
impl<H, S: Sketch> Store<H, S> for IndexStorage<H, S>
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

    fn get_inverted(&self, tier: Tier, sf: u32) -> Result<Vec<H>, IndexError> {
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

    fn put_inverted(&self, tier: Tier, sf: u32, hash: &H) -> Result<(), IndexError> {
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

    fn remove_inverted(&self, tier: Tier, sf: u32, hash: &H) -> Result<(), IndexError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::FixedSketch;

    type Mock = IndexStorage<u64, FixedSketch<6>>;

    fn make_sketch(vals: [u32; 6]) -> FixedSketch<6> {
        FixedSketch::new(vals).unwrap()
    }

    // --- sketch put/get ---

    #[test]
    fn put_and_get_sketch_roundtrip() {
        let store = Mock::new();
        let sk = make_sketch([1, 2, 3, 4, 5, 6]);

        store.put_sketch(&42, &sk).unwrap();
        let got = store.get_sketch(&42).unwrap();

        assert_eq!(got, Some(sk));
    }

    #[test]
    fn get_sketch_nonexistent_returns_none() {
        let store = Mock::new();
        let got = store.get_sketch(&99).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn put_sketch_overwrites_existing() {
        let store = Mock::new();
        let sk1 = make_sketch([1, 2, 3, 4, 5, 6]);
        let sk2 = make_sketch([10, 20, 30, 40, 50, 60]);

        store.put_sketch(&1, &sk1).unwrap();
        store.put_sketch(&1, &sk2).unwrap();

        let got = store.get_sketch(&1).unwrap();
        assert_eq!(got, Some(sk2));
    }

    // --- inverted put/get ---

    #[test]
    fn put_and_get_inverted_single_hash() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &42).unwrap();
        let hashes = store.get_inverted(Tier::One, 100).unwrap();

        assert_eq!(hashes, vec![42]);
    }

    #[test]
    fn put_inverted_multiple_hashes_same_sf_appends() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::One, 100, &2).unwrap();
        store.put_inverted(Tier::One, 100, &3).unwrap();

        let hashes = store.get_inverted(Tier::One, 100).unwrap();
        assert_eq!(hashes, vec![1, 2, 3]);
    }

    #[test]
    fn get_inverted_empty_sf_returns_empty_vec() {
        let store = Mock::new();
        let hashes = store.get_inverted(Tier::One, 999).unwrap();
        assert!(hashes.is_empty());
    }

    #[test]
    fn inverted_tiers_are_independent() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::Two, 100, &2).unwrap();
        store.put_inverted(Tier::Three, 100, &3).unwrap();

        assert_eq!(store.get_inverted(Tier::One, 100).unwrap(), vec![1]);
        assert_eq!(store.get_inverted(Tier::Two, 100).unwrap(), vec![2]);
        assert_eq!(store.get_inverted(Tier::Three, 100).unwrap(), vec![3]);
    }

    // --- remove_inverted ---

    #[test]
    fn remove_inverted_removes_hash_from_list() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::One, 100, &2).unwrap();
        store.put_inverted(Tier::One, 100, &3).unwrap();

        store.remove_inverted(Tier::One, 100, &2).unwrap();

        let hashes = store.get_inverted(Tier::One, 100).unwrap();
        assert_eq!(hashes, vec![1, 3]);
    }

    #[test]
    fn remove_inverted_last_hash_leaves_empty() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &7).unwrap();
        store.remove_inverted(Tier::One, 100, &7).unwrap();

        let hashes = store.get_inverted(Tier::One, 100).unwrap();
        assert!(hashes.is_empty());
    }

    #[test]
    fn remove_inverted_nonexistent_sf_does_not_panic() {
        let store = Mock::new();

        let result = store.remove_inverted(Tier::One, 999, &1);
        assert!(result.is_ok());
    }

    #[test]
    fn remove_inverted_nonexistent_hash_does_not_alter_others() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::One, 100, &2).unwrap();

        store.remove_inverted(Tier::One, 100, &99).unwrap();

        let hashes = store.get_inverted(Tier::One, 100).unwrap();
        assert_eq!(hashes, vec![1, 2]);
    }

    #[test]
    fn remove_inverted_on_one_tier_does_not_affect_another() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::Two, 100, &1).unwrap();

        store.remove_inverted(Tier::One, 100, &1).unwrap();

        assert!(store.get_inverted(Tier::One, 100).unwrap().is_empty());
        assert_eq!(store.get_inverted(Tier::Two, 100).unwrap(), vec![1]);
    }

    // --- len ---

    #[test]
    fn len_sketches_starts_at_zero() {
        let store = Mock::new();
        assert_eq!(store.len_sketches().unwrap(), 0);
    }

    #[test]
    fn len_sketches_increases_after_put() {
        let store = Mock::new();
        store.put_sketch(&1, &make_sketch([1, 2, 3, 4, 5, 6])).unwrap();
        store.put_sketch(&2, &make_sketch([2, 3, 4, 5, 6, 7])).unwrap();
        assert_eq!(store.len_sketches().unwrap(), 2);
    }

    #[test]
    fn len_sketches_does_not_increase_on_overwrite() {
        let store = Mock::new();
        store.put_sketch(&1, &make_sketch([1, 2, 3, 4, 5, 6])).unwrap();
        store.put_sketch(&1, &make_sketch([10, 20, 30, 40, 50, 60])).unwrap();
        assert_eq!(store.len_sketches().unwrap(), 1);
    }

    #[test]
    fn len_inverted_starts_at_zero_per_tier() {
        let store = Mock::new();
        assert_eq!(store.len_inverted(Tier::One).unwrap(), 0);
        assert_eq!(store.len_inverted(Tier::Two).unwrap(), 0);
        assert_eq!(store.len_inverted(Tier::Three).unwrap(), 0);
    }

    #[test]
    fn len_inverted_counts_unique_sfs() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::One, 200, &2).unwrap();
        store.put_inverted(Tier::One, 200, &3).unwrap(); // same sf, no new key

        assert_eq!(store.len_inverted(Tier::One).unwrap(), 2);
    }

    #[test]
    fn len_inverted_per_tier_independent() {
        let store = Mock::new();

        store.put_inverted(Tier::One, 100, &1).unwrap();
        store.put_inverted(Tier::Two, 200, &2).unwrap();
        store.put_inverted(Tier::Two, 300, &3).unwrap();

        assert_eq!(store.len_inverted(Tier::One).unwrap(), 1);
        assert_eq!(store.len_inverted(Tier::Two).unwrap(), 2);
        assert_eq!(store.len_inverted(Tier::Three).unwrap(), 0);
    }

    // --- concurrency ---

    #[test]
    fn concurrent_reads_do_not_deadlock() {
        use std::thread;

        let store = Mock::new();
        let sk = make_sketch([1, 2, 3, 4, 5, 6]);

        store.put_sketch(&1, &sk).unwrap();
        store.put_inverted(Tier::One, 100, &1).unwrap();

        let store = std::sync::Arc::new(store);
        let mut handles = vec![];

        for _ in 0..4 {
            let s = store.clone();
            handles.push(thread::spawn(move || {
                let _sk = s.get_sketch(&1);
                let _inv = s.get_inverted(Tier::One, 100);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }
}
