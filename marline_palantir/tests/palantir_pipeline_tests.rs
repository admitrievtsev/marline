use std::collections::HashMap;

use chunkfs::hashers::Sha256Hasher;
use chunkfs::{Data, DataContainer, Hasher, Scrub};
use marline_index::heuristic_index::SearchConfig;
use marline_palantir::encoder::GdeltaEncoder;
use marline_palantir::lifecycle_manager::LifecycleManager;
use marline_palantir::metadata_manager::MetadataManager;
use marline_palantir::palantir_scrubber::PalantirScrubber;
use marline_palantir::sf_generator::{PalantirHasher, SuperFeatureGenerator};
use marline_palantir::types::{BlockID, Chunk, TierConfig};

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
