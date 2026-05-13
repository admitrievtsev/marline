use crate::chunkfs_sbc::{ClusterPoint, Clusters, EqCluster};
use crate::clusterer::Clusterer;
use crate::SBCHash;
use chunkfs::ClusteringMeasurements;
use std::collections::HashMap;

pub struct EqClusterer;

impl<Hash: SBCHash + std::fmt::Debug> Clusterer<Hash> for EqClusterer {
    fn clusterize<'a>(
        &mut self,
        chunk_sbc_hash: Vec<ClusterPoint<'a, Hash>>,
    ) -> (Clusters<'a, Hash>, ClusteringMeasurements) {
        let mut clusters: Clusters<Hash> = Clusters(HashMap::default());
        let mut total_cluster_size: usize = 0;
        let number_of_vertices_in_cluster = HashMap::new();
        for (sbc_hash, data_container) in chunk_sbc_hash {
            let cluster = clusters.partial_search(sbc_hash.clone(), 3);
            //let cluster = clusters.0.entry(sbc_hash.clone()).or_insert(vec![]);

            cluster.push((sbc_hash, data_container));
            total_cluster_size += 1;
        }
        let distance_to_other_clusters = HashMap::new();
        let distance_to_vertices_in_cluster = HashMap::new();
        let cluster_dedup_ratio = HashMap::new();
        let number_of_clusters = total_cluster_size;

        let clusterization_report = ClusteringMeasurements {
            total_cluster_size,
            number_of_clusters,
            number_of_vertices_in_cluster,
            distance_to_vertices_in_cluster,
            distance_to_other_clusters,
            cluster_dedup_ratio,
        };

        (clusters, clusterization_report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decoder, encoder, hasher, SBCMap, SBCScrubber};
    use chunkfs::chunkers::{SizeParams, SuperChunker};
    use chunkfs::hashers::Sha256Hasher;
    use chunkfs::FileSystem;

    fn generate_test_data() -> Vec<u8> {
        const TEST_DATA_SIZE: usize = 16000;
        (0..TEST_DATA_SIZE).map(|_| rand::random::<u8>()).collect()
    }

    #[test]
    fn scrub_should_return_correct_scrub_measurements_for_eq_clusterer() {
        let test_data = generate_test_data();
        let chunk_size = SizeParams::new(2 * 1024, 8 * 1024, 16 * 1024);

        let mut fs = FileSystem::new_with_scrubber(
            HashMap::default(),
            SBCMap::new(decoder::GdeltaDecoder::new(false)),
            Box::new(SBCScrubber::new(
                hasher::AronovichHasher,
                EqClusterer,
                encoder::GdeltaEncoder::new(false),
            )),
            Sha256Hasher::default(),
        );

        let mut handle = fs
            .create_file("file".to_string(), SuperChunker::new(chunk_size))
            .unwrap();
        fs.write_to_file(&mut handle, &test_data).unwrap();
        fs.close_file(handle).unwrap();

        let scrub_report = fs.scrub().unwrap();

        let cluster_report = &scrub_report.clusterization_report;
        assert!(cluster_report.total_cluster_size > 0);
        assert!(cluster_report.number_of_clusters > 0);
        assert!(cluster_report
            .number_of_vertices_in_cluster
            .values()
            .all(|&v| v == 1));
        assert!(cluster_report.distance_to_vertices_in_cluster.is_empty());
        assert!(cluster_report
            .distance_to_other_clusters
            .values()
            .all(|v| !v.is_empty()));
        assert!(cluster_report
            .cluster_dedup_ratio
            .values()
            .all(|&v| v == 0.0));
    }
}
