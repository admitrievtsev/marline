use crate::index::SketchKVindex;
use crate::index::store::{self, Store};
use crate::sketch::Sketch;
use std::hash::Hash;
use std::marker::PhantomData;

pub struct PalantirIndex<H, S, ST>
where
    H: Clone + Eq + Hash + Send + Sync,
    S: Sketch,
    ST: Store<H, S>,
{
    store: ST,
    /// added to avoid "unusedes type perameter"
    /// i ain't 100 percents sure about it,
    /// if you can explain why we should delete, then delete
    _phantom: PhantomData<(H, S)>, 
}

// impl<H, S, ST> PalantirIndex<H, S, ST> {
//     pub fn from
// }

impl<H, S, ST> SketchKVindex for PalantirIndex<H, S, ST> {
    
}