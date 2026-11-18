use std::collections::HashMap;

use chunkfs::hashers::Sha256Hasher;
use chunkfs::{Data, DataContainer, Database, Hasher, IterableDatabase, Scrub};
use marline_index::heuristic_index::SearchConfig;
use marline_palantir::encoder::{GdeltaEncoder, PalantirEncoder};
use marline_palantir::lifecycle_manager::LifecycleManager;
use marline_palantir::metadata_manager::MetadataManager;
use marline_palantir::palantir_scrubber::PalantirScrubber;
use marline_palantir::sf_generator::{PalantirHasher, SuperFeatureGenerator};
use marline_palantir::types::{BlockID, Chunk, SuperFeature, TierConfig};
use std::io;

fn hash_data(data: &[u8]) -> [u8; 32] {
    Sha256Hasher::default().hash(data)
}

#[test]
fn identical_chunks() {
    let sf_gen = PalantirHasher::new(7, vec![4, 3, 2]);
    let tier_config = TierConfig::new([4, 3, 2]);
    let lifecycle_configs = LifecycleManager::<3>::default_configs();
    let mut mm = MetadataManager::<[u8; 32], 3>::new(
        tier_config,
        lifecycle_configs,
        &SearchConfig::default(),
    );

    let base: Vec<u8> = b"This is a base chunk for the Palantir similarity pipeline. \
                           It has enough bytes for the gear-hash rolling hash to produce \
                           stable super-features across multiple tiers."
        .to_vec();
    let hash = hash_data(&base);

    let chunk = Chunk::new(base);
    let sfs = sf_gen.generate(&chunk);
    mm.add_block(hash, &sfs, BlockID::new(hash));

    let found = mm.lookup_fingerprint(&hash);
    assert!(found.is_some(), "Identical chunk not found via fingerprint lookup");
}

#[test]
fn similar_chunks() {
    let sf_gen = PalantirHasher::new(7, vec![4, 3, 2]);
    let encoder = GdeltaEncoder;
    let tier_config = TierConfig::new([4, 3, 2]);
    let lifecycle_configs = LifecycleManager::<3>::default_configs();
    let mut scrubber = PalantirScrubber::new(
        sf_gen,
        encoder,
        tier_config,
        lifecycle_configs,
        SearchConfig::default(),
    );

    let base: Vec<u8> = (0..8192).map(|i| (i % 256) as u8).collect();
    let mut similar = base.clone();
    for item in similar[4096..4916].iter_mut() {
        *item = item.wrapping_add(7);
    }

    let hash_base = hash_data(&base);
    let hash_similar = hash_data(&similar);

    let mut database = HashMap::new();
    database.insert(hash_base, DataContainer::from(base.clone()));
    database.insert(hash_similar, DataContainer::from(similar.clone()));

    let mut target_map = HashMap::new();
    let result = scrubber.scrub(&mut database, &mut target_map).unwrap();

    assert_eq!(result.processed_data, base.len() + similar.len());
    assert_eq!(target_map.len(), 2);

    let delta_count = [(&hash_base, &base), (&hash_similar, &similar)]
        .into_iter()
        .filter(|(h, data)| target_map.get(*h).unwrap().len() < data.len())
        .count();
    assert!(
        delta_count >= 1,
        "At least one chunk should be delta-encoded, got {} deltas",
        delta_count
    );

    for (hash, data) in [(&hash_base, &base), (&hash_similar, &similar)] {
        let v = target_map.get(hash).unwrap();
        if v.len() < data.len() {
            let delta_zstd = zstd::encode_all(v.as_slice(), 0).unwrap();
            let raw_zstd = zstd::encode_all(data.as_slice(), 0).unwrap();
            let ratio = delta_zstd.len() as f64 / raw_zstd.len() as f64;
            assert!(ratio < 0.9, "Filter should preserve delta, ratio={:.2}", ratio);
            break;
        }
    }

    for hash in [&hash_base, &hash_similar] {
        match database.get(hash).unwrap().extract() {
            Data::TargetChunk(keys) => assert_eq!(keys.as_slice(), &[*hash]),
            _ => panic!("Expected TargetChunk after scrub"),
        }
    }
}

struct OrderedDatabase(Vec<([u8; 32], DataContainer<[u8; 32]>)>);

impl Database<[u8; 32], DataContainer<[u8; 32]>> for OrderedDatabase {
    fn insert(&mut self, key: [u8; 32], value: DataContainer<[u8; 32]>) -> io::Result<()> {
        if let Some((_, existing)) = self.0.iter_mut().find(|(existing, _)| *existing == key) {
            *existing = value;
        } else {
            self.0.push((key, value));
        }
        Ok(())
    }

