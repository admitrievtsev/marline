use crate::index::store::{Store, Tier};
use crate::index::{IndexError, SketchKVindex};
use crate::sketch::Sketch;
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;

#[allow(dead_code)]
pub trait SketchKVPalantir<S: Send + Sync + Sketch>: Send + Sync + SketchKVindex<S> {
    fn get_db(&self) -> Result<Option<Arc<Self::Value>>, Self::Error>;

    fn put_db(&self, key: &S) -> Result<(), Self::Error>;

    fn top_k_db(&self, key: &S) -> Result<Vec<(Arc<Self::Value>, usize)>, Self::Error>;
}

#[allow(dead_code)]
fn tier_for_sketch_size(len: usize) -> Result<Tier, IndexError> {
    match len {
        3 => Ok(Tier::One),
        4 => Ok(Tier::Two),
        6 => Ok(Tier::Three),
        _ => Err(IndexError::InvalidSearchOptions),
    }
}

#[allow(dead_code)]
pub struct PalantirIndex<H, S, ST>
where
    H: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<H, S>,
{
    store: ST,

    _phantom: PhantomData<(H, S)>,
}

impl<H, S, ST> PalantirIndex<H, S, ST>
where
    H: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<H, S>,
{
    #[allow(dead_code)]
    pub fn new(store: ST) -> Self {
        Self { store, _phantom: PhantomData }
    }
}

impl<H, S, ST> SketchKVindex<S> for PalantirIndex<H, S, ST>
where
    H: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<H, S>,
{
    type Value = H;
    type Error = IndexError;

    fn len(&self) -> Result<usize, Self::Error> {
        self.store.len_sketches()
    }

    fn get(&self, key: &S) -> Result<Option<Arc<Self::Value>>, Self::Error> {
        let results = self.top_k(key, 1)?;
        Ok(results.into_iter().next().map(|(h, _)| h))
    }

    fn put(&self, key: &S, value: Arc<Self::Value>) -> Result<(), Self::Error> {
        let tier = tier_for_sketch_size(key.len())?;
        for sf in key.iter() {
            self.store.put_inverted(tier, sf, &value)?;
        }
        Ok(())
    }

    fn remove(&self, _key: &S) -> Result<(), Self::Error> {
        todo!("remove requires hash — discuss with team")
    }

    fn top_k(&self, key: &S, k: usize) -> Result<Vec<(Arc<Self::Value>, usize)>, Self::Error> {
        let tier = tier_for_sketch_size(key.len())?;
        let mut counts: HashMap<H, usize> = HashMap::new();

        for sf in key.iter() {
            let hashes = self.store.get_inverted(tier, sf)?;
            for h in hashes {
                *counts.entry(h).or_insert(0) += 1;
            }
        }

        let mut scored: Vec<(Arc<H>, usize)> =
            counts.into_iter().map(|(h, count)| (Arc::new(h), count)).collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.truncate(k);

        Ok(scored)
    }

    fn clear(&self) -> Result<(), Self::Error> {
        todo!("clear requires store to implement clear")
    }
}
