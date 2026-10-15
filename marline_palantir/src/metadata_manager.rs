use crate::lifecycle_manager::{LifecycleManager, LifecycleTierConfig};
use crate::tables::{FPTable, SFTable};
use crate::types::{BlockID, SuperFeature, TierConfig};
use crate::utils::lcm_vec;
use marline_storage::ChunkHash;
use marline_index::heuristic_index::SearchConfig;
use marline_index::index::metrics::Metric;
use marline_index::index::IndexError;

pub type TierID = u8;

/// Metadata management for the Palantir deduplication pipeline.
///
/// This module will track chunk metadata such as versioning, base-chunk
/// relationships, and scrub statistics.  Currently a placeholder.
pub struct MetadataManager<H: ChunkHash + Send + Sync, const N: usize> {
    fp_table: FPTable<H>,
    sf_tables: [AnySFTable<H>; N],
    sf_counts: [usize; N],
    lifecycle_manager: LifecycleManager<N>,
}

impl<H: ChunkHash + Send + Sync, const N: usize> MetadataManager<H, N> {
    pub fn new(
        tier_config: TierConfig<N>,
        lifecycle_configs: [LifecycleTierConfig; N],
        search_config: &SearchConfig,
    ) -> Self {
        let features_num = if let Some(fn_val) = tier_config.features_num {
            fn_val
        } else {
            lcm_vec(&tier_config.tier_list).expect("tier_list LCM overflow") as usize
        };
        let sf_counts: [usize; N] =
            std::array::from_fn(|i| features_num / tier_config.tier_list[i] as usize);
        let sf_tables: [AnySFTable<H>; N] = std::array::from_fn(|i| {
            AnySFTable::from_index(sf_counts[i] as u32, search_config).expect("Invalid tier index")
        });
        Self {
            fp_table: FPTable::<H>::new(),
            sf_tables,
            sf_counts,
            lifecycle_manager: LifecycleManager::new(lifecycle_configs),
        }
    }

    /// Function for accurate deduplication
    pub fn lookup_fingerprint(&self, fingerprint: &H) -> Option<&BlockID<H>> {
        self.fp_table.lookup(fingerprint)
    }

    /// Function of searching for base block tier-by-tier
    pub fn lookup_super_features(
        &self,
        super_features: &[SuperFeature],
    ) -> Option<(BlockID<H>, TierID)> {
        let mut first_index: usize;
        let mut last_index: usize = 0;

        for i in 0..N {
            first_index = last_index;
            last_index += self.sf_counts[i];
            let slice = &super_features[first_index..last_index];

            let table = &self.sf_tables[i];
            if let Some(block) = table
                .get_with_upd_metric(slice, self.lifecycle_manager.tier_after_use_upd_fn(i as u8))
            {
                return Some((block, i as TierID));
            }
        }
        None
    }

    /// Add the block: FP + SF for all tiers
    pub fn add_block(
        &mut self,
        fingerprint: H,
        super_features: &[SuperFeature],
        block_id: BlockID<H>,
    ) {
        self.fp_table.insert(fingerprint, block_id.clone());

        let mut first_index: usize;
        let mut last_index: usize = 0;

        for i in 0..N {
            first_index = last_index;
            last_index += self.sf_counts[i];
            let slice = &super_features[first_index..last_index];
            let metric = self.lifecycle_manager.default_tier_metric(i as TierID);
            self.sf_tables[i].insert(&block_id, slice, metric);
        }
    }

    /// Finish version: update lifecycle metrics and remove expired entries
    pub fn finish_version(&self) -> Result<(), IndexError> {
        for (id, table) in self.sf_tables.iter().enumerate() {
            let upd_fn = self.lifecycle_manager.tier_between_backups_upd_fn(id as TierID);
            let filter_fn = self.lifecycle_manager.tier_drop_or_not_fn(id as TierID);
            table.update_and_clean(upd_fn, filter_fn)?;
        }
        Ok(())
    }