    fn get(&self, key: &[u8; 32]) -> io::Result<DataContainer<[u8; 32]>> {
        self.0
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "key not found"))
    }

    fn contains(&self, key: &[u8; 32]) -> bool {
        self.0.iter().any(|(existing, _)| existing == key)
    }
}

impl IterableDatabase<[u8; 32], DataContainer<[u8; 32]>> for OrderedDatabase {
    fn iterator(&self) -> Box<dyn Iterator<Item = (&[u8; 32], &DataContainer<[u8; 32]>)> + '_> {
        Box::new(self.0.iter().map(|(key, value)| (key, value)))
    }

    fn iterator_mut(
        &mut self,
    ) -> Box<dyn Iterator<Item = (&[u8; 32], &mut DataContainer<[u8; 32]>)> + '_> {
        Box::new(self.0.iter_mut().map(|(key, value)| (&*key, value)))
    }

    fn clear(&mut self) -> io::Result<()> {
        self.0.clear();
        Ok(())
    }
}

#[derive(Clone)]
struct FixedFeatures(Vec<SuperFeature>);

impl SuperFeatureGenerator for FixedFeatures {
    fn generate(&self, _: &Chunk) -> Vec<SuperFeature> {
        self.0.clone()
    }

    fn generate_with_table(&self, chunk: &Chunk) -> (Vec<SuperFeature>, HashMap<u32, u32>) {
        (self.generate(chunk), HashMap::new())
    }

    fn generate_from_raw(&self, _: &[u32]) -> Vec<SuperFeature> {
        self.0.clone()
    }
}

struct FixedDeltaEncoder(usize);

impl PalantirEncoder for FixedDeltaEncoder {
    fn encode(&self, _: &[u8], _: &[u8]) -> Vec<u8> {
        vec![0; self.0]
    }

    fn encode_below_limit(&self, _: &[u8], _: &[u8], limit: usize) -> Option<Vec<u8>> {
        (self.0 < limit).then(|| vec![0; self.0])
    }

    fn encode_with_manifest(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        _: &HashMap<u64, u32>,
    ) -> Vec<u8> {
        self.encode(new_chunk, base_chunk)
    }

    fn encode_with_manifest_below_limit(
        &self,
        new_chunk: &[u8],
        base_chunk: &[u8],
        _: &HashMap<u64, u32>,
        limit: usize,
    ) -> Option<Vec<u8>> {
        self.encode_below_limit(new_chunk, base_chunk, limit)
    }

    fn generate_fp_table(_: &[u8]) -> Option<HashMap<u64, u32>> {
        None
    }
}

fn alpha_case(delta_len: usize, early_alpha: bool) -> (u64, HashMap<[u8; 32], Vec<u8>>, Vec<u8>) {
    let features = vec![SuperFeature::new(0, 7)];
    let base: Vec<u8> = (0..1000).map(|i| ((i * 31 + i / 7) % 251) as u8).collect();
    let target: Vec<u8> = (0..1000).map(|i| ((i * 73 + i / 11 + 17) % 251) as u8).collect();
    let base_hash = [1; 32];
    let target_hash = [2; 32];
    let mut scrubber = PalantirScrubber::new(
        FixedFeatures(features),
        FixedDeltaEncoder(delta_len),
        TierConfig::new([1]),
        LifecycleManager::<1>::default_configs(),
        SearchConfig::default(),
    );
    scrubber.set_delta_promote_threshold(Some(0.01));
    scrubber.set_early_alpha(early_alpha);
    let mut database = OrderedDatabase(vec![
        (base_hash, DataContainer::from(base)),
        (target_hash, DataContainer::from(target.clone())),
    ]);
    let mut target_map = HashMap::new();

    scrubber.scrub(&mut database, &mut target_map).unwrap();
    (scrubber.delta_stored(), target_map, target)
}

#[test]
fn alpha_boundary_is_deterministic_and_independent_of_early_encoding() {
    // ceil(0.01 * 1000) = 10: 9 bytes stay a delta; 10 bytes promote to base.
    let delta_early = alpha_case(9, true);
    let delta_full = alpha_case(9, false);
    assert_eq!(delta_early, delta_full);
    assert_eq!(delta_early.0, 1);
    assert_ne!(delta_early.1.get(&[2; 32]), Some(&delta_early.2));

    let base_early = alpha_case(10, true);
    let base_full = alpha_case(10, false);
    assert_eq!(base_early, base_full);
    assert_eq!(base_early.0, 0);
    assert_eq!(base_early.1.get(&[2; 32]), Some(&base_early.2));
}
