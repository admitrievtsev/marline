use crate::index::error::IndexError;

#[allow(unused)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    One,
    Two,
    Three,
}

#[allow(unused)]
pub trait Store<H, S>: Send + Sync
where
    H: Clone + Send + Sync,
{
    /// hash → Sketch ---
    fn get_sketch(&self, hash: &H) -> Result<Option<S>, IndexError>;
    fn put_sketch(&self, hash: &H, sketch: &S) -> Result<(), IndexError>;

    /// superfeature → Vec<Hash> ---
    fn get_inverted(&self, tier: Tier, sf: u64) -> Result<Vec<H>, IndexError>;
    fn add_inverted(&self, tier: Tier, sf: u64, hash: &H) -> Result<(), IndexError>;
    fn remove_inverted(&self, tier: Tier, sf: u64, hash: &H) -> Result<(), IndexError>;

    ///
    fn len_sketches(&self) -> Result<usize, IndexError>;
    fn len_inverted(&self, tier: Tier) -> Result<usize, IndexError>;
}