    pub fn fp_table_size(&self) -> usize {
        self.fp_table.len()
    }

    pub fn sf_table_size(&self) -> usize {
        self.sf_tables.iter().map(|table| table.len()).sum()
    }
}

impl<H: ChunkHash + Send + Sync> Default for MetadataManager<H, 3> {
    fn default() -> Self {
        Self::new(
            TierConfig::new([4, 3, 2]),
            <LifecycleManager<3>>::default_configs(),
            &SearchConfig::default(),
        )
    }
}

macro_rules! any_sf_method {
    ($self:ident, $method:ident ($($args:tt)*)) => {
        match $self {
            Self::T1(t) => t.$method($($args)*),
            Self::T2(t) => t.$method($($args)*),
            Self::T3(t) => t.$method($($args)*),
            Self::T4(t) => t.$method($($args)*),
            Self::T5(t) => t.$method($($args)*),
            Self::T6(t) => t.$method($($args)*),
            Self::T7(t) => t.$method($($args)*),
            Self::T8(t) => t.$method($($args)*),
            Self::T9(t) => t.$method($($args)*),
            Self::T10(t) => t.$method($($args)*),
            Self::T11(t) => t.$method($($args)*),
            Self::T12(t) => t.$method($($args)*),
        }
    };
}

pub enum AnySFTable<H: ChunkHash + Send + Sync> {
    T1(SFTable<H, 1>),
    T2(SFTable<H, 2>),
    T3(SFTable<H, 3>),
    T4(SFTable<H, 4>),
    T5(SFTable<H, 5>),
    T6(SFTable<H, 6>),
    T7(SFTable<H, 7>),
    T8(SFTable<H, 8>),
    T9(SFTable<H, 9>),
    T10(SFTable<H, 10>),
    T11(SFTable<H, 11>),
    T12(SFTable<H, 12>),
}

impl<H: ChunkHash + Send + Sync> AnySFTable<H> {
    pub fn from_index(index: u32, config: &SearchConfig) -> Option<Self> {
        match index {
            1 => Some(Self::T1(SFTable::new(config.clone()))),
            2 => Some(Self::T2(SFTable::new(config.clone()))),
            3 => Some(Self::T3(SFTable::new(config.clone()))),
            4 => Some(Self::T4(SFTable::new(config.clone()))),
            5 => Some(Self::T5(SFTable::new(config.clone()))),
            6 => Some(Self::T6(SFTable::new(config.clone()))),
            7 => Some(Self::T7(SFTable::new(config.clone()))),
            8 => Some(Self::T8(SFTable::new(config.clone()))),
            9 => Some(Self::T9(SFTable::new(config.clone()))),
            10 => Some(Self::T10(SFTable::new(config.clone()))),
            11 => Some(Self::T11(SFTable::new(config.clone()))),
            12 => Some(Self::T12(SFTable::new(config.clone()))),
            _ => None,
        }
    }

    pub fn nearest(&self, features: &[SuperFeature]) -> Option<BlockID<H>> {
        any_sf_method!(self, nearest(features))
    }

    pub fn insert(&self, block_id: &BlockID<H>, features: &[SuperFeature], metric: Metric) {
        any_sf_method!(self, insert(block_id, features, metric))
    }

    pub fn len(&self) -> usize {
        any_sf_method!(self, len())
    }

    pub fn is_empty(&self) -> bool {
        any_sf_method!(self, is_empty())
    }

    pub fn get_with_upd_metric(
        &self,
        features: &[SuperFeature],
        f: impl FnOnce(Metric) -> Metric,
    ) -> Option<BlockID<H>> {
        any_sf_method!(self, get_with_upd_metric(features, f))
    }

    pub fn update_and_clean(
        &self,
        update_fn: impl FnMut(Metric) -> Metric,
        cleanup_fn: impl Fn(Metric) -> bool,
    ) -> Result<(), IndexError> {
        any_sf_method!(self, update_and_clean(update_fn, cleanup_fn))
    }
}